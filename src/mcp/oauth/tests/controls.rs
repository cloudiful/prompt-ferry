//! Configuration and grant controls for the personal MCP OAuth backend.
//!
//! These tests cover the controls the phase requires but the end-to-end flow
//! does not exercise on its own: a missing deployment setting, a missing
//! secret manager, a per-server proxy, an admin-scope server, a wider granted
//! scope, and the invalidation of a grant after a disconnect.

use std::sync::Arc;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use uuid::Uuid;

use super::mock_as::MockAs;
use crate::db::{ConfigRepository, McpServer, McpServerInput, UserStore};
use crate::keys::hash_client_key;
use crate::mcp::oauth::{McpOAuthError, McpOAuthService, McpOAuthState};
use crate::relay_secrets::RelaySecretManager;
use crate::standalone_config::StandaloneConfigStore;

const USER_ID: i64 = 21;
const OTHER_USER_ID: i64 = 22;

struct Fixture {
    store: Arc<StandaloneConfigStore>,
    path: std::path::PathBuf,
    repository: ConfigRepository,
    manager: RelaySecretManager,
}

impl Fixture {
    async fn new() -> Self {
        let base = std::env::var("PROMPT_FERRY_TEST_TMPDIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir());
        let _ = std::fs::create_dir_all(&base);
        let path = base.join(format!(
            "prompt-ferry-oauth-controls-{}.sqlite",
            Uuid::new_v4()
        ));
        let store = Arc::new(StandaloneConfigStore::open(&path).await.expect("store"));
        for user_id in [USER_ID, OTHER_USER_ID] {
            sqlx::query(
                "INSERT INTO standalone_users \
                 (user_id, login_name, display_name, password_hash, is_admin, enabled) \
                 VALUES (?1, ?2, ?2, 'hash', 0, 1)",
            )
            .bind(user_id)
            .bind(format!("oauth-control-{user_id}"))
            .execute(store.pool())
            .await
            .expect("insert user");
        }
        let manager =
            RelaySecretManager::from_base64(&STANDARD.encode([7_u8; 32])).expect("manager");
        let repository = ConfigRepository::sqlite(store.clone(), manager.clone());
        Self {
            store,
            path,
            repository,
            manager,
        }
    }

    async fn server(&self, input: McpServerInput) -> McpServer {
        let server_id = Uuid::new_v4();
        self.repository
            .create_mcp_server(server_id, input)
            .await
            .expect("create server");
        self.repository
            .get_mcp_server(server_id)
            .await
            .expect("load server")
            .expect("server row")
    }

    fn service(
        &self,
        redirect_base: Option<&str>,
        secrets: Option<RelaySecretManager>,
    ) -> McpOAuthService {
        McpOAuthService::new(
            self.repository.clone(),
            secrets,
            redirect_base.map(str::to_string),
            UserStore::sqlite(self.store.pool().clone()),
        )
        .with_private_network()
    }

    async fn close(self) {
        let pool = self.store.pool().clone();
        drop(self.store);
        pool.close().await;
        let _ = std::fs::remove_file(self.path);
    }
}

fn server_input(scope: &str, owner: Option<i64>, url: Option<&str>) -> McpServerInput {
    McpServerInput {
        scope: scope.to_string(),
        owner_user_id: owner,
        source_endpoint_id: None,
        name: format!("personal-{scope}-{}", Uuid::new_v4()),
        aggregate_naming_mode: "qualified_only".to_string(),
        transport: "http".to_string(),
        provider_kind: None,
        url: url.map(str::to_string),
        command: None,
        args: serde_json::json!([]),
        env_json: serde_json::json!({}),
        bearer_tokens_json: serde_json::json!([]),
        http_headers_json: serde_json::json!({}),
        auth_mode: "oauth".to_string(),
        basic_username: None,
        basic_password: None,
        proxy_url: None,
        tool_filter_mode: "blacklist".to_string(),
        allowed_tools: serde_json::json!([]),
        disabled_tools: serde_json::json!([]),
        disabled_resources: serde_json::json!([]),
        enabled: true,
        timeout_ms: 30_000,
        lifecycle_policy: "auto".to_string(),
        lifecycle_manual_protocol_version: None,
    }
}

#[allow(dead_code)]
mod test_db_url {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/support/test_db_url.rs"
    ));
}

/// A personal OAuth server row that needs no database read for the fail-fast
/// settings checks that reject before any repository access.
fn lazy_server() -> McpServer {
    McpServer {
        server_id: Uuid::new_v4(),
        source_endpoint_id: None,
        scope: "user".to_string(),
        owner_user_id: Some(USER_ID),
        name: "lazy-personal".to_string(),
        aggregate_naming_mode: "qualified_only".to_string(),
        transport: "http".to_string(),
        provider_kind: None,
        url: Some("https://mcp.example.test/mcp".to_string()),
        command: None,
        args: serde_json::json!([]),
        env_json: serde_json::json!({}),
        bearer_tokens_json: serde_json::json!([]),
        http_headers_json: serde_json::json!({}),
        auth_mode: "oauth".to_string(),
        basic_username: None,
        basic_password: None,
        proxy_url: None,
        has_proxy_url: false,
        tool_filter_mode: "blacklist".to_string(),
        allowed_tools: serde_json::json!([]),
        disabled_tools: serde_json::json!([]),
        disabled_resources: serde_json::json!([]),
        enabled: true,
        timeout_ms: 30_000,
        lifecycle_policy: "auto".to_string(),
        lifecycle_manual_protocol_version: None,
        lifecycle_learned_mode: None,
        lifecycle_learned_protocol_version: None,
        lifecycle_learned_for_updated_at: None,
        lifecycle_learned_at: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}

fn lazy_service(secrets: Option<RelaySecretManager>, redirect: Option<&str>) -> McpOAuthService {
    let pool = test_db_url::lazy_test_pool();
    McpOAuthService::new(
        ConfigRepository::postgres(&pool),
        secrets,
        redirect.map(str::to_string),
        UserStore::postgres(&pool),
    )
}

fn session() -> String {
    hash_client_key("controls-session")
}

fn state_of(authorization_url: &str) -> String {
    reqwest::Url::parse(authorization_url)
        .expect("authorization url")
        .query_pairs()
        .find(|(name, _)| name == "state")
        .map(|(_, value)| value.into_owned())
        .expect("state parameter")
}

#[tokio::test]
async fn a_missing_redirect_base_disables_only_personal_oauth() {
    let fixture = Fixture::new().await;
    let server = fixture
        .server(server_input(
            "user",
            Some(USER_ID),
            Some("https://mcp.example.test/mcp"),
        ))
        .await;
    let service = fixture.service(None, Some(fixture.manager.clone()));
    let error = service
        .start(&server, USER_ID, &session())
        .await
        .expect_err("a missing redirect base cannot start an authorization");
    assert!(matches!(error, McpOAuthError::Disabled(_)), "{error:?}");
    let status = service.status(&server, USER_ID).await.expect("status");
    assert!(
        status.reason.is_some(),
        "the status must explain why OAuth is unavailable"
    );
    assert!(status.scopes.is_empty());
    fixture.close().await;
}

#[tokio::test]
async fn a_missing_secret_manager_disables_personal_oauth() {
    let service = lazy_service(None, Some("https://app.example.test"));
    let error = service
        .start(&lazy_server(), USER_ID, &session())
        .await
        .expect_err("a missing secret manager cannot start an authorization");
    assert_eq!(error, McpOAuthError::Unavailable);
}

#[tokio::test]
async fn a_postgres_shared_key_reason_disables_personal_oauth_actionably() {
    let fixture = Fixture::new().await;
    let server = fixture
        .server(server_input(
            "user",
            Some(USER_ID),
            Some("https://mcp.example.test/mcp"),
        ))
        .await;
    let service = fixture
        .service(Some("https://app.example.test"), None)
        .with_disabled_reason(
            "PostgreSQL personal MCP OAuth requires an explicitly configured shared \
             WORKER_CONFIG_ENCRYPTION_KEY so every worker can decrypt the same grants",
        );
    let error = service
        .start(&server, USER_ID, &session())
        .await
        .expect_err("a missing explicit PG shared key must disable personal OAuth");
    let McpOAuthError::Disabled(message) = error else {
        panic!("expected a disabled error, got {error:?}");
    };
    assert!(message.contains("PostgreSQL"), "{message}");
    assert!(
        message.contains("WORKER_CONFIG_ENCRYPTION_KEY"),
        "{message}"
    );
    let status = service.status(&server, USER_ID).await.expect("status");
    assert_eq!(status.state, McpOAuthState::Disconnected);
    assert!(
        status
            .reason
            .as_deref()
            .is_some_and(|reason| reason.contains("WORKER_CONFIG_ENCRYPTION_KEY")),
        "the status must carry the actionable reason: {:?}",
        status.reason
    );
    fixture.close().await;
}

#[tokio::test]
async fn a_per_server_proxy_and_an_admin_scope_are_rejected() {
    let fixture = Fixture::new().await;
    let mut proxied = server_input("user", Some(USER_ID), Some("https://mcp.example.test/mcp"));
    proxied.proxy_url = Some("http://proxy.example.test:3128".to_string());
    let server = fixture.server(proxied).await;
    let service = fixture.service(
        Some("https://app.example.test"),
        Some(fixture.manager.clone()),
    );
    let error = service
        .start(&server, USER_ID, &session())
        .await
        .expect_err("a proxied personal OAuth server cannot start");
    assert!(matches!(error, McpOAuthError::Disabled(_)), "{error:?}");

    // An admin-scope row carries no owner at all, so it can never be a
    // personal OAuth server.
    let admin_server = fixture
        .server(server_input(
            "admin",
            None,
            Some("https://mcp.example.test/mcp"),
        ))
        .await;
    let error = service
        .start(&admin_server, USER_ID, &session())
        .await
        .expect_err("an admin-scope server cannot use personal OAuth");
    assert!(matches!(error, McpOAuthError::Forbidden), "{error:?}");

    let foreign = fixture
        .server(server_input(
            "user",
            Some(OTHER_USER_ID),
            Some("https://mcp.example.test/mcp"),
        ))
        .await;
    let error = service
        .start(&foreign, USER_ID, &session())
        .await
        .expect_err("another owner cannot start the authorization");
    assert_eq!(error, McpOAuthError::Forbidden);
    fixture.close().await;
}

#[tokio::test]
async fn a_wider_granted_scope_is_rejected_and_disconnect_invalidates_the_grant() {
    let mock = MockAs::start().await;
    let fixture = Fixture::new().await;
    let server = fixture
        .server(server_input("user", Some(USER_ID), Some(&mock.mcp_url())))
        .await;
    let service = fixture.service(
        Some("https://app.example.test"),
        Some(fixture.manager.clone()),
    );

    mock.script().granted_scope = Some("read-mcp write-mcp".to_string());
    let url = service
        .start(&server, USER_ID, &session())
        .await
        .expect("start");
    let state = state_of(&url);
    let error = service
        .callback(
            &server,
            USER_ID,
            &session(),
            "good-code",
            &state,
            Some(mock.base()),
        )
        .await
        .expect_err("a write scope must never be accepted");
    assert!(matches!(error, McpOAuthError::Disabled(_)), "{error:?}");
    let status = service.status(&server, USER_ID).await.expect("status");
    assert_eq!(status.state, crate::mcp::oauth::McpOAuthState::Disconnected);

    // A read-only grant is accepted and a disconnect invalidates it: the next
    // token request never falls back to a cached token.
    mock.script().granted_scope = None;
    let url = service
        .start(&server, USER_ID, &session())
        .await
        .expect("start");
    let state = state_of(&url);
    service
        .callback(
            &server,
            USER_ID,
            &session(),
            "good-code",
            &state,
            Some(mock.base()),
        )
        .await
        .expect("callback");
    assert!(
        service
            .access_token(&server, USER_ID)
            .await
            .expect("access token")
            .starts_with("access-token-"),
        "the grant yields the token the authorization server issued"
    );
    service
        .disconnect(&server, USER_ID)
        .await
        .expect("disconnect");
    assert!(
        service.access_token(&server, USER_ID).await.is_err(),
        "a disconnected grant never yields a token"
    );
    fixture.close().await;
}
