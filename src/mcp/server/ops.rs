use rmcp::{
    ErrorData,
    model::{
        CacheScope, CallToolRequestParams, CallToolResponse, CompleteRequestParams, CompleteResult,
        GetPromptRequestParams, GetPromptResponse, ListPromptsResult, ListResourceTemplatesResult,
        ListResourcesResult, ListToolsResult, PaginatedRequestParams, ProtocolVersion,
        ReadResourceRequestParams, ReadResourceResponse, Reference, RequestId, RequestMetaObject,
    },
};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::db::McpServer;

use super::{
    McpRuntimeStorage, ProxyService, RequestScope, filtering,
    value::{
        json_request, optional_params, parse_call_tool_response, parse_get_prompt_response,
        parse_read_resource_response, parse_result, parse_result_field, required_params, with_meta,
    },
};
use crate::mcp::targeting::{
    PrefixedTarget, parse_prefixed_name, parse_resource_target, parse_resource_template_target,
};

struct AggregateCallContext<'a> {
    user_id: Option<i64>,
    conversation_id: Option<&'a str>,
    request_id: &'a RequestId,
    method: &'a str,
    storage: &'a McpRuntimeStorage,
    selected_credential: Option<crate::db::McpCredential>,
}

impl ProxyService {
    pub(super) async fn list_tools_for_scope(
        &self,
        scope: &RequestScope,
        request_id: &RequestId,
        params: Option<PaginatedRequestParams>,
        protocol_version: Option<&ProtocolVersion>,
    ) -> Result<ListToolsResult, ErrorData> {
        let items = self
            .list_result(scope, request_id, "tools/list", "tools", params)
            .await?;
        Ok(apply_cache_metadata(
            ListToolsResult::with_all_items(items),
            protocol_version,
        ))
    }

    pub(super) async fn list_resources_for_scope(
        &self,
        scope: &RequestScope,
        request_id: &RequestId,
        params: Option<PaginatedRequestParams>,
        protocol_version: Option<&ProtocolVersion>,
    ) -> Result<ListResourcesResult, ErrorData> {
        let items = self
            .list_result(scope, request_id, "resources/list", "resources", params)
            .await?;
        Ok(apply_cache_metadata(
            ListResourcesResult::with_all_items(items),
            protocol_version,
        ))
    }

    pub(super) async fn list_resource_templates_for_scope(
        &self,
        scope: &RequestScope,
        request_id: &RequestId,
        params: Option<PaginatedRequestParams>,
        protocol_version: Option<&ProtocolVersion>,
    ) -> Result<ListResourceTemplatesResult, ErrorData> {
        let items = self
            .list_result(
                scope,
                request_id,
                "resources/templates/list",
                "resourceTemplates",
                params,
            )
            .await?;
        Ok(apply_cache_metadata(
            ListResourceTemplatesResult::with_all_items(items),
            protocol_version,
        ))
    }

    pub(super) async fn list_prompts_for_scope(
        &self,
        scope: &RequestScope,
        request_id: &RequestId,
        params: Option<PaginatedRequestParams>,
        protocol_version: Option<&ProtocolVersion>,
    ) -> Result<ListPromptsResult, ErrorData> {
        let items = self
            .list_result(scope, request_id, "prompts/list", "prompts", params)
            .await?;
        Ok(apply_cache_metadata(
            ListPromptsResult::with_all_items(items),
            protocol_version,
        ))
    }

    pub(super) async fn call_tool_for_scope(
        &self,
        scope: &RequestScope,
        request_id: &RequestId,
        params: CallToolRequestParams,
        meta: RequestMetaObject,
    ) -> Result<CallToolResponse, ErrorData> {
        if scope.server_name.is_some() {
            return self
                .dispatch_result(
                    scope,
                    request_id,
                    "tools/call",
                    with_meta(params, meta),
                    parse_call_tool_response,
                )
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
                storage: &scope.storage,
                selected_credential: scope.selected_credential.clone(),
            },
            with_meta(params, meta),
            target,
            |params, upstream_name| params.name = upstream_name.into(),
            |server, upstream_name| {
                if filtering::is_disabled_item(server, "tools", upstream_name) {
                    return Err(ErrorData::invalid_params("tool is disabled", None));
                }
                Ok(())
            },
            parse_call_tool_response,
        )
        .await
    }

    pub(super) async fn read_resource_for_scope(
        &self,
        scope: &RequestScope,
        request_id: &RequestId,
        params: ReadResourceRequestParams,
        meta: RequestMetaObject,
        protocol_version: Option<&ProtocolVersion>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        if scope.server_name.is_some() {
            let response = self
                .dispatch_result(
                    scope,
                    request_id,
                    "resources/read",
                    with_meta(params, meta),
                    parse_read_resource_response,
                )
                .await?;
            return Ok(apply_cache_metadata_to_read_response(
                response,
                protocol_version,
            ));
        }

        let Some(target) = parse_resource_target(&params.uri).map_err(super::internal_error)?
        else {
            return Err(ErrorData::invalid_params(
                "uri must start with mcp://server/",
                None,
            ));
        };
        let response = self
            .forward_aggregate_call(
                AggregateCallContext {
                    user_id: scope.user_id,
                    conversation_id: scope.conversation_id.as_deref(),
                    request_id,
                    method: "resources/read",
                    storage: &scope.storage,
                    selected_credential: scope.selected_credential.clone(),
                },
                with_meta(params, meta),
                target,
                |params, upstream_name| params.uri = upstream_name,
                |server, upstream_name| {
                    if filtering::is_disabled_item(server, "resources", upstream_name) {
                        return Err(ErrorData::invalid_params("resource is disabled", None));
                    }
                    Ok(())
                },
                parse_read_resource_response,
            )
            .await?;
        Ok(apply_cache_metadata_to_read_response(
            response,
            protocol_version,
        ))
    }

    pub(super) async fn get_prompt_for_scope(
        &self,
        scope: &RequestScope,
        request_id: &RequestId,
        params: GetPromptRequestParams,
        meta: RequestMetaObject,
    ) -> Result<GetPromptResponse, ErrorData> {
        if scope.server_name.is_some() {
            return self
                .dispatch_result(
                    scope,
                    request_id,
                    "prompts/get",
                    with_meta(params, meta),
                    parse_get_prompt_response,
                )
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
                storage: &scope.storage,
                selected_credential: scope.selected_credential.clone(),
            },
            with_meta(params, meta),
            target,
            |params, upstream_name| params.name = upstream_name,
            |_, _| Ok(()),
            parse_get_prompt_response,
        )
        .await
    }

    pub(super) async fn complete_for_scope(
        &self,
        scope: &RequestScope,
        request_id: &RequestId,
        params: CompleteRequestParams,
        meta: RequestMetaObject,
    ) -> Result<CompleteResult, ErrorData> {
        if scope.server_name.is_some() {
            return self
                .dispatch_result(
                    scope,
                    request_id,
                    "completion/complete",
                    with_meta(params, meta),
                    parse_result,
                )
                .await;
        }

        let context = AggregateCallContext {
            user_id: scope.user_id,
            conversation_id: scope.conversation_id.as_deref(),
            request_id,
            method: "completion/complete",
            storage: &scope.storage,
            selected_credential: scope.selected_credential.clone(),
        };
        match params.r#ref {
            Reference::Prompt(ref prompt) => {
                let Some(target) = parse_prefixed_name(&prompt.name) else {
                    return Err(ErrorData::invalid_params(
                        "ref/prompt name must be server__name",
                        None,
                    ));
                };
                self.forward_aggregate_call(
                    context,
                    with_meta(params, meta),
                    target,
                    |params, upstream_name| params.r#ref = Reference::for_prompt(upstream_name),
                    |_, _| Ok(()),
                    parse_result,
                )
                .await
            }
            Reference::Resource(ref resource) => {
                let Some(target) =
                    parse_resource_template_target(&resource.uri).map_err(super::internal_error)?
                else {
                    return Err(ErrorData::invalid_params(
                        "ref/resource uri must be a namespaced mcp:// template",
                        None,
                    ));
                };
                self.forward_aggregate_call(
                    context,
                    with_meta(params, meta),
                    target,
                    |params, upstream_name| {
                        params.r#ref = Reference::for_resource(upstream_name);
                    },
                    |_, _| Ok(()),
                    parse_result,
                )
                .await
            }
            _ => Err(ErrorData::invalid_params(
                "unsupported completion ref type",
                None,
            )),
        }
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
        let snapshot = scope
            .cache
            .get(&server)
            .await
            .ok_or_else(|| ErrorData::internal_error("mcp catalog is not ready", None))?;
        let items = match field {
            "tools" => snapshot
                .tools
                .into_iter()
                .filter(|item| {
                    item.get("name")
                        .and_then(Value::as_str)
                        .is_none_or(|name| filtering::is_tool_allowed(&server, name))
                })
                .collect(),
            "resources" => snapshot
                .resources
                .into_iter()
                .filter(|item| {
                    item.get("uri")
                        .and_then(Value::as_str)
                        .is_none_or(|uri| !filtering::is_disabled_item(&server, "resources", uri))
                })
                .collect(),
            "resourceTemplates" => snapshot
                .resource_templates
                .into_iter()
                .filter(|item| {
                    item.get("uriTemplate")
                        .and_then(Value::as_str)
                        .is_none_or(|uri| !filtering::is_disabled_item(&server, "resources", uri))
                })
                .collect(),
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

    async fn dispatch_result<T, P, Parse>(
        &self,
        scope: &RequestScope,
        request_id: &RequestId,
        method: &str,
        params: P,
        parse: Parse,
    ) -> Result<T, ErrorData>
    where
        P: serde::Serialize,
        Parse: FnOnce(Value) -> Result<T, ErrorData>,
    {
        let response = self
            .dispatch(
                scope,
                json_request(request_id, method, required_params(params)?),
            )
            .await?;
        parse(response)
    }

    async fn forward_aggregate_call<T, P, Rewrite, Validate, Parse>(
        &self,
        context: AggregateCallContext<'_>,
        mut params: P,
        target: PrefixedTarget,
        rewrite: Rewrite,
        validate: Validate,
        parse: Parse,
    ) -> Result<T, ErrorData>
    where
        T: 'static,
        P: serde::Serialize,
        Rewrite: FnOnce(&mut P, String),
        Validate: FnOnce(&McpServer, &str) -> Result<(), ErrorData>,
        Parse: FnOnce(Value) -> Result<T, ErrorData>,
    {
        let server = self
            .load_server_by_name(context.user_id, &target.server_name, context.storage)
            .await?;
        validate(&server, &target.upstream_name)?;
        rewrite(&mut params, target.upstream_name);
        let response = filtering::call_server_filtered(
            context.storage,
            &server,
            json_request(context.request_id, context.method, required_params(params)?),
            context.conversation_id,
            context.selected_credential.as_ref(),
        )
        .await
        .map_err(super::internal_error)?;
        parse(response)
    }
}

/// Apply SEP-2549 cache metadata (`ttlMs`/`cacheScope`) to a freshly built
/// paginated result when the downstream peer negotiated `2026-07-28` (or a
/// newer ISO-date version). Older peers (e.g. `2025-11-25`) must NOT receive
/// these fields: rmcp 3.2.0 only strips the `resultType` discriminator for
/// legacy peers, leaving the cache fields on the wire, which strict Zod
/// validators reject as unknown properties.
fn apply_cache_metadata<T>(mut result: T, protocol_version: Option<&ProtocolVersion>) -> T
where
    T: CacheMetadataSettable,
{
    if peer_expects_cache_metadata(protocol_version) {
        result.set_ttl_ms_if_absent(0);
        result.set_cache_scope_if_absent(CacheScope::Private);
    }
    result
}

/// Patch a `resources/read` response so a single-service upstream that
/// predates SEP-2549 still satisfies a downstream peer on `2026-07-28`. If
/// the upstream already supplied `ttlMs`/`cacheScope`, they are preserved
/// untouched.
fn apply_cache_metadata_to_read_response(
    response: ReadResourceResponse,
    protocol_version: Option<&ProtocolVersion>,
) -> ReadResourceResponse {
    if !peer_expects_cache_metadata(protocol_version) {
        return response;
    }
    match response {
        ReadResourceResponse::Complete(mut result) => {
            if result.ttl_ms.is_none() {
                result.ttl_ms = Some(0);
            }
            if result.cache_scope.is_none() {
                result.cache_scope = Some(CacheScope::Private);
            }
            ReadResourceResponse::Complete(result)
        }
        // MRTR intermediates are not cached; nothing to patch.
        other => other,
    }
}

/// `2026-07-28` is the first version where the spec schema makes
/// `CacheableResult` mandatory, so ISO `YYYY-MM-DD` lex order matches
/// chronological order and any version `>= 2026-07-28` requires the fields.
fn peer_expects_cache_metadata(protocol_version: Option<&ProtocolVersion>) -> bool {
    protocol_version
        .map(|version| version.as_str() >= ProtocolVersion::V_2026_07_28.as_str())
        .unwrap_or(false)
}

/// Tiny trait shim so `apply_cache_metadata` can serve the four paginated
/// result types (`ListToolsResult`, `ListResourcesResult`,
/// `ListResourceTemplatesResult`, `ListPromptsResult`) without duplicating
/// the version check. Implementations are gated on the existing
/// `with_ttl_ms`/`with_cache_scope` builders; the `_if_absent` methods only
/// fire when the field is `None`, which is exactly what `with_all_items`
/// produces, so the builders are safe to invoke unconditionally.
trait CacheMetadataSettable {
    fn set_ttl_ms_if_absent(&mut self, ttl_ms: u64);
    fn set_cache_scope_if_absent(&mut self, scope: CacheScope);
}

impl CacheMetadataSettable for ListToolsResult {
    fn set_ttl_ms_if_absent(&mut self, ttl_ms: u64) {
        if self.ttl_ms.is_none() {
            self.ttl_ms = Some(ttl_ms);
        }
    }
    fn set_cache_scope_if_absent(&mut self, scope: CacheScope) {
        if self.cache_scope.is_none() {
            self.cache_scope = Some(scope);
        }
    }
}

impl CacheMetadataSettable for ListResourcesResult {
    fn set_ttl_ms_if_absent(&mut self, ttl_ms: u64) {
        if self.ttl_ms.is_none() {
            self.ttl_ms = Some(ttl_ms);
        }
    }
    fn set_cache_scope_if_absent(&mut self, scope: CacheScope) {
        if self.cache_scope.is_none() {
            self.cache_scope = Some(scope);
        }
    }
}

impl CacheMetadataSettable for ListResourceTemplatesResult {
    fn set_ttl_ms_if_absent(&mut self, ttl_ms: u64) {
        if self.ttl_ms.is_none() {
            self.ttl_ms = Some(ttl_ms);
        }
    }
    fn set_cache_scope_if_absent(&mut self, scope: CacheScope) {
        if self.cache_scope.is_none() {
            self.cache_scope = Some(scope);
        }
    }
}

impl CacheMetadataSettable for ListPromptsResult {
    fn set_ttl_ms_if_absent(&mut self, ttl_ms: u64) {
        if self.ttl_ms.is_none() {
            self.ttl_ms = Some(ttl_ms);
        }
    }
    fn set_cache_scope_if_absent(&mut self, scope: CacheScope) {
        if self.cache_scope.is_none() {
            self.cache_scope = Some(scope);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::{ReadResourceResult, ResourceContents};

    #[test]
    fn peer_expects_cache_metadata_accepts_2026_07_28() {
        assert!(peer_expects_cache_metadata(Some(
            &ProtocolVersion::V_2026_07_28
        )));
    }

    #[test]
    fn peer_expects_cache_metadata_rejects_legacy_versions() {
        assert!(!peer_expects_cache_metadata(Some(
            &ProtocolVersion::V_2025_11_25
        )));
        assert!(!peer_expects_cache_metadata(None));
    }

    fn complete_read_response(
        ttl_ms: Option<u64>,
        scope: Option<CacheScope>,
    ) -> ReadResourceResponse {
        let mut result = ReadResourceResult::new(vec![ResourceContents::text("hi", "mcp://x/y")]);
        result.ttl_ms = ttl_ms;
        result.cache_scope = scope;
        ReadResourceResponse::Complete(result)
    }

    #[test]
    fn apply_cache_metadata_to_read_response_supplements_for_2026_07_28() {
        let response = complete_read_response(None, None);
        match apply_cache_metadata_to_read_response(response, Some(&ProtocolVersion::V_2026_07_28))
        {
            ReadResourceResponse::Complete(result) => {
                assert_eq!(result.ttl_ms, Some(0));
                assert_eq!(result.cache_scope, Some(CacheScope::Private));
            }
            other => panic!("expected Complete, got {other:?}"),
        }
    }

    #[test]
    fn apply_cache_metadata_to_read_response_preserves_existing_values() {
        let response = complete_read_response(Some(60), Some(CacheScope::Private));
        match apply_cache_metadata_to_read_response(response, Some(&ProtocolVersion::V_2026_07_28))
        {
            ReadResourceResponse::Complete(result) => {
                assert_eq!(result.ttl_ms, Some(60));
                assert_eq!(result.cache_scope, Some(CacheScope::Private));
            }
            other => panic!("expected Complete, got {other:?}"),
        }
    }

    #[test]
    fn apply_cache_metadata_to_read_response_skips_for_legacy_or_none() {
        let legacy_input = complete_read_response(None, None);
        match apply_cache_metadata_to_read_response(
            legacy_input,
            Some(&ProtocolVersion::V_2025_11_25),
        ) {
            ReadResourceResponse::Complete(result) => {
                assert_eq!(result.ttl_ms, None);
                assert_eq!(result.cache_scope, None);
            }
            other => panic!("expected Complete, got {other:?}"),
        }

        let none_input = complete_read_response(None, None);
        match apply_cache_metadata_to_read_response(none_input, None) {
            ReadResourceResponse::Complete(result) => {
                assert_eq!(result.ttl_ms, None);
                assert_eq!(result.cache_scope, None);
            }
            other => panic!("expected Complete, got {other:?}"),
        }
    }
}
