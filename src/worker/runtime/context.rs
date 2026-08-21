use std::time::Instant;

pub(super) use super::bridge::BridgeSender;

use crate::config::WorkerConfig;
use chrono::Utc;
use reqwest::{Client, StatusCode};

#[derive(Debug, Clone, Copy)]
pub(super) struct ResponseLimits {
    pub(super) max_upstream_response_bytes: usize,
    pub(super) max_raw_response_capture_bytes: usize,
    pub(super) max_response_text_capture_bytes: usize,
}

impl Default for ResponseLimits {
    fn default() -> Self {
        Self {
            max_upstream_response_bytes: 64 * 1024 * 1024,
            max_raw_response_capture_bytes: 4 * 1024 * 1024,
            max_response_text_capture_bytes: 1024 * 1024,
        }
    }
}

impl From<&WorkerConfig> for ResponseLimits {
    fn from(config: &WorkerConfig) -> Self {
        Self {
            max_upstream_response_bytes: config.max_upstream_response_bytes.max(1),
            max_raw_response_capture_bytes: config.max_raw_response_capture_bytes.max(1),
            max_response_text_capture_bytes: config.max_response_text_capture_bytes.max(1),
        }
    }
}

use uuid::Uuid;

use crate::{
    db,
    worker::runtime::standalone::StandaloneRuntimeState,
    worker_admin::AdminState,
    worker_admin_types::RequestContentLoggingResponse,
    worker_usage::{
        UsageLog, UsageRecordingMode, UsageRequestMetadata,
        record_usage_event as record_managed_usage, usage_recording_mode,
    },
};

use super::{BufferedBridgeRequest, BufferedMcpRequest, RequestPromptLog, WorkerRuntimeState};
use crate::mcp::targeting::McpRequestMetadata;

pub(super) fn is_bridge_send_error(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<super::bridge::BridgeSendError>()
            .is_some()
    })
}

#[derive(Clone)]
pub(super) struct RuntimeServices {
    pub(super) admin_state: Option<AdminState>,
    pub(super) mcp_state: Option<crate::mcp::McpRuntimeState>,
    pub(super) standalone_state: Option<StandaloneRuntimeState>,
    pub(super) out_tx: BridgeSender,
    pub(super) client: Client,
    pub(super) runtime_state: WorkerRuntimeState,
    pub(super) response_limits: ResponseLimits,
}

impl RuntimeServices {
    pub(super) fn new(
        admin_state: Option<AdminState>,
        out_tx: BridgeSender,
        client: Client,
        runtime_state: WorkerRuntimeState,
        response_limits: ResponseLimits,
    ) -> Self {
        Self {
            admin_state: admin_state.clone(),
            mcp_state: admin_state
                .as_ref()
                .map(crate::mcp::McpRuntimeState::from_admin_state),
            standalone_state: None,
            out_tx,
            client,
            runtime_state,
            response_limits,
        }
    }

    pub(super) fn admin_state(&self) -> Option<&AdminState> {
        self.admin_state.as_ref()
    }

    pub(super) fn mcp_state(&self) -> Option<&crate::mcp::McpRuntimeState> {
        self.mcp_state.as_ref()
    }

    pub(super) fn standalone_state(&self) -> Option<&StandaloneRuntimeState> {
        self.standalone_state.as_ref()
    }

    pub(super) fn with_standalone_state(mut self, state: StandaloneRuntimeState) -> Self {
        if let Some(mcp_state) = state.mcp_runtime() {
            self.mcp_state = Some(mcp_state);
        }
        self.standalone_state = Some(state);
        self
    }

    pub(super) async fn record_usage_event(&self, log: UsageLog) -> Option<i64> {
        match usage_recording_mode(self.admin_state.is_some(), self.standalone_state.is_some()) {
            UsageRecordingMode::SharedManaged => {
                record_managed_usage(self.admin_state(), log).await
            }
            UsageRecordingMode::Standalone => {
                if let Some(state) = self.standalone_state() {
                    state.record_usage(log);
                }
                None
            }
            UsageRecordingMode::Noop => None,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct RequestExecutionContext {
    pub(super) request_id: Uuid,
    pub(super) started: Instant,
    pub(super) request_model: Option<String>,
    pub(super) client_key_id: Option<i64>,
    pub(super) client_key_label: Option<String>,
    pub(super) user_id: Option<i64>,
    pub(super) owner_worker_id: Uuid,
    pub(super) request_prompt_log: RequestPromptLog,
}

impl RequestExecutionContext {
    pub(super) fn new(
        request_id: Uuid,
        started: Instant,
        request_model: Option<String>,
        client_key_id: Option<i64>,
        client_key_label: Option<String>,
        user_id: Option<i64>,
        owner_worker_id: Uuid,
        request_prompt_log: RequestPromptLog,
    ) -> Self {
        Self {
            request_id,
            started,
            request_model,
            client_key_id,
            client_key_label,
            user_id,
            owner_worker_id,
            request_prompt_log,
        }
    }

    pub(super) fn for_mcp(
        request_id: Uuid,
        started: Instant,
        user_id: Option<i64>,
        owner_worker_id: Uuid,
        request_prompt_log: RequestPromptLog,
    ) -> Self {
        Self {
            request_id,
            started,
            request_model: None,
            client_key_id: None,
            client_key_label: None,
            user_id,
            owner_worker_id,
            request_prompt_log,
        }
    }

    pub(super) fn elapsed_ms(&self) -> i64 {
        self.started.elapsed().as_millis() as i64
    }

    pub(super) fn ai_usage_log(
        &self,
        request: &BufferedBridgeRequest,
        fallback_user_id: Option<i64>,
    ) -> UsageLog {
        let mut metadata = self.usage_metadata(
            request.path.clone(),
            request.request_user_agent.clone(),
            fallback_user_id,
        );
        metadata.http_request_content_encoding = request.http_request_content_encoding.clone();
        metadata.http_request_compressed = request.http_request_compressed;
        metadata.http_request_compressed_bytes = request.http_request_compressed_bytes;
        metadata.http_request_decompressed_bytes = request.http_request_decompressed_bytes;
        metadata.http_request_compression_ratio = request.http_request_compression_ratio;
        UsageLog::ai_request(self.request_id, metadata, self.request_model.clone())
    }

    pub(super) fn mcp_usage_log(
        &self,
        request: &BufferedMcpRequest,
        metadata: &McpRequestMetadata,
        request_content_logging: &RequestContentLoggingResponse,
        redact_content: bool,
    ) -> UsageLog {
        let mut usage_metadata = self.usage_metadata(request.path.clone(), None, None);
        usage_metadata.parent_event_id = None;
        usage_metadata.conversation_seq = None;
        usage_metadata.http_request_content_encoding =
            request.http_request_content_encoding.clone();
        usage_metadata.http_request_compressed = request.http_request_compressed;
        usage_metadata.http_request_compressed_bytes = request.http_request_compressed_bytes;
        usage_metadata.http_request_decompressed_bytes = request.http_request_decompressed_bytes;
        usage_metadata.http_request_compression_ratio = request.http_request_compression_ratio;
        let _ = redact_content;
        let request_raw_json = request_content_logging
            .mode
            .captures_raw()
            .then(|| metadata.request_raw_json.clone())
            .flatten();

        UsageLog::mcp_request(
            self.request_id,
            usage_metadata,
            metadata.server_name.clone(),
            metadata.protocol_method.clone(),
            metadata.operation_name.clone(),
        )
        .with_request_raw_json(request_raw_json)
    }

    fn usage_metadata(
        &self,
        path: String,
        request_user_agent: Option<String>,
        fallback_user_id: Option<i64>,
    ) -> UsageRequestMetadata {
        let last_heartbeat_at = Utc::now();
        let lease_expires_at =
            last_heartbeat_at + chrono::Duration::seconds(super::REQUEST_RECORD_LEASE_SECONDS);
        UsageRequestMetadata {
            user_id: self.user_id.or(fallback_user_id).filter(|id| *id > 0),
            client_key_id: self.client_key_id,
            client_key_label: self.client_key_label.clone(),
            request_user_agent,
            path,
            http_request_content_encoding: None,
            http_request_compressed: false,
            http_request_compressed_bytes: None,
            http_request_decompressed_bytes: None,
            http_request_compression_ratio: None,
            conversation_id: self.request_prompt_log.conversation_id,
            parent_event_id: self.request_prompt_log.parent_event_id,
            conversation_seq: self.request_prompt_log.conversation_seq,
            conversation_source: self.request_prompt_log.conversation_source.clone(),
            storage_sanitized: self.request_prompt_log.storage_sanitized,
            storage_sanitized_nul_count: self.request_prompt_log.storage_sanitized_nul_count,
            redaction: self.request_prompt_log.redaction.clone(),
            client_installation_id: self.request_prompt_log.client_installation_id.clone(),
            normalized_item_count: self.request_prompt_log.normalized_item_count,
            normalized_chain_hash: self.request_prompt_log.normalized_chain_hash.clone(),
            normalized_first_ref_hash: self.request_prompt_log.normalized_first_ref_hash.clone(),
            normalized_last_ref_hash: self.request_prompt_log.normalized_last_ref_hash.clone(),
            request_storage_mode: self.request_prompt_log.request_storage_mode.clone(),
            request_full_json: self.request_prompt_log.request_full_json.clone(),
            request_delta_json: self.request_prompt_log.request_delta_json.clone(),
            snapshot_prompt_refs_json: self.request_prompt_log.snapshot_prompt_refs_json.clone(),
            request_raw_json: self.request_prompt_log.request_raw_json.clone(),
            request_has_previous_response_id: self
                .request_prompt_log
                .request_has_previous_response_id,
            request_previous_response_id: self
                .request_prompt_log
                .request_previous_response_id
                .clone(),
            request_previous_response_parent_found: self
                .request_prompt_log
                .request_previous_response_parent_found,
            request_conversation_key: self.request_prompt_log.request_conversation_key.clone(),
            request_conversation_parent_found: self
                .request_prompt_log
                .request_conversation_parent_found,
            base_checkpoint_event_id: self.request_prompt_log.base_checkpoint_event_id,
            upstream_redaction_enabled: self.request_prompt_log.upstream_redaction_enabled,
            upstream_redacted_request_json: self
                .request_prompt_log
                .upstream_redacted_request_json
                .clone(),
            upstream_restore_session: self.request_prompt_log.upstream_restore_session.clone(),
            owner_worker_id: Some(self.owner_worker_id),
            lease_expires_at: Some(lease_expires_at),
            last_heartbeat_at: Some(last_heartbeat_at),
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct RouteExecutionContext {
    pub(super) route: db::RouteConfig,
    pub(super) endpoint_id: Option<Uuid>,
    pub(super) model_route_rule_id: Option<Uuid>,
    pub(super) route_selection_reason: db::RouteSelectionReason,
}

impl RouteExecutionContext {
    pub(super) fn new(route: &db::RouteConfig) -> Self {
        Self {
            route: route.clone(),
            endpoint_id: Some(route.route_id).filter(|id| !id.is_nil()),
            model_route_rule_id: route.model_route_rule_id,
            route_selection_reason: route.route_selection_reason,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        path::PathBuf,
        sync::Arc,
        time::{SystemTime, UNIX_EPOCH},
    };

    use base64::{Engine as _, engine::general_purpose::STANDARD};

    use super::{ResponseLimits, RuntimeServices};
    use crate::{
        relay_secrets::RelaySecretManager,
        standalone_config::{StandaloneConfig, StandaloneConfigStore},
        worker::runtime::{WorkerRuntimeState, standalone::StandaloneRuntimeState},
        worker_usage::{UsageLog, UsageRequestMetadata},
    };

    fn database_path() -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("prompt-ferry-usage-dispatch-{suffix}.sqlite"))
    }

    #[tokio::test]
    async fn dispatches_standalone_usage_and_keeps_no_state_as_noop() {
        let path = database_path();
        let store = Arc::new(StandaloneConfigStore::open(&path).await.expect("store"));
        let standalone = StandaloneRuntimeState::new(
            store,
            RelaySecretManager::from_base64(&STANDARD.encode([7_u8; 32])).expect("manager"),
            StandaloneConfig::default(),
        );
        let services = RuntimeServices::new(
            None,
            super::BridgeSender::test_sender(),
            reqwest::Client::new(),
            WorkerRuntimeState::default(),
            ResponseLimits::default(),
        )
        .with_standalone_state(standalone.clone());
        let log = UsageLog::ai_request(
            uuid::Uuid::new_v4(),
            UsageRequestMetadata {
                path: "/v1/models".to_string(),
                ..UsageRequestMetadata::default()
            },
            None,
        );

        services.record_usage_event(log).await;
        assert_eq!(standalone.recent_usage().len(), 1);

        let no_state = RuntimeServices::new(
            None,
            super::BridgeSender::test_sender(),
            reqwest::Client::new(),
            WorkerRuntimeState::default(),
            ResponseLimits::default(),
        );
        no_state
            .record_usage_event(UsageLog::ai_request(
                uuid::Uuid::new_v4(),
                UsageRequestMetadata {
                    path: "/v1/models".to_string(),
                    ..UsageRequestMetadata::default()
                },
                None,
            ))
            .await;
        assert_eq!(standalone.recent_usage().len(), 1);
        let _ = std::fs::remove_file(path);
    }
}

#[derive(Debug, Clone)]
pub(super) struct FailurePayload {
    pub(super) status: StatusCode,
    pub(super) error_code: String,
    pub(super) error_message: String,
    pub(super) upstream_error_body: Option<String>,
    pub(super) response_body: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub(super) struct AffinityFailureAudit {
    pub(super) endpoint_id: Option<uuid::Uuid>,
    pub(super) endpoint_key_id: Option<uuid::Uuid>,
    pub(super) model_route_rule_id: Option<uuid::Uuid>,
    pub(super) requested_endpoint_id: Option<uuid::Uuid>,
    pub(super) requested_key_id: Option<uuid::Uuid>,
}
