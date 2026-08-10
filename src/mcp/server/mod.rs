use std::{borrow::Cow, sync::Mutex};

use rmcp::{
    ErrorData, ServerHandler,
    model::{
        CallToolRequestParams, CallToolResponse, CompleteRequestParams, CompleteResult,
        GetPromptRequestParams, GetPromptResponse, InitializeRequestParams, InitializeResult,
        ListPromptsResult, ListResourceTemplatesResult, ListResourcesResult, ListToolsResult,
        PaginatedRequestParams, ProtocolVersion, ReadResourceRequestParams, ReadResourceResponse,
        ServerInfo,
    },
    service::{NotificationContext, RequestContext, RoleServer},
};

use crate::db::McpServer;

use super::{McpCatalogCache, aggregate, filtering, targeting::load_visible_server};

mod ops;
mod value;

use value::{internal_error, server_info};

#[derive(Clone)]
pub(super) struct RequestScope {
    pub(super) user_id: Option<i64>,
    pub(super) server_name: Option<String>,
    pub(super) conversation_id: Option<String>,
    pub(super) pool: sqlx::PgPool,
    pub(super) cache: McpCatalogCache,
    pub(super) selected_credential: Option<crate::db::McpCredential>,
}

pub(super) struct ProxyService {
    session_scope: Mutex<Option<RequestScope>>,
}

impl ProxyService {
    pub(super) fn new() -> Self {
        Self {
            session_scope: Mutex::new(None),
        }
    }

    fn bind_scope(&self, extensions: &rmcp::model::Extensions) -> Result<RequestScope, ErrorData> {
        let parts = extensions
            .get::<http::request::Parts>()
            .ok_or_else(|| ErrorData::internal_error("missing HTTP request parts", None))?;
        let scope = parts
            .extensions
            .get::<RequestScope>()
            .cloned()
            .ok_or_else(|| ErrorData::internal_error("missing MCP request scope", None))?;

        let mut guard = self
            .session_scope
            .lock()
            .map_err(|_| ErrorData::internal_error("mcp session lock poisoned", None))?;
        match guard.as_ref() {
            Some(bound) if bound.server_name != scope.server_name => Err(
                ErrorData::invalid_params("MCP session target mismatch", None),
            ),
            Some(bound) if bound.user_id != scope.user_id => {
                Err(ErrorData::invalid_params("MCP session user mismatch", None))
            }
            Some(bound) if bound.conversation_id != scope.conversation_id => Err(
                ErrorData::invalid_params("MCP session conversation mismatch", None),
            ),
            Some(_) => Ok(scope),
            None => {
                *guard = Some(scope.clone());
                Ok(scope)
            }
        }
    }

    async fn dispatch(
        &self,
        scope: &RequestScope,
        request: serde_json::Value,
    ) -> Result<serde_json::Value, ErrorData> {
        if scope.server_name.is_none() {
            return aggregate::aggregate(
                &scope.pool,
                &scope.cache,
                scope.user_id,
                scope.conversation_id.as_deref(),
                request,
            )
            .await
            .map_err(internal_error);
        }

        let server = self.load_server(scope).await?;
        filtering::call_server_filtered(
            &server,
            request,
            scope.conversation_id.as_deref(),
            scope.selected_credential.as_ref(),
        )
        .await
        .map_err(internal_error)
    }

    async fn load_server(&self, scope: &RequestScope) -> Result<McpServer, ErrorData> {
        let Some(server_name) = scope.server_name.as_deref() else {
            return Err(ErrorData::internal_error("missing MCP server name", None));
        };
        self.load_server_by_name(scope.user_id, server_name, &scope.pool)
            .await
    }

    async fn load_server_by_name(
        &self,
        user_id: Option<i64>,
        server_name: &str,
        pool: &sqlx::PgPool,
    ) -> Result<McpServer, ErrorData> {
        load_visible_server(pool, user_id, server_name)
            .await
            .map_err(internal_error)?
            .ok_or_else(|| ErrorData::invalid_params("mcp server not found or disabled", None))
    }
}

impl ServerHandler for ProxyService {
    async fn initialize(
        &self,
        request: InitializeRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<InitializeResult, ErrorData> {
        if context.peer.peer_info().is_none() {
            context.peer.set_peer_info(request.clone());
        }
        self.bind_scope(&context.extensions)?;
        let mut info = server_info();
        if self
            .supported_protocol_versions()
            .contains(&request.protocol_version)
        {
            info.protocol_version = request.protocol_version;
        }
        Ok(info)
    }

    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Borrowed(&[ProtocolVersion::V_2026_07_28, ProtocolVersion::V_2025_11_25])
    }

    async fn on_initialized(&self, context: NotificationContext<RoleServer>) {
        if let Err(err) = self.bind_scope(&context.extensions) {
            tracing::warn!(error = %err, "mcp session scope bind failed on initialized notification");
        }
    }

    async fn list_tools(
        &self,
        request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let scope = self.bind_scope(&context.extensions)?;
        self.list_tools_for_scope(&scope, &context.id, request)
            .await
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let scope = self.bind_scope(&context.extensions)?;
        self.call_tool_for_scope(&scope, &context.id, request, context.meta)
            .await
    }

    async fn list_resources(
        &self,
        request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        let scope = self.bind_scope(&context.extensions)?;
        self.list_resources_for_scope(&scope, &context.id, request)
            .await
    }

    async fn list_resource_templates(
        &self,
        request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, ErrorData> {
        let scope = self.bind_scope(&context.extensions)?;
        self.list_resource_templates_for_scope(&scope, &context.id, request)
            .await
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        let scope = self.bind_scope(&context.extensions)?;
        self.read_resource_for_scope(&scope, &context.id, request, context.meta)
            .await
    }

    async fn complete(
        &self,
        request: CompleteRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CompleteResult, ErrorData> {
        let scope = self.bind_scope(&context.extensions)?;
        self.complete_for_scope(&scope, &context.id, request, context.meta)
            .await
    }

    async fn list_prompts(
        &self,
        request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, ErrorData> {
        let scope = self.bind_scope(&context.extensions)?;
        self.list_prompts_for_scope(&scope, &context.id, request)
            .await
    }

    async fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<GetPromptResponse, ErrorData> {
        let scope = self.bind_scope(&context.extensions)?;
        self.get_prompt_for_scope(&scope, &context.id, request, context.meta)
            .await
    }

    fn get_info(&self) -> ServerInfo {
        server_info()
    }
}
