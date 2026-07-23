use rmcp::{
    ErrorData,
    model::{
        CallToolRequestParams, GetPromptRequestParams, GetPromptResult, ListPromptsResult,
        ListResourcesResult, ListToolsResult, PaginatedRequestParams, ReadResourceRequestParams,
        ReadResourceResult, RequestId,
    },
};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::db::McpServer;

use super::{
    ProxyService, RequestScope, filtering,
    value::{json_request, optional_params, parse_result, parse_result_field, required_params},
};
use crate::mcp::targeting::{PrefixedTarget, parse_prefixed_name, parse_resource_target};

struct AggregateCallContext<'a> {
    user_id: Option<i64>,
    conversation_id: Option<&'a str>,
    request_id: &'a RequestId,
    method: &'a str,
}

impl ProxyService {
    pub(super) async fn list_tools_for_scope(
        &self,
        scope: &RequestScope,
        request_id: &RequestId,
        params: Option<PaginatedRequestParams>,
    ) -> Result<ListToolsResult, ErrorData> {
        self.list_result(scope, request_id, "tools/list", "tools", params)
            .await
            .map(ListToolsResult::with_all_items)
    }

    pub(super) async fn list_resources_for_scope(
        &self,
        scope: &RequestScope,
        request_id: &RequestId,
        params: Option<PaginatedRequestParams>,
    ) -> Result<ListResourcesResult, ErrorData> {
        self.list_result(scope, request_id, "resources/list", "resources", params)
            .await
            .map(ListResourcesResult::with_all_items)
    }

    pub(super) async fn list_prompts_for_scope(
        &self,
        scope: &RequestScope,
        request_id: &RequestId,
        params: Option<PaginatedRequestParams>,
    ) -> Result<ListPromptsResult, ErrorData> {
        self.list_result(scope, request_id, "prompts/list", "prompts", params)
            .await
            .map(ListPromptsResult::with_all_items)
    }

    pub(super) async fn call_tool_for_scope(
        &self,
        scope: &RequestScope,
        request_id: &RequestId,
        params: CallToolRequestParams,
    ) -> Result<rmcp::model::CallToolResult, ErrorData> {
        if scope.server_name.is_some() {
            return self
                .dispatch_result(scope, request_id, "tools/call", params)
                .await;
        }

        let Some(target) = parse_prefixed_name(params.name.as_ref()) else {
            return Err(ErrorData::invalid_params("name must be server__name", None));
        };
        self.forward_aggregate_call(
            AggregateCallContext {
                user_id: scope.user_id,
                conversation_id: scope.conversation_id.as_deref(),
                request_id,
                method: "tools/call",
            },
            params,
            target,
            |params, upstream_name| params.name = upstream_name.into(),
            |server, upstream_name| {
                if filtering::is_disabled_item(server, "tools", upstream_name) {
                    return Err(ErrorData::invalid_params("tool is disabled", None));
                }
                Ok(())
            },
        )
        .await
    }

    pub(super) async fn read_resource_for_scope(
        &self,
        scope: &RequestScope,
        request_id: &RequestId,
        params: ReadResourceRequestParams,
    ) -> Result<ReadResourceResult, ErrorData> {
        if scope.server_name.is_some() {
            return self
                .dispatch_result(scope, request_id, "resources/read", params)
                .await;
        }

        let Some(target) = parse_resource_target(&params.uri).map_err(super::internal_error)?
        else {
            return Err(ErrorData::invalid_params(
                "uri must start with mcp://server/",
                None,
            ));
        };
        self.forward_aggregate_call(
            AggregateCallContext {
                user_id: scope.user_id,
                conversation_id: scope.conversation_id.as_deref(),
                request_id,
                method: "resources/read",
            },
            params,
            target,
            |params, upstream_name| params.uri = upstream_name,
            |server, upstream_name| {
                if filtering::is_disabled_item(server, "resources", upstream_name) {
                    return Err(ErrorData::invalid_params("resource is disabled", None));
                }
                Ok(())
            },
        )
        .await
    }

    pub(super) async fn get_prompt_for_scope(
        &self,
        scope: &RequestScope,
        request_id: &RequestId,
        params: GetPromptRequestParams,
    ) -> Result<GetPromptResult, ErrorData> {
        if scope.server_name.is_some() {
            return self
                .dispatch_result(scope, request_id, "prompts/get", params)
                .await;
        }

        let Some(target) = parse_prefixed_name(&params.name) else {
            return Err(ErrorData::invalid_params("name must be server__name", None));
        };
        self.forward_aggregate_call(
            AggregateCallContext {
                user_id: scope.user_id,
                conversation_id: scope.conversation_id.as_deref(),
                request_id,
                method: "prompts/get",
            },
            params,
            target,
            |params, upstream_name| params.name = upstream_name,
            |_, _| Ok(()),
        )
        .await
    }

    async fn list_result<T>(
        &self,
        scope: &RequestScope,
        request_id: &RequestId,
        method: &str,
        field: &str,
        params: Option<PaginatedRequestParams>,
    ) -> Result<Vec<T>, ErrorData>
    where
        T: DeserializeOwned,
    {
        let response = if scope.server_name.is_some() {
            self.cached_server_list(scope, request_id, field).await?
        } else {
            self.dispatch(
                scope,
                json_request(request_id, method, optional_params(params)?),
            )
            .await?
        };
        parse_result_field(&response, field)
    }

    async fn cached_server_list(
        &self,
        scope: &RequestScope,
        request_id: &RequestId,
        field: &str,
    ) -> Result<Value, ErrorData> {
        let server = self.load_server(scope).await?;
        let snapshot = self
            .cache
            .get(&server)
            .await
            .ok_or_else(|| ErrorData::internal_error("mcp catalog is not ready", None))?;
        let items = match field {
            "tools" => snapshot.tools,
            "resources" => snapshot.resources,
            "prompts" => snapshot.prompts,
            _ => return Err(ErrorData::internal_error("unknown MCP catalog field", None)),
        };
        let mut result = serde_json::Map::new();
        result.insert(field.to_string(), Value::Array(items));
        Ok(json!({
            "jsonrpc": "2.0",
            "id": serde_json::to_value(request_id).map_err(super::internal_error)?,
            "result": result,
        }))
    }

    async fn dispatch_result<T, P>(
        &self,
        scope: &RequestScope,
        request_id: &RequestId,
        method: &str,
        params: P,
    ) -> Result<T, ErrorData>
    where
        T: DeserializeOwned,
        P: serde::Serialize,
    {
        let response = self
            .dispatch(
                scope,
                json_request(request_id, method, required_params(params)?),
            )
            .await?;
        parse_result(response)
    }

    async fn forward_aggregate_call<T, P, Rewrite, Validate>(
        &self,
        context: AggregateCallContext<'_>,
        mut params: P,
        target: PrefixedTarget,
        rewrite: Rewrite,
        validate: Validate,
    ) -> Result<T, ErrorData>
    where
        T: DeserializeOwned,
        P: serde::Serialize,
        Rewrite: FnOnce(&mut P, String),
        Validate: FnOnce(&McpServer, &str) -> Result<(), ErrorData>,
    {
        let server = self
            .load_server_by_name(context.user_id, &target.server_name)
            .await?;
        validate(&server, &target.upstream_name)?;
        rewrite(&mut params, target.upstream_name);
        let response = filtering::call_server_filtered(
            &server,
            json_request(context.request_id, context.method, required_params(params)?),
            context.conversation_id,
        )
        .await
        .map_err(super::internal_error)?;
        parse_result(response)
    }
}
