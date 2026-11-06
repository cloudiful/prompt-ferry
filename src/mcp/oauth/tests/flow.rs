//! End-to-end flow tests against an in-process mock authorization server.

use std::sync::Arc;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use sqlx::Row;
use uuid::Uuid;

use super::mock_as::{MockAs, Refresh};
use crate::db::{ConfigRepository, McpServerInput, UserStore};
use crate::keys::hash_client_key;
use crate::mcp::oauth::{McpOAuthError, McpOAuthService, McpOAuthState};
use crate::relay_secrets::RelaySecretManager;
use crate::standalone_config::StandaloneConfigStore;

const USER_ID: i64 = 7;
const OTHER_USER_ID: i64 = 8;

struct Harness {
    service: McpOAuthService,
    server: crate::db::McpServer,
    repository: ConfigRepository,
    manager: RelaySecretManager,
    store: Arc<StandaloneConfigStore>,
    path: std::path::PathBuf,
    server_id: Uuid,
}

impl Harness {
    fn session(&self) -> String {
        hash_client_key("session-one")
    }

    /// A second, independent service over the same store: what another worker
    /// (or a restarted process) sees.
    fn second_service(&self) -> McpOAuthService {
        McpOAuthService::new(
            self.repository.clone(),
            Some(self.manager.clone()),
            Some("https://app.example.test".to_string()),
            UserStore::sqlite(self.store.pool().clone()),
        )
        .with_private_network()
    }

    /// A service whose OAuth HTTP client enforces a short timeout cap.
    fn service_with_timeout(&self, timeout: std::time::Duration) -> McpOAuthService {
        McpOAuthService::new(
            self.repository.clone(),
            Some(self.manager.clone()),
            Some("https://app.example.test".to_string()),
            UserStore::sqlite(self.store.pool().clone()),
        )
        .with_short_timeout(timeout)
    }

    async fn close(self) {
        let pool = self.store.pool().clone();
        drop(self.store);
        pool.close().await;
        let _ = std::fs::remove_file(self.path);
    }
}

async fn setup(mock: &MockAs) -> Harness {
    let base = std::env::var("PROMPT_FERRY_TEST_TMPDIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    let _ = std::fs::create_dir_all(&base);
    let path = base.join(format!("prompt-ferry-oauth-flow-{}.sqlite", Uuid::new_v4()));
    let store = Arc::new(StandaloneConfigStore::open(&path).await.expect("store"));
    let manager = RelaySecretManager::from_base64(&STANDARD.encode([9_u8; 32])).expect("manager");
    let repository = ConfigRepository::sqlite(store.clone(), manager.clone());
    for user_id in [USER_ID, OTHER_USER_ID] {
        sqlx::query(
            "INSERT INTO standalone_users \
             (user_id, login_name, display_name, password_hash, is_admin, enabled) \
             VALUES (?1, ?2, ?2, 'hash', 0, 1)",
        )
        .bind(user_id)
        .bind(format!("oauth-user-{user_id}"))
        .execute(store.pool())
        .await
        .expect("insert user");
    }
    let server_id = Uuid::new_v4();
    repository
        .create_mcp_server(server_id, oauth_server_input(mock, USER_ID))
        .await
        .expect("create oauth server");
    let server = repository
        .get_mcp_server(server_id)
        .await
        .expect("load server")
        .expect("server row");
    let service = McpOAuthService::new(
        repository.clone(),
        Some(manager.clone()),
        Some("https://app.example.test".to_string()),
        UserStore::sqlite(store.pool().clone()),
    )
    .with_private_network();
    Harness {
        service,
        server,
        repository,
        manager,
        store,
        path,
        server_id,
    }
}

fn oauth_server_input(mock: &MockAs, owner: i64) -> McpServerInput {
    McpServerInput {
        scope: "user".to_string(),
        owner_user_id: Some(owner),
        source_endpoint_id: None,
        name: "personal".to_string(),
        aggregate_naming_mode: "qualified_only".to_string(),
        transport: "http".to_string(),
        provider_kind: None,
        url: Some(mock.mcp_url()),
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

fn authorization_state(authorization_url: &str) -> String {
    let url = reqwest::Url::parse(authorization_url).expect("authorization url");
    url.query_pairs()
        .find(|(name, _)| name == "state")
        .map(|(_, value)| value.into_owned())
        .expect("state parameter")
}

#[tokio::test]
async fn start_and_callback_install_an_owner_bound_grant() {
    let mock = MockAs::start().await;
    let harness = setup(&mock).await;

    let authorization_url = harness
        .service
        .start(&harness.server, USER_ID, &harness.session())
        .await
        .expect("start");
    assert!(authorization_url.contains("code_challenge="));
    // The RFC 8707 resource indicator keeps the protected resource, including
    // its `?login` query.
    assert!(authorization_url.contains("login"));
    assert!(!authorization_url.contains("confidential-secret"));

    {
        let script = mock.script();
        assert_eq!(
            script.register_application_type.as_deref(),
            Some("web"),
            "registration must declare a web client"
        );
        assert_eq!(
            script.register_token_endpoint_auth_method.as_deref(),
            Some("client_secret_basic"),
            "registration must declare the confidential method the manager uses"
        );
    }

    let state = authorization_state(&authorization_url);
    harness
        .service
        .callback(
            &harness.server,
            USER_ID,
            &harness.session(),
            "good-code",
            &state,
            Some(mock.base()),
        )
        .await
        .expect("callback");

    let status = harness
        .service
        .status(&harness.server, USER_ID)
        .await
        .expect("status");
    assert_eq!(status.state, McpOAuthState::Connected);
    assert_eq!(status.scopes, vec!["read-mcp".to_string()]);
    assert!(status.expires_at.is_some());
    assert!(status.reason.is_none());

    let token = harness
        .service
        .access_token(&harness.server, USER_ID)
        .await
        .expect("access token");
    assert_eq!(token, "access-token-1");
    {
        let script = mock.script();
        let request = script.token_requests.last().expect("token request");
        assert!(request.contains("code_verifier="));
        assert!(request.contains("login"));
    }
    // No secret-bearing field is ever rendered by the public status.
    let rendered = format!("{:?}", status);
    assert!(!rendered.contains("confidential-secret"));
    assert!(!rendered.contains("access-token"));

    harness.close().await;
}

#[tokio::test]
async fn refresh_rotates_the_token_through_the_lease() {
    let mock = MockAs::start().await;
    let harness = setup(&mock).await;
    connect(&harness, &mock).await;
    mock.push_refresh(Refresh::Rotate);

    let status = harness
        .service
        .refresh(&harness.server, USER_ID)
        .await
        .expect("refresh");
    assert_eq!(status.scopes, vec!["read-mcp".to_string()]);
    assert!(status.generation > 1);
    let token = harness
        .service
        .access_token(&harness.server, USER_ID)
        .await
        .expect("access token");
    assert_eq!(token, "refreshed-token-1");
    harness.close().await;
}

#[tokio::test]
async fn refresh_ignores_unrequested_offline_access_when_response_omits_scope() {
    let mock = MockAs::start().await;
    let harness = setup(&mock).await;
    {
        let mut script = mock.script();
        script.advertise_offline_access = true;
        script.omit_refresh_scope = true;
    }
    connect(&harness, &mock).await;
    mock.push_refresh(Refresh::Rotate);
    let result = harness.service.refresh(&harness.server, USER_ID).await;
    let refresh_request = mock
        .script()
        .token_requests
        .last()
        .cloned()
        .expect("refresh request");
    let request_url = reqwest::Url::parse(&format!("https://example.test/?{refresh_request}"))
        .expect("parse refresh form");
    let scopes: Vec<String> = request_url
        .query_pairs()
        .filter(|(name, _)| name == "scope")
        .map(|(_, value)| value.into_owned())
        .collect();
    harness.close().await;
    assert_eq!(
        scopes,
        vec!["read-mcp"],
        "refresh must request only the originally authorized read-mcp scope"
    );
    let status = result.expect("omitted provider scope preserves the original narrow grant");
    assert_eq!(status.scopes, vec!["read-mcp".to_string()]);
}

#[tokio::test]
async fn a_widened_refresh_scope_is_rejected_and_the_old_grant_survives() {
    let mock = MockAs::start().await;
    let harness = setup(&mock).await;
    connect(&harness, &mock).await;
    mock.push_refresh(Refresh::Rotate);
    mock.script().refresh_granted_scope = Some("read-mcp write-mcp".to_string());

    let error = harness
        .service
        .refresh(&harness.server, USER_ID)
        .await
        .expect_err("a widened refresh grant is rejected");
    assert_eq!(
        error,
        McpOAuthError::Disabled(
            "the authorization server granted scopes beyond the requested read-only scope"
                .to_string()
        )
    );

    let stored = harness
        .repository
        .load_mcp_oauth_credential(&harness.manager, harness.server_id, USER_ID)
        .await
        .expect("load credential")
        .expect("the old grant remains stored");
    assert_eq!(stored.credentials.scopes, vec!["read-mcp".to_string()]);
    assert_eq!(
        stored.credentials.engine["token_response"]["access_token"].as_str(),
        Some("access-token-1"),
        "the widened response must not overwrite the old token"
    );

    let token = harness
        .service
        .access_token(&harness.server, USER_ID)
        .await
        .expect("the narrow grant still yields its token");
    assert_eq!(token, "access-token-1");
    harness.close().await;
}

#[tokio::test]
async fn start_rejects_malicious_authorization_endpoints_before_registration() {
    let mock = MockAs::start().await;
    let harness = setup(&mock).await;
    let authority = mock
        .base()
        .strip_prefix("http://")
        .expect("mock base is http");
    let endpoints = [
        format!("http://user:secret@{authority}/authorize"),
        format!("{}/authorize#fragment", mock.base()),
        format!("ftp://{authority}/authorize"),
    ];

    for endpoint in endpoints {
        mock.script().authorization_endpoint_override = Some(endpoint.clone());
        let error = harness
            .service
            .start(&harness.server, USER_ID, &harness.session())
            .await
            .expect_err("a malicious authorization endpoint must be rejected");
        assert!(
            matches!(error, McpOAuthError::Disabled(_)),
            "{endpoint} must be disabled, got {error:?}"
        );
    }

    // The strict browser policy still rejects an insecure HTTP endpoint even
    // though the mock discovery itself is loopback.
    mock.script().authorization_endpoint_override = Some(format!("{}/authorize", mock.base()));
    let strict = harness.service.clone().with_strict_browser_policy();
    let error = strict
        .start(&harness.server, USER_ID, &harness.session())
        .await
        .expect_err("HTTP authorization metadata is rejected by the strict policy");
    assert!(matches!(error, McpOAuthError::Disabled(_)), "{error:?}");

    {
        let script = mock.script();
        assert!(
            script.registration_bodies.is_empty(),
            "no dynamic registration may happen for a rejected endpoint"
        );
        assert!(
            script.token_requests.is_empty(),
            "no token request may happen for a rejected endpoint"
        );
    }
    harness.close().await;
}

#[tokio::test]
async fn a_short_whole_operation_budget_times_out_without_saving_a_refresh() {
    let mock = MockAs::start().await;
    let harness = setup(&mock).await;
    connect(&harness, &mock).await;
    mock.push_refresh(Refresh::Rotate);
    mock.script().token_delay = Some(std::time::Duration::from_millis(500));
    let service = harness
        .service
        .clone()
        .with_operation_timeout(std::time::Duration::from_millis(100));

    let started = std::time::Instant::now();
    let error = service
        .refresh(&harness.server, USER_ID)
        .await
        .expect_err("the whole refresh operation is bounded");
    assert!(
        matches!(error, McpOAuthError::Upstream(_)),
        "expected a timeout error, got {error:?}"
    );
    assert!(
        started.elapsed() < std::time::Duration::from_secs(2),
        "the short test budget must not wait for the 30s ceiling"
    );
    let stored = harness
        .repository
        .load_mcp_oauth_credential(&harness.manager, harness.server_id, USER_ID)
        .await
        .expect("load credential")
        .expect("the old grant remains stored");
    assert_eq!(
        stored.credentials.engine["token_response"]["access_token"].as_str(),
        Some("access-token-1"),
        "a timed-out refresh must not save a late credential"
    );
    harness.close().await;
}

#[tokio::test]
async fn invalid_grant_requires_a_new_sign_in() {
    let mock = MockAs::start().await;
    let harness = setup(&mock).await;
    connect(&harness, &mock).await;
    mock.push_refresh(Refresh::InvalidGrant);

    let error = harness
        .service
        .refresh(&harness.server, USER_ID)
        .await
        .expect_err("invalid grant is not a success");
    assert_eq!(error, McpOAuthError::AuthorizationRequired);
    let status = harness
        .service
        .status(&harness.server, USER_ID)
        .await
        .expect("status");
    assert_eq!(status.state, McpOAuthState::ReauthRequired);
    let access = harness.service.access_token(&harness.server, USER_ID).await;
    assert!(access.is_err(), "a rejected grant never yields a token");
    harness.close().await;
}

#[tokio::test]
async fn transient_refresh_failure_keeps_the_stored_grant() {
    let mock = MockAs::start().await;
    let harness = setup(&mock).await;
    connect(&harness, &mock).await;
    mock.push_refresh(Refresh::Transient);

    let result = harness.service.refresh(&harness.server, USER_ID).await;
    assert!(matches!(result, Err(McpOAuthError::Upstream(_))));
    let status = harness
        .service
        .status(&harness.server, USER_ID)
        .await
        .expect("status");
    assert_eq!(
        status.state,
        McpOAuthState::Connected,
        "a transient failure must not discard the valid grant"
    );
    harness.close().await;
}

#[tokio::test]
async fn a_redirect_from_the_token_endpoint_is_not_followed() {
    let mock = MockAs::start().await;
    let harness = setup(&mock).await;
    mock.script().token_redirect = true;
    let url = harness
        .service
        .start(&harness.server, USER_ID, &harness.session())
        .await
        .expect("start");
    let state = authorization_state(&url);
    let result = harness
        .service
        .callback(
            &harness.server,
            USER_ID,
            &harness.session(),
            "good-code",
            &state,
            Some(mock.base()),
        )
        .await;
    assert!(
        matches!(result, Err(McpOAuthError::Upstream(_))),
        "a redirect response must not be followed: {result:?}"
    );
    harness.close().await;
}

#[tokio::test]
async fn registration_without_a_client_secret_is_rejected() {
    let mock = MockAs::start().await;
    let harness = setup(&mock).await;
    mock.script().registration_without_secret = true;
    let result = harness
        .service
        .start(&harness.server, USER_ID, &harness.session())
        .await;
    assert!(matches!(result, Err(McpOAuthError::Disabled(_))));
    let pending: i64 = sqlx::query("SELECT COUNT(*) AS count FROM standalone_mcp_oauth_states")
        .fetch_one(harness.store.pool())
        .await
        .expect("count states")
        .get("count");
    assert_eq!(
        pending, 0,
        "a rejected registration stores no pending state"
    );
    harness.close().await;
}

#[tokio::test]
async fn disconnect_clears_the_grant_and_inactive_owners_fail_closed() {
    let mock = MockAs::start().await;
    let harness = setup(&mock).await;
    connect(&harness, &mock).await;
    assert!(
        harness
            .service
            .disconnect(&harness.server, USER_ID)
            .await
            .expect("disconnect")
    );
    let status = harness
        .service
        .status(&harness.server, USER_ID)
        .await
        .expect("status");
    assert_eq!(status.state, McpOAuthState::Disconnected);

    // A deactivated owner fails closed before any network request.
    sqlx::query("UPDATE standalone_users SET enabled = 0 WHERE user_id = ?1")
        .bind(USER_ID)
        .execute(harness.store.pool())
        .await
        .expect("deactivate owner");
    let error = harness
        .service
        .access_token(&harness.server, USER_ID)
        .await
        .expect_err("inactive owner");
    assert_eq!(error, McpOAuthError::Forbidden);

    // Another owner can never use this server's grant.
    let error = harness
        .service
        .refresh(&harness.server, OTHER_USER_ID)
        .await
        .expect_err("cross owner");
    assert_eq!(error, McpOAuthError::Forbidden);

    let _ = harness.server_id;
    harness.close().await;
}

async fn connect(harness: &Harness, mock: &MockAs) {
    let url = harness
        .service
        .start(&harness.server, USER_ID, &harness.session())
        .await
        .expect("start");
    let state = authorization_state(&url);
    harness
        .service
        .callback(
            &harness.server,
            USER_ID,
            &harness.session(),
            "good-code",
            &state,
            Some(mock.base()),
        )
        .await
        .expect("callback");
}

async fn credential_rows(harness: &Harness) -> i64 {
    sqlx::query("SELECT COUNT(*) AS count FROM standalone_mcp_oauth_credentials")
        .fetch_one(harness.store.pool())
        .await
        .expect("count credentials")
        .get("count")
}

#[tokio::test]
async fn status_is_persisted_and_shared_across_service_instances() {
    let mock = MockAs::start().await;
    let harness = setup(&mock).await;
    let other_worker = harness.second_service();

    let url = harness
        .service
        .start(&harness.server, USER_ID, &harness.session())
        .await
        .expect("start");
    let _ = authorization_state(&url);

    // A different instance (another worker or a restarted process) sees the
    // pending attempt instead of a process-local flag.
    assert_eq!(
        other_worker
            .status(&harness.server, USER_ID)
            .await
            .expect("status")
            .state,
        McpOAuthState::Authorizing
    );

    // The persisted state expires independently of this process.
    sqlx::query("UPDATE standalone_mcp_oauth_states SET expires_at = ?1")
        .bind((chrono::Utc::now() - chrono::Duration::seconds(5)).to_rfc3339())
        .execute(harness.store.pool())
        .await
        .expect("expire state");
    assert_eq!(
        other_worker
            .status(&harness.server, USER_ID)
            .await
            .expect("status")
            .state,
        McpOAuthState::Disconnected
    );

    // A rejected callback from the wrong session leaves the valid attempt
    // usable, so both instances still report `authorizing`.
    let url = harness
        .service
        .start(&harness.server, USER_ID, &harness.session())
        .await
        .expect("start again");
    let state_again = authorization_state(&url);
    let rejected = harness
        .service
        .callback(
            &harness.server,
            USER_ID,
            &hash_client_key("another-browser"),
            "good-code",
            &state_again,
            Some(mock.base()),
        )
        .await;
    assert_eq!(rejected, Err(McpOAuthError::InvalidState));
    assert_eq!(
        other_worker
            .status(&harness.server, USER_ID)
            .await
            .expect("status")
            .state,
        McpOAuthState::Authorizing
    );

    harness
        .service
        .callback(
            &harness.server,
            USER_ID,
            &harness.session(),
            "good-code",
            &state_again,
            Some(mock.base()),
        )
        .await
        .expect("callback");
    for service in [&harness.service, &other_worker] {
        let status = service
            .status(&harness.server, USER_ID)
            .await
            .expect("status");
        assert_eq!(status.state, McpOAuthState::Connected);
        assert_eq!(status.scopes, vec!["read-mcp".to_string()]);
        assert!(status.expires_at.is_some());
    }
    // A successful consumption ends the attempt for every worker.
    let pending: i64 = sqlx::query("SELECT COUNT(*) AS count FROM standalone_mcp_oauth_states")
        .fetch_one(harness.store.pool())
        .await
        .expect("count states")
        .get("count");
    assert_eq!(pending, 0);
    harness.close().await;
}

#[tokio::test]
async fn a_rejected_issuer_preserves_the_attempt_until_a_correct_callback() {
    let mock = MockAs::start().await;
    let harness = setup(&mock).await;
    let url = harness
        .service
        .start(&harness.server, USER_ID, &harness.session())
        .await
        .expect("start");
    let state = authorization_state(&url);

    // The mock advertises `authorization_response_iss_parameter_supported`, so
    // both a mismatched and a missing issuer are rejected before the atomic
    // consume and before any token request.
    for iss in [Some("https://attacker.example.test"), None] {
        let error = harness
            .service
            .callback(
                &harness.server,
                USER_ID,
                &harness.session(),
                "good-code",
                &state,
                iss,
            )
            .await
            .expect_err("an unverifiable issuer cannot complete the grant");
        assert_eq!(error, McpOAuthError::InvalidState, "iss={iss:?}");
    }
    assert!(
        mock.script().token_requests.is_empty(),
        "no token exchange may run for a rejected issuer"
    );
    assert_eq!(credential_rows(&harness).await, 0);
    // The preflight rejected nothing permanently: the attempt is still pending
    // and a correct callback consumes it exactly once.
    assert_eq!(
        harness
            .service
            .status(&harness.server, USER_ID)
            .await
            .expect("status")
            .state,
        McpOAuthState::Authorizing
    );
    harness
        .service
        .callback(
            &harness.server,
            USER_ID,
            &harness.session(),
            "good-code",
            &state,
            Some(mock.base()),
        )
        .await
        .expect("the correct issuer completes the attempt");
    assert_eq!(mock.script().token_requests.len(), 1);
    assert_eq!(credential_rows(&harness).await, 1);
    let replay = harness
        .service
        .callback(
            &harness.server,
            USER_ID,
            &harness.session(),
            "good-code",
            &state,
            Some(mock.base()),
        )
        .await;
    assert_eq!(replay, Err(McpOAuthError::InvalidState));
    assert_eq!(mock.script().token_requests.len(), 1, "no second exchange");
    harness.close().await;
}

#[tokio::test]
async fn a_wrong_server_session_or_owner_preserves_the_attempt() {
    let mock = MockAs::start().await;
    let harness = setup(&mock).await;
    let mut other_input = oauth_server_input(&mock, USER_ID);
    other_input.name = format!("personal-other-{}", Uuid::new_v4());
    let other_server = harness
        .repository
        .create_mcp_server(Uuid::new_v4(), other_input)
        .await
        .expect("second oauth server");
    let url = harness
        .service
        .start(&harness.server, USER_ID, &harness.session())
        .await
        .expect("start");
    let state = authorization_state(&url);

    // Another server whose callback references this state consumes nothing.
    let wrong_server = harness
        .service
        .callback(
            &other_server,
            USER_ID,
            &harness.session(),
            "good-code",
            &state,
            Some(mock.base()),
        )
        .await;
    assert_eq!(wrong_server, Err(McpOAuthError::InvalidState));
    // A different browser session and a different user consume nothing either.
    let wrong_session = harness
        .service
        .callback(
            &harness.server,
            USER_ID,
            &hash_client_key("another-browser"),
            "good-code",
            &state,
            Some(mock.base()),
        )
        .await;
    assert_eq!(wrong_session, Err(McpOAuthError::InvalidState));
    let wrong_owner = harness
        .service
        .callback(
            &harness.server,
            OTHER_USER_ID,
            &harness.session(),
            "good-code",
            &state,
            Some(mock.base()),
        )
        .await;
    assert_eq!(wrong_owner, Err(McpOAuthError::Forbidden));

    assert!(
        mock.script().token_requests.is_empty(),
        "no rejection before consumption may reach the token endpoint"
    );
    assert_eq!(credential_rows(&harness).await, 0);
    assert_eq!(
        harness
            .service
            .status(&harness.server, USER_ID)
            .await
            .expect("status")
            .state,
        McpOAuthState::Authorizing
    );
    harness
        .service
        .callback(
            &harness.server,
            USER_ID,
            &harness.session(),
            "good-code",
            &state,
            Some(mock.base()),
        )
        .await
        .expect("the legitimate callback still completes");
    assert_eq!(mock.script().token_requests.len(), 1);
    harness.close().await;
}

#[tokio::test]
async fn concurrent_valid_callbacks_permit_at_most_one_exchange() {
    let mock = MockAs::start().await;
    let harness = setup(&mock).await;
    let url = harness
        .service
        .start(&harness.server, USER_ID, &harness.session())
        .await
        .expect("start");
    let state = authorization_state(&url);

    let session = harness.session();
    let issuer = mock.base().to_string();
    let first = harness.service.callback(
        &harness.server,
        USER_ID,
        &session,
        "good-code",
        &state,
        Some(&issuer),
    );
    let second = harness.service.callback(
        &harness.server,
        USER_ID,
        &session,
        "good-code",
        &state,
        Some(&issuer),
    );
    let (first, second) = tokio::join!(first, second);
    let successes = [&first, &second]
        .iter()
        .filter(|result| result.is_ok())
        .count();
    assert_eq!(successes, 1, "exactly one callback may exchange the code");
    assert_eq!(
        mock.script().token_requests.len(),
        1,
        "the losing callback must not reach the token endpoint"
    );
    assert_eq!(credential_rows(&harness).await, 1);
    let replay = harness
        .service
        .callback(
            &harness.server,
            USER_ID,
            &session,
            "good-code",
            &state,
            Some(&issuer),
        )
        .await;
    assert_eq!(
        replay,
        Err(McpOAuthError::InvalidState),
        "the consumed state cannot be replayed after the concurrent exchange"
    );
    assert_eq!(
        mock.script().token_requests.len(),
        1,
        "the replayed callback must not reach the token endpoint"
    );
    harness.close().await;
}

#[tokio::test]
async fn the_per_send_guard_survives_a_healthy_refresh_and_rejects_a_disconnect() {
    let mock = MockAs::start().await;
    let harness = setup(&mock).await;
    connect(&harness, &mock).await;
    let (connection_id, fingerprint, url) = harness
        .service
        .send_context(&harness.server, USER_ID)
        .await
        .expect("send context");

    // Force the stored token past its expiry: the guard refreshes inside the
    // same connection epoch and still returns a token for the send.
    sqlx::query("UPDATE standalone_mcp_oauth_credentials SET expires_at = ?1")
        .bind((chrono::Utc::now() - chrono::Duration::seconds(60)).to_rfc3339())
        .execute(harness.store.pool())
        .await
        .expect("expire token");
    mock.push_refresh(Refresh::Rotate);
    let token = harness
        .service
        .send_token(
            harness.server.server_id,
            USER_ID,
            connection_id,
            &fingerprint,
            &url,
        )
        .await
        .expect("a healthy refresh keeps the send authorized");
    assert_eq!(token, "refreshed-token-1");

    // A disconnect ends it for every later send.
    harness
        .service
        .disconnect(&harness.server, USER_ID)
        .await
        .expect("disconnect");
    let error = harness
        .service
        .send_token(
            harness.server.server_id,
            USER_ID,
            connection_id,
            &fingerprint,
            &url,
        )
        .await
        .expect_err("a disconnected grant cannot authorize a send");
    assert_eq!(error, McpOAuthError::AuthorizationRequired);
    harness.close().await;
}

#[tokio::test]
async fn an_oversized_token_response_is_rejected() {
    let mock = MockAs::start().await;
    let harness = setup(&mock).await;
    mock.script().oversize_token_body = true;
    let url = harness
        .service
        .start(&harness.server, USER_ID, &harness.session())
        .await
        .expect("start");
    let state = authorization_state(&url);
    let error = harness
        .service
        .callback(
            &harness.server,
            USER_ID,
            &harness.session(),
            "good-code",
            &state,
            Some(mock.base()),
        )
        .await
        .expect_err("a body above the 1 MiB cap is refused");
    assert!(matches!(error, McpOAuthError::Upstream(_)), "{error:?}");
    assert_eq!(credential_rows(&harness).await, 0);
    assert_eq!(
        harness
            .service
            .status(&harness.server, USER_ID)
            .await
            .expect("status")
            .state,
        McpOAuthState::Disconnected
    );
    harness.close().await;
}

#[tokio::test]
async fn a_slow_token_endpoint_times_out_without_storing_a_grant() {
    let mock = MockAs::start().await;
    let harness = setup(&mock).await;
    mock.script().token_delay = Some(std::time::Duration::from_millis(400));
    let short = harness.service_with_timeout(std::time::Duration::from_millis(150));
    let url = short
        .start(&harness.server, USER_ID, &harness.session())
        .await
        .expect("start");
    let state = authorization_state(&url);
    let error = short
        .callback(
            &harness.server,
            USER_ID,
            &harness.session(),
            "good-code",
            &state,
            Some(mock.base()),
        )
        .await
        .expect_err("a token endpoint past the cap is refused");
    assert!(matches!(error, McpOAuthError::Upstream(_)), "{error:?}");
    assert_eq!(credential_rows(&harness).await, 0);
    harness.close().await;
}

#[tokio::test]
async fn cross_owner_attempts_make_no_provider_request() {
    let mock = MockAs::start().await;
    let harness = setup(&mock).await;
    connect(&harness, &mock).await;
    let tokens_before = mock.script().token_requests.len();
    let registrations_before = mock.script().registration_bodies.len();

    // Every non-owner entry point fails before the network boundary.
    for result in [
        harness
            .service
            .start(&harness.server, OTHER_USER_ID, &harness.session())
            .await
            .map(|_| ()),
        harness
            .service
            .refresh(&harness.server, OTHER_USER_ID)
            .await
            .map(|_| ()),
        harness
            .service
            .access_token(&harness.server, OTHER_USER_ID)
            .await
            .map(|_| ()),
        harness
            .service
            .disconnect(&harness.server, OTHER_USER_ID)
            .await
            .map(|_| ()),
    ] {
        assert_eq!(result, Err(McpOAuthError::Forbidden));
    }
    let (token_requests, registrations) = {
        let script = mock.script();
        (
            script.token_requests.len(),
            script.registration_bodies.len(),
        )
    };
    assert_eq!(
        token_requests, tokens_before,
        "another owner must not trigger a token request"
    );
    assert_eq!(
        registrations, registrations_before,
        "another owner must not trigger a registration"
    );
    harness.close().await;
}
