// Issue #384 Phase 2: redaction runtime moved from `prompt_ferry::redact`.
// Leaf crate with zero intra-workspace dependencies.
pub mod test_support;

use std::{
    collections::{HashMap, HashSet},
    sync::{LazyLock, RwLock},
};

use chrono::{DateTime, Utc};
use redactor::{
    AppliedReplacement, CustomStringMatch, CustomStringRule, CustomStringScope, Finding, InputKind,
    RedactionPolicy, RedactionResult, RedactionRules, RedactionStats, Redactor, RedactorBuilder,
    RedactorError,
};
use serde::{Deserialize, Serialize};

/// Persisted per-rule shape for [`RedactionConfig::custom_strings`].
///
/// `pattern` / `match_type` / `scope` are the fields the redactor runs on;
/// `created_at` / `updated_at` are per-rule bookkeeping for the admin console
/// (this blob is the only storage for custom string rules). They never reach
/// [`RedactionPolicy`]: call [`RedactionCustomStringRule::runtime`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedactionCustomStringRule {
    pub pattern: String,
    #[serde(default)]
    pub match_type: CustomStringMatch,
    #[serde(default)]
    pub scope: CustomStringScope,
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
}

impl RedactionCustomStringRule {
    pub fn runtime(&self) -> CustomStringRule {
        CustomStringRule {
            pattern: self.pattern.clone(),
            match_type: self.match_type,
            scope: self.scope,
        }
    }

    /// Runtime-relevant fields are equal; timestamps are ignored.
    pub fn same_content(&self, other: &Self) -> bool {
        self.pattern == other.pattern
            && self.match_type == other.match_type
            && self.scope == other.scope
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RedactionConfig {
    pub enabled: bool,
    #[serde(default)]
    pub rules: RedactionRules,
    #[serde(default)]
    pub custom_strings: Vec<RedactionCustomStringRule>,
}

impl RedactionConfig {
    pub fn normalized(&self) -> RedactionConfig {
        RedactionConfig {
            enabled: self.enabled,
            rules: self.rules,
            custom_strings: normalize_custom_strings(&self.custom_strings),
        }
    }

    pub fn policy(&self) -> RedactionPolicy {
        RedactionPolicy {
            rules: self.rules,
            custom_strings: self
                .custom_strings
                .iter()
                .map(RedactionCustomStringRule::runtime)
                .collect(),
            custom_files: Vec::new(),
        }
    }

    pub fn effective_with(&self, user_config: &RedactionConfig) -> RedactionConfig {
        self.normalized()
            .merge_normalized(&user_config.normalized())
    }

    fn merge_normalized(&self, user_config: &RedactionConfig) -> RedactionConfig {
        RedactionConfig {
            enabled: self.enabled || user_config.enabled,
            rules: self.rules.merged_with(user_config.rules),
            custom_strings: merge_normalized_custom_strings(
                &self.custom_strings,
                &user_config.custom_strings,
            ),
        }
    }

    pub fn validate(&self) -> Result<(), RedactorError> {
        for rule in &self.custom_strings {
            if rule.pattern.contains("[[RDX:v2:") {
                return Err(RedactorError::Validation(
                    "pattern must not contain redaction token [[RDX:v2:".to_string(),
                ));
            }
        }
        self.policy().validate().map_err(RedactorError::Validation)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedactionPreviewRequest {
    pub text: String,
    #[serde(default)]
    pub input_kind: InputKind,
    pub enabled: bool,
    #[serde(default)]
    pub rules: RedactionRules,
    #[serde(default)]
    pub custom_strings: Vec<RedactionCustomStringRule>,
}

impl RedactionPreviewRequest {
    pub fn normalized(&self) -> RedactionPreviewRequest {
        RedactionPreviewRequest {
            text: self.text.clone(),
            input_kind: self.input_kind,
            enabled: self.enabled,
            rules: self.rules,
            custom_strings: normalize_custom_strings(&self.custom_strings),
        }
    }

    pub fn config(&self) -> RedactionConfig {
        RedactionConfig {
            enabled: self.enabled,
            rules: self.rules,
            custom_strings: self.custom_strings.clone(),
        }
        .normalized()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RedactionPreviewResponse {
    pub redacted_text: String,
    pub findings: Vec<Finding>,
    pub applied_replacements: Vec<AppliedReplacement>,
    pub stats: RedactionStats,
}

#[derive(Debug, Clone, Serialize)]
pub struct RedactionUsageSummary {
    pub applied: bool,
    pub findings_count: i32,
    pub replacements_count: i32,
    pub types: Vec<String>,
    pub fields: Vec<String>,
}

#[derive(Debug, Clone)]
struct RedactionRuntime {
    enabled: bool,
    redactor: Redactor,
}

impl Default for RedactionRuntime {
    fn default() -> Self {
        Self::from_config(&RedactionConfig::default()).expect("default redaction config is valid")
    }
}

impl RedactionRuntime {
    fn from_config(config: &RedactionConfig) -> Result<Self, RedactorError> {
        let redactor = RedactorBuilder::new()
            .with_redaction_policy(config.policy())
            .try_build()?;
        Ok(Self {
            enabled: config.enabled,
            redactor,
        })
    }
}

#[derive(Debug, Clone)]
struct RedactionRuntimeStore {
    global_config: RedactionConfig,
    global_runtime: RedactionRuntime,
    user_configs: HashMap<i64, RedactionConfig>,
    user_runtimes: HashMap<i64, RedactionRuntime>,
    /// Monotonic policy generation, bumped only when a config mutation changes
    /// runtime-relevant content, so upstream redaction sessions created under an
    /// older policy are rebuilt instead of reusing their token counter
    /// (Issue #524 Task 5) while an unchanged re-publish (restart, settings
    /// write) keeps every persisted session valid.
    generation: u64,
}

impl Default for RedactionRuntimeStore {
    fn default() -> Self {
        let global_config = RedactionConfig::default();
        let global_runtime = RedactionRuntime::from_config(&global_config)
            .expect("default redaction config is valid");
        Self {
            global_config,
            global_runtime,
            user_configs: HashMap::new(),
            user_runtimes: HashMap::new(),
            generation: 0,
        }
    }
}

impl RedactionRuntimeStore {
    fn build_user_runtimes(
        global_config: &RedactionConfig,
        user_configs: &HashMap<i64, RedactionConfig>,
    ) -> Result<HashMap<i64, RedactionRuntime>, RedactorError> {
        let mut user_runtimes = HashMap::with_capacity(user_configs.len());
        for (user_id, user_config) in user_configs {
            let effective = global_config.merge_normalized(user_config);
            user_runtimes.insert(*user_id, RedactionRuntime::from_config(&effective)?);
        }
        Ok(user_runtimes)
    }

    fn has_any_enabled(&self) -> bool {
        self.global_config.enabled || self.user_configs.values().any(|config| config.enabled)
    }
}

static REDACTION_RUNTIME: LazyLock<RwLock<RedactionRuntimeStore>> =
    LazyLock::new(|| RwLock::new(RedactionRuntimeStore::default()));

// Unconditional so downstream `test_support` helpers can lock it even when
// this crate is built as a non-test dependency of `prompt-ferry` tests.
// `tokio::sync::Mutex` keeps `clippy::await_holding_lock` quiet when the
// guard is held across `.await` in async tests.
pub static TEST_REDACTION_LOCK: LazyLock<tokio::sync::Mutex<()>> =
    LazyLock::new(|| tokio::sync::Mutex::new(()));

/// Runtime-relevant config equality. Custom string timestamps are admin-console
/// bookkeeping and never reach the redactor, so re-publishing the same rules
/// with fresh timestamps must not look like a policy change.
fn config_content_eq(left: &RedactionConfig, right: &RedactionConfig) -> bool {
    left.enabled == right.enabled
        && left.rules == right.rules
        && left.custom_strings.len() == right.custom_strings.len()
        && left
            .custom_strings
            .iter()
            .zip(&right.custom_strings)
            .all(|(left, right)| left.same_content(right))
}

fn user_configs_content_eq(
    left: &HashMap<i64, RedactionConfig>,
    right: &HashMap<i64, RedactionConfig>,
) -> bool {
    left.len() == right.len()
        && left.iter().all(|(user_id, config)| {
            right
                .get(user_id)
                .is_some_and(|other| config_content_eq(config, other))
        })
}

pub fn apply_config(config: &RedactionConfig) -> Result<(), RedactorError> {
    let normalized = config.normalized();
    let runtime = RedactionRuntime::from_config(&normalized)?;
    let mut store = REDACTION_RUNTIME
        .write()
        .expect("redaction runtime lock poisoned");
    let changed = !config_content_eq(&store.global_config, &normalized);
    let user_runtimes =
        RedactionRuntimeStore::build_user_runtimes(&normalized, &store.user_configs)?;
    store.global_config = normalized;
    store.global_runtime = runtime;
    store.user_runtimes = user_runtimes;
    if changed {
        store.generation = store.generation.wrapping_add(1);
    }
    Ok(())
}

pub fn apply_configs(
    global_config: &RedactionConfig,
    user_configs: HashMap<i64, RedactionConfig>,
) -> Result<(), RedactorError> {
    let normalized_global = global_config.normalized();
    let normalized_users = user_configs
        .into_iter()
        .map(|(user_id, config)| (user_id, config.normalized()))
        .collect::<HashMap<_, _>>();
    let global_runtime = RedactionRuntime::from_config(&normalized_global)?;
    let user_runtimes =
        RedactionRuntimeStore::build_user_runtimes(&normalized_global, &normalized_users)?;
    let mut store = REDACTION_RUNTIME
        .write()
        .expect("redaction runtime lock poisoned");
    let changed = !config_content_eq(&store.global_config, &normalized_global)
        || !user_configs_content_eq(&store.user_configs, &normalized_users);
    let generation = if changed {
        store.generation.wrapping_add(1)
    } else {
        store.generation
    };
    *store = RedactionRuntimeStore {
        global_config: normalized_global,
        global_runtime,
        user_configs: normalized_users,
        user_runtimes,
        generation,
    };
    Ok(())
}

pub fn apply_user_config(user_id: i64, config: &RedactionConfig) -> Result<(), RedactorError> {
    let normalized = config.normalized();
    let mut store = REDACTION_RUNTIME
        .write()
        .expect("redaction runtime lock poisoned");
    let changed = store
        .user_configs
        .get(&user_id)
        .is_none_or(|existing| !config_content_eq(existing, &normalized));
    let effective = store.global_config.merge_normalized(&normalized);
    let runtime = RedactionRuntime::from_config(&effective)?;
    store.user_configs.insert(user_id, normalized);
    store.user_runtimes.insert(user_id, runtime);
    if changed {
        store.generation = store.generation.wrapping_add(1);
    }
    Ok(())
}

fn normalize_custom_strings(
    custom_strings: &[RedactionCustomStringRule],
) -> Vec<RedactionCustomStringRule> {
    let mut seen = HashSet::<(String, CustomStringMatch, CustomStringScope)>::new();
    let mut normalized = Vec::new();
    for rule in custom_strings {
        let mut rule = rule.clone();
        rule.pattern = rule.pattern.trim().to_owned();
        if rule.pattern.is_empty() {
            continue;
        }
        if seen.insert((rule.pattern.clone(), rule.match_type, rule.scope)) {
            normalized.push(rule);
        }
    }
    normalized
}

fn merge_normalized_custom_strings(
    global: &[RedactionCustomStringRule],
    user: &[RedactionCustomStringRule],
) -> Vec<RedactionCustomStringRule> {
    let mut seen = HashSet::new();
    global
        .iter()
        .chain(user)
        .filter(|rule| seen.insert((rule.pattern.clone(), rule.match_type, rule.scope)))
        .cloned()
        .collect()
}

pub fn has_any_enabled() -> bool {
    REDACTION_RUNTIME
        .read()
        .expect("redaction runtime lock poisoned")
        .has_any_enabled()
}

pub fn redactor_snapshot_for_user(user_id: Option<i64>) -> Option<Redactor> {
    let store = REDACTION_RUNTIME
        .read()
        .expect("redaction runtime lock poisoned");
    let runtime = user_id
        .and_then(|value| store.user_runtimes.get(&value))
        .unwrap_or(&store.global_runtime);
    runtime.enabled.then(|| runtime.redactor.clone())
}

pub fn redaction_enabled_for_user(user_id: Option<i64>) -> bool {
    let store = REDACTION_RUNTIME
        .read()
        .expect("redaction runtime lock poisoned");
    user_id
        .and_then(|value| store.user_runtimes.get(&value))
        .unwrap_or(&store.global_runtime)
        .enabled
}

/// Current redaction policy generation. It changes when a config mutation
/// changes runtime-relevant content; callers stamp it onto persisted state so
/// state from an older policy is rebuilt rather than reused (Issue #524 Task 5).
pub fn policy_generation() -> u64 {
    REDACTION_RUNTIME
        .read()
        .expect("redaction runtime lock poisoned")
        .generation
}

pub fn redact_text(text: &str) -> String {
    redact_text_for_user(text, None)
}

pub fn redact_text_for_user(text: &str, user_id: Option<i64>) -> String {
    let Some(redactor) = redactor_snapshot_for_user(user_id) else {
        return text.to_string();
    };
    redactor
        .redact_with_input_kind(text, InputKind::Text)
        .map(|result| result.redacted_text)
        .unwrap_or_else(|_| text.to_string())
}

pub fn preview(
    request: &RedactionPreviewRequest,
) -> Result<RedactionPreviewResponse, RedactorError> {
    let config = request.config();
    if !config.enabled {
        config.validate()?;
        return Ok(RedactionPreviewResponse {
            redacted_text: request.text.clone(),
            findings: Vec::new(),
            applied_replacements: Vec::new(),
            stats: RedactionStats::default(),
        });
    }
    let result = RedactorBuilder::new()
        .with_redaction_policy(config.policy())
        .try_build()?
        .redact_with_input_kind(&request.text, request.input_kind)?;
    Ok(RedactionPreviewResponse::from(result))
}

pub fn summarize_result(
    result: &RedactionPreviewResponse,
    fields: &[&str],
) -> RedactionUsageSummary {
    let mut types = result
        .findings
        .iter()
        .map(|finding| format!("{:?}", finding.kind).to_lowercase())
        .collect::<Vec<_>>();
    types.sort();
    types.dedup();

    let mut field_values = fields
        .iter()
        .map(|field| (*field).to_string())
        .collect::<Vec<_>>();
    field_values.sort();
    field_values.dedup();

    RedactionUsageSummary {
        applied: !result.findings.is_empty() || !result.applied_replacements.is_empty(),
        findings_count: i32::try_from(result.findings.len()).unwrap_or(i32::MAX),
        replacements_count: i32::try_from(result.applied_replacements.len()).unwrap_or(i32::MAX),
        types,
        fields: field_values,
    }
}

pub fn summarize_text_for_user(
    text: &str,
    input_kind: InputKind,
    user_id: Option<i64>,
    fields: &[&str],
) -> RedactionUsageSummary {
    let Some(redactor) = redactor_snapshot_for_user(user_id) else {
        return RedactionUsageSummary {
            applied: false,
            findings_count: 0,
            replacements_count: 0,
            types: Vec::new(),
            fields: Vec::new(),
        };
    };
    match redactor.redact_with_input_kind(text, input_kind) {
        Ok(result) => summarize_result(&RedactionPreviewResponse::from(result), fields),
        Err(_) => RedactionUsageSummary {
            applied: false,
            findings_count: 0,
            replacements_count: 0,
            types: Vec::new(),
            fields: Vec::new(),
        },
    }
}

impl From<RedactionResult> for RedactionPreviewResponse {
    fn from(value: RedactionResult) -> Self {
        Self {
            redacted_text: value.redacted_text,
            findings: value.findings,
            applied_replacements: value.applied_replacements,
            stats: value.stats,
        }
    }
}

pub fn truncate(text: &str, max_chars: usize) -> String {
    if text.is_empty() {
        return "empty response".to_string();
    }
    let truncated = text.chars().take(max_chars).collect::<String>();
    if truncated.len() < text.len() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{DateTime, Utc};
    use redactor::{CustomStringMatch, InputKind, RedactorError};

    use crate::test_support::{apply as apply_test_config, lock, secret_redaction};
    use crate::{
        RedactionConfig, RedactionCustomStringRule, RedactionPreviewRequest, apply_config,
        apply_configs, apply_user_config, policy_generation, redact_text_for_user,
    };

    fn custom_string_config(pattern: &str) -> RedactionConfig {
        RedactionConfig {
            enabled: true,
            custom_strings: vec![RedactionCustomStringRule {
                pattern: pattern.to_string(),
                match_type: CustomStringMatch::Exact,
                scope: redactor::CustomStringScope::Text,
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    #[test]
    fn redacts_secrets_in_text() {
        let _guard = secret_redaction();
        let redacted = crate::redact_text("API_TOKEN=sk_live_1234567890ABCDEFghij");

        assert!(!redacted.contains("sk_live_1234567890ABCDEFghij"));
        assert!(redacted.contains("[[RDX:v2:"));
    }

    #[test]
    fn runtime_config_can_disable_redaction() {
        let _guard = apply_test_config(&crate::RedactionConfig {
            enabled: false,
            ..Default::default()
        });

        let redacted = crate::redact_text("API_TOKEN=sk_live_1234567890ABCDEFghij");

        assert_eq!(redacted, "API_TOKEN=sk_live_1234567890ABCDEFghij");
    }

    #[test]
    fn preview_supports_custom_string_rules() {
        let result = crate::preview(&RedactionPreviewRequest {
            text: "tenant=acme".to_string(),
            input_kind: InputKind::Text,
            enabled: true,
            rules: Default::default(),
            custom_strings: vec![RedactionCustomStringRule {
                pattern: "acme".to_string(),
                match_type: CustomStringMatch::Exact,
                scope: redactor::CustomStringScope::Text,
                ..Default::default()
            }],
        })
        .expect("preview should succeed");

        assert!(result.redacted_text.contains("[[RDX:v2:"));
        assert_eq!(result.stats.applied_replacements, 1);
    }

    #[test]
    fn user_runtime_combines_global_and_private_rules() {
        let _guard = lock();
        apply_configs(
            &RedactionConfig {
                enabled: true,
                custom_strings: vec![RedactionCustomStringRule {
                    pattern: "global-secret".to_string(),
                    match_type: CustomStringMatch::Exact,
                    scope: redactor::CustomStringScope::Text,
                    ..Default::default()
                }],
                ..Default::default()
            },
            HashMap::from([(
                42,
                RedactionConfig {
                    enabled: true,
                    custom_strings: vec![RedactionCustomStringRule {
                        pattern: "private-secret".to_string(),
                        match_type: CustomStringMatch::Exact,
                        scope: redactor::CustomStringScope::Text,
                        ..Default::default()
                    }],
                    ..Default::default()
                },
            )]),
        )
        .expect("configs should apply");

        let user_redacted = redact_text_for_user("global-secret private-secret", Some(42));
        assert!(!user_redacted.contains("global-secret"));
        assert!(!user_redacted.contains("private-secret"));

        let global_redacted = redact_text_for_user("global-secret private-secret", None);
        assert!(!global_redacted.contains("global-secret"));
        assert!(global_redacted.contains("private-secret"));
    }

    #[test]
    fn config_normalization_trims_and_deduplicates_custom_strings() {
        let config = RedactionConfig {
            enabled: true,
            custom_strings: vec![
                RedactionCustomStringRule {
                    pattern: "  acme  ".to_string(),
                    match_type: CustomStringMatch::Exact,
                    scope: redactor::CustomStringScope::Text,
                    ..Default::default()
                },
                RedactionCustomStringRule {
                    pattern: "acme".to_string(),
                    match_type: CustomStringMatch::Exact,
                    scope: redactor::CustomStringScope::Text,
                    ..Default::default()
                },
                RedactionCustomStringRule {
                    pattern: "   ".to_string(),
                    match_type: CustomStringMatch::Contains,
                    scope: redactor::CustomStringScope::Line,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };

        let normalized = config.normalized();

        assert_eq!(normalized.custom_strings.len(), 1);
        assert_eq!(normalized.custom_strings[0].pattern, "acme");
    }

    #[test]
    fn preview_request_normalization_trims_and_deduplicates_custom_strings() {
        let request = RedactionPreviewRequest {
            text: "tenant=acme".to_string(),
            input_kind: InputKind::Text,
            enabled: true,
            rules: Default::default(),
            custom_strings: vec![
                RedactionCustomStringRule {
                    pattern: " acme ".to_string(),
                    match_type: CustomStringMatch::Exact,
                    scope: redactor::CustomStringScope::Text,
                    ..Default::default()
                },
                RedactionCustomStringRule {
                    pattern: "acme".to_string(),
                    match_type: CustomStringMatch::Exact,
                    scope: redactor::CustomStringScope::Text,
                    ..Default::default()
                },
            ],
        };

        let normalized = request.normalized();

        assert_eq!(normalized.custom_strings.len(), 1);
        assert_eq!(normalized.custom_strings[0].pattern, "acme");
    }

    #[test]
    fn normalization_preserves_rule_timestamps() {
        let created = DateTime::parse_from_rfc3339("2026-01-02T03:04:05Z")
            .unwrap()
            .with_timezone(&Utc);
        let updated = DateTime::parse_from_rfc3339("2026-02-03T04:05:06Z")
            .unwrap()
            .with_timezone(&Utc);
        let config = RedactionConfig {
            enabled: true,
            custom_strings: vec![
                RedactionCustomStringRule {
                    pattern: "  acme  ".to_string(),
                    match_type: CustomStringMatch::Exact,
                    scope: redactor::CustomStringScope::Text,
                    created_at: Some(created),
                    updated_at: Some(updated),
                },
                RedactionCustomStringRule {
                    pattern: "acme".to_string(),
                    match_type: CustomStringMatch::Exact,
                    scope: redactor::CustomStringScope::Text,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };

        let normalized = config.normalized();
        assert_eq!(normalized.custom_strings.len(), 1);
        assert_eq!(normalized.custom_strings[0].pattern, "acme");
        assert_eq!(normalized.custom_strings[0].created_at, Some(created));
        assert_eq!(normalized.custom_strings[0].updated_at, Some(updated));
    }

    #[test]
    fn legacy_config_without_rule_timestamps_still_parses() {
        let config: RedactionConfig = serde_json::from_value(serde_json::json!({
            "enabled": true,
            "custom_strings": [{ "pattern": "acme", "match_type": "exact", "scope": "text" }]
        }))
        .expect("legacy config parses");

        assert_eq!(config.custom_strings.len(), 1);
        assert_eq!(config.custom_strings[0].created_at, None);
        assert_eq!(config.custom_strings[0].updated_at, None);
    }

    #[test]
    fn identical_global_config_reapply_keeps_the_generation() {
        let _guard = lock();
        let config = custom_string_config("acme");
        apply_config(&config).expect("first apply");
        let generation = policy_generation();
        assert!(redact_text_for_user("tenant=acme", None).contains("[[RDX:v2:"));

        apply_config(&config).expect("repeat apply");
        assert_eq!(
            policy_generation(),
            generation,
            "re-publishing identical config must keep the generation"
        );
        assert!(redact_text_for_user("tenant=acme", None).contains("[[RDX:v2:"));

        let mut retimestamped = config.clone();
        retimestamped.custom_strings[0].created_at = Some(Utc::now());
        retimestamped.custom_strings[0].updated_at = Some(Utc::now());
        apply_config(&retimestamped).expect("timestamp-only apply");
        assert_eq!(
            policy_generation(),
            generation,
            "admin-console timestamps are not runtime content"
        );

        let changed = custom_string_config("globex");
        apply_config(&changed).expect("changed apply");
        assert!(policy_generation() > generation);
        assert!(!redact_text_for_user("tenant=globex", None).contains("globex"));
    }

    #[test]
    fn apply_configs_bumps_only_on_content_change() {
        let _guard = lock();
        let global = custom_string_config("acme");
        let user = custom_string_config("private-secret");
        apply_configs(&global, HashMap::from([(707, user.clone())])).expect("first apply");
        let generation = policy_generation();
        assert!(!redact_text_for_user("private-secret", Some(707)).contains("private-secret"));

        apply_configs(&global, HashMap::from([(707, user.clone())])).expect("repeat apply");
        assert_eq!(policy_generation(), generation);

        let mut changed_user = user;
        changed_user.custom_strings[0].pattern = "renamed-secret".to_string();
        apply_configs(&global, HashMap::from([(707, changed_user)])).expect("changed user apply");
        assert!(policy_generation() > generation);
        assert!(!redact_text_for_user("renamed-secret", Some(707)).contains("renamed-secret"));

        let generation = policy_generation();
        apply_configs(&global, HashMap::new()).expect("dropped user apply");
        assert!(
            policy_generation() > generation,
            "removing a user config changes the effective policy"
        );
    }

    #[test]
    fn apply_user_config_bumps_only_on_content_change() {
        let _guard = lock();
        apply_config(&custom_string_config("acme")).expect("global apply");
        let generation = policy_generation();

        let user = custom_string_config("private-secret");
        apply_user_config(708, &user).expect("first user apply");
        let generation_with_user = policy_generation();
        assert!(generation_with_user > generation);
        assert!(!redact_text_for_user("private-secret", Some(708)).contains("private-secret"));

        apply_user_config(708, &user).expect("repeat user apply");
        assert_eq!(policy_generation(), generation_with_user);

        let mut changed_user = user;
        changed_user.custom_strings[0].pattern = "renamed-secret".to_string();
        apply_user_config(708, &changed_user).expect("changed user apply");
        assert!(policy_generation() > generation_with_user);
        assert!(!redact_text_for_user("renamed-secret", Some(708)).contains("renamed-secret"));
    }

    #[test]
    fn config_rejects_rdx_token() {
        let config = RedactionConfig {
            enabled: true,
            custom_strings: vec![RedactionCustomStringRule {
                pattern: "leak [[RDX:v2:abcdef]] more".to_string(),
                match_type: CustomStringMatch::Exact,
                scope: redactor::CustomStringScope::Text,
                ..Default::default()
            }],
            ..Default::default()
        };

        let err = config
            .validate()
            .expect_err("pattern containing [[RDX:v2: must be rejected");
        match err {
            RedactorError::Validation(message) => {
                assert!(
                    message.contains("pattern must not contain redaction token [[RDX:v2:"),
                    "unexpected validation message: {message}",
                );
            }
            other => panic!("expected RedactorError::Validation, got {other:?}"),
        }
    }
}
