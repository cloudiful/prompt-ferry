use std::sync::Mutex;

use rmcp::{
    ErrorData,
    model::{
        ClientNotification, ClientRequest, CompleteResult, ErrorCode, ListResourceTemplatesResult,
        ServerInfo, ServerResult, SetLevelRequestMethod, SubscribeRequestMethod,
        UnsubscribeRequestMethod,
    },
    service::{NotificationContext, RequestContext, RoleServer, Service},
};

use crate::db::McpServer;

use super::{McpCatalogCache, aggregate, filtering, targeting::load_visible_server};

mod ops;
mod value;

use value::{internal_error, server_info};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct RequestScope {
    pub(super) user_id: Option<i64>,
    pub(super) server_name: Option<String>,
    pub(super) conversation_id: Option<String>,
}

pub(super) struct ProxyService {
    pool: sqlx::PgPool,
    cache: McpCatalogCache,
    session_scope: Mutex<Option<RequestScope>>,
}

impl ProxyService {
    pub(super) fn new(pool: sqlx::PgPool, cache: McpCatalogCache) -> Self {
        Self {
            pool,
            cache,
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
                &self.pool,
                &self.cache,
                scope.user_id,
                scope.conversation_id.as_deref(),
                request,
            )
            .await
            .map_err(internal_error);
        }

        let server = self.load_server(scope).await?;
        filtering::call_server_filtered(&server, request, scope.conversation_id.as_deref())
            .await
            .map_err(internal_error)
    }

    async fn load_server(&self, scope: &RequestScope) -> Result<McpServer, ErrorData> {
        let Some(server_name) = scope.server_name.as_deref() else {
            return Err(ErrorData::internal_error("missing MCP server name", None));
        };
        self.load_server_by_name(scope.user_id, server_name).await
    }

    async fn load_server_by_name(
        &self,
        user_id: Option<i64>,
        server_name: &str,
    ) -> Result<McpServer, ErrorData> {
        load_visible_server(&self.pool, user_id, server_name)
            .await
            .map_err(internal_error)?
            .ok_or_else(|| ErrorData::invalid_params("mcp server not found or disabled", None))
    }
}

impl Service<RoleServer> for ProxyService {
    async fn handle_request(
        &self,
        request: ClientRequest,
        context: RequestContext<RoleServer>,
    ) -> Result<ServerResult, ErrorData> {
        match request {
            ClientRequest::InitializeRequest(request) => {
                if context.peer.peer_info().is_none() {
                    context.peer.set_peer_info(request.params);
                }
                self.bind_scope(&context.extensions)?;
                Ok(ServerResult::InitializeResult(server_info()))
            }
            ClientRequest::PingRequest(_) => Ok(ServerResult::empty(())),
            ClientRequest::CompleteRequest(_) => {
                Ok(ServerResult::CompleteResult(CompleteResult::default()))
            }
            ClientRequest::SetLevelRequest(_) => {
                Err(ErrorData::method_not_found::<SetLevelRequestMethod>())
            }
            ClientRequest::ListToolsRequest(request) => {
                let scope = self.bind_scope(&context.extensions)?;
                Ok(ServerResult::ListToolsResult(
                    self.list_tools_for_scope(&scope, &context.id, request.params)
                        .await?,
                ))
            }
            ClientRequest::CallToolRequest(request) => {
                let scope = self.bind_scope(&context.extensions)?;
                Ok(ServerResult::CallToolResult(
                    self.call_tool_for_scope(&scope, &context.id, request.params)
                        .await?,
                ))
            }
            ClientRequest::ListResourcesRequest(request) => {
                let scope = self.bind_scope(&context.extensions)?;
                Ok(ServerResult::ListResourcesResult(
                    self.list_resources_for_scope(&scope, &context.id, request.params)
                        .await?,
                ))
            }
            ClientRequest::ListResourceTemplatesRequest(_) => Ok(
                ServerResult::ListResourceTemplatesResult(ListResourceTemplatesResult::default()),
            ),
            ClientRequest::ReadResourceRequest(request) => {
                let scope = self.bind_scope(&context.extensions)?;
                Ok(ServerResult::ReadResourceResult(
                    self.read_resource_for_scope(&scope, &context.id, request.params)
                        .await?,
                ))
            }
            ClientRequest::SubscribeRequest(_) => {
                Err(ErrorData::method_not_found::<SubscribeRequestMethod>())
            }
            ClientRequest::UnsubscribeRequest(_) => {
                Err(ErrorData::method_not_found::<UnsubscribeRequestMethod>())
            }
            ClientRequest::ListPromptsRequest(request) => {
                let scope = self.bind_scope(&context.extensions)?;
                Ok(ServerResult::ListPromptsResult(
                    self.list_prompts_for_scope(&scope, &context.id, request.params)
                        .await?,
                ))
            }
            ClientRequest::GetPromptRequest(request) => {
                let scope = self.bind_scope(&context.extensions)?;
                Ok(ServerResult::GetPromptResult(
                    self.get_prompt_for_scope(&scope, &context.id, request.params)
                        .await?,
                ))
            }
            ClientRequest::CustomRequest(request) => Err(ErrorData::new(
                ErrorCode::METHOD_NOT_FOUND,
                request.method,
                None,
            )),
            ClientRequest::ListTasksRequest(_)
            | ClientRequest::GetTaskRequest(_)
            | ClientRequest::GetTaskPayloadRequest(_)
            | ClientRequest::CancelTaskRequest(_) => {
                Err(ErrorData::new(ErrorCode::METHOD_NOT_FOUND, "tasks", None))
            }
        }
    }

    async fn handle_notification(
        &self,
        notification: ClientNotification,
        context: NotificationContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        if matches!(notification, ClientNotification::InitializedNotification(_)) {
            self.bind_scope(&context.extensions)?;
        }
        Ok(())
    }

    fn get_info(&self) -> ServerInfo {
        server_info()
    }
}
