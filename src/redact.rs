use std::{
    collections::{HashMap, HashSet},
    sync::{LazyLock, RwLock},
};

use redactor::{
    AppliedReplacement, CustomStringRule, Finding, InputKind, RedactionPolicy, RedactionResult,
    RedactionRules, RedactionStats, Redactor, RedactorBuilder, RedactorError,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RedactionConfig {
    pub enabled: bool,
    #[serde(default)]
    pub rules: RedactionRules,
    #[serde(default)]
    pub custom_strings: Vec<CustomStringRule>,
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
            custom_strings: normalize_custom_strings(&self.custom_strings),
            custom_files: Vec::new(),
        }
    }

    pub fn effective_with(&self, user_config: &RedactionConfig) -> RedactionConfig {
        RedactionConfig {
            enabled: self.enabled || user_config.enabled,
            rules: RedactionRules {
                secret: self.rules.secret || user_config.rules.secret,
                domain: self.rules.domain || user_config.rules.domain,
                url: self.rules.url || user_config.rules.url,
                email: self.rules.email || user_config.rules.email,
                ip: self.rules.ip || user_config.rules.ip,
                cidr: self.rules.cidr || user_config.rules.cidr,
                phone: self.rules.phone || user_config.rules.phone,
                person: self.rules.person || user_config.rules.person,
                organization: self.rules.organization || user_config.rules.organization,
            },
            custom_strings: self
                .custom_strings
                .iter()
                .cloned()
                .chain(user_config.custom_strings.iter().cloned())
                .collect(),
        }
    }

    pub fn validate(&self) -> Result<(), RedactorError> {
        self.normalized()
            .policy()
            .validate()
            .map_err(RedactorError::Validation)
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
    pub custom_strings: Vec<CustomStringRule>,
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
            custom_strings: normalize_custom_strings(&self.custom_strings),
        }
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
        let normalized = config.normalized();
        normalized.validate()?;
        let redactor = RedactorBuilder::new()
            .with_redaction_policy(normalized.policy())
            .build();
        Ok(Self {
            enabled: normalized.enabled,
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
            let effective = global_config.effective_with(user_config);
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

#[cfg(test)]
pub static TEST_REDACTION_LOCK: LazyLock<std::sync::Mutex<()>> =
    LazyLock::new(|| std::sync::Mutex::new(()));

pub fn apply_config(config: &RedactionConfig) -> Result<(), RedactorError> {
    let normalized = config.normalized();
    let runtime = RedactionRuntime::from_config(&normalized)?;
    let mut store = REDACTION_RUNTIME
        .write()
        .expect("redaction runtime lock poisoned");
    let user_runtimes =
        RedactionRuntimeStore::build_user_runtimes(&normalized, &store.user_configs)?;
    store.global_config = normalized;
    store.global_runtime = runtime;
    store.user_runtimes = user_runtimes;
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
    *REDACTION_RUNTIME
        .write()
        .expect("redaction runtime lock poisoned") = RedactionRuntimeStore {
        global_config: normalized_global,
        global_runtime,
        user_configs: normalized_users,
        user_runtimes,
    };
    Ok(())
}

pub fn apply_user_config(user_id: i64, config: &RedactionConfig) -> Result<(), RedactorError> {
    let normalized = config.normalized();
    normalized.validate()?;
    let mut store = REDACTION_RUNTIME
        .write()
        .expect("redaction runtime lock poisoned");
    let effective = store.global_config.effective_with(&normalized);
    let runtime = RedactionRuntime::from_config(&effective)?;
    store.user_configs.insert(user_id, normalized);
    store.user_runtimes.insert(user_id, runtime);
    Ok(())
}

fn normalize_custom_strings(custom_strings: &[CustomStringRule]) -> Vec<CustomStringRule> {
    let mut seen = HashSet::<(String, String, String)>::new();
    let mut normalized = Vec::new();
    for rule in custom_strings {
        let pattern = rule.pattern.trim();
        if pattern.is_empty() {
            continue;
        }
        let key = (
            pattern.to_string(),
            format!("{:?}", rule.match_type),
            format!("{:?}", rule.scope),
        );
        if seen.insert(key) {
            normalized.push(CustomStringRule {
                pattern: pattern.to_string(),
                match_type: rule.match_type.clone(),
                scope: rule.scope.clone(),
            });
        }
    }
    normalized
}

pub fn has_any_enabled() -> bool {
    REDACTION_RUNTIME
        .read()
        .expect("redaction runtime lock poisoned")
        .has_any_enabled()
}

pub fn effective_config_for_user(user_id: Option<i64>) -> RedactionConfig {
    let store = REDACTION_RUNTIME
        .read()
        .expect("redaction runtime lock poisoned");
    match user_id.and_then(|id| store.user_configs.get(&id)) {
        Some(user_config) => store.global_config.effective_with(user_config),
        None => store.global_config.clone(),
    }
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

pub fn redact_text(text: &str) -> String {
    redact_text_for_user(text, None)
}

pub fn redact_text_for_user(text: &str, user_id: Option<i64>) -> String {
    let store = REDACTION_RUNTIME
        .read()
        .expect("redaction runtime lock poisoned");
    let runtime = user_id
        .and_then(|user_id| store.user_runtimes.get(&user_id))
        .unwrap_or(&store.global_runtime);
    if !runtime.enabled {
        return text.to_string();
    }
    runtime
        .redactor
        .redact_with_input_kind(text, InputKind::Text)
        .map(|result| result.redacted_text)
        .unwrap_or_else(|_| text.to_string())
}

pub fn preview(
    request: &RedactionPreviewRequest,
) -> Result<RedactionPreviewResponse, RedactorError> {
    let config = request.config();
    config.validate()?;
    if !config.enabled {
        return Ok(RedactionPreviewResponse {
            redacted_text: request.text.clone(),
            findings: Vec::new(),
            applied_replacements: Vec::new(),
            stats: RedactionStats::default(),
        });
    }
    let result = RedactorBuilder::new()
        .with_redaction_policy(config.policy())
        .build()
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
    let store = REDACTION_RUNTIME
        .read()
        .expect("redaction runtime lock poisoned");
    let runtime = user_id
        .and_then(|value| store.user_runtimes.get(&value))
        .unwrap_or(&store.global_runtime);
    if !runtime.enabled {
        return RedactionUsageSummary {
            applied: false,
            findings_count: 0,
            replacements_count: 0,
            types: Vec::new(),
            fields: Vec::new(),
        };
    }
    match runtime.redactor.redact_with_input_kind(text, input_kind) {
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

pub fn redact_error(err: &(dyn std::error::Error + 'static)) -> String {
    redact_text(&err.to_string())
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

    use redactor::{CustomStringMatch, InputKind, RedactionRules};

    use super::{
        RedactionConfig, RedactionPreviewRequest, apply_config, apply_configs, redact_text_for_user,
    };

    #[test]
    fn redacts_secrets_in_text() {
        let _guard = super::TEST_REDACTION_LOCK
            .lock()
            .expect("test lock poisoned");
        apply_config(&RedactionConfig {
            enabled: true,
            rules: RedactionRules {
                secret: true,
                ..RedactionRules::default()
            },
            ..Default::default()
        })
        .expect("config should apply");
        let redacted = super::redact_text("API_TOKEN=sk_live_1234567890ABCDEFghij");

        assert!(!redacted.contains("sk_live_1234567890ABCDEFghij"));
        assert!(redacted.contains("[[RDX:v2:"));
    }

    #[test]
    fn runtime_config_can_disable_redaction() {
        let _guard = super::TEST_REDACTION_LOCK
            .lock()
            .expect("test lock poisoned");
        apply_config(&super::RedactionConfig {
            enabled: false,
            ..Default::default()
        })
        .expect("config should apply");

        let redacted = super::redact_text("API_TOKEN=sk_live_1234567890ABCDEFghij");

        assert_eq!(redacted, "API_TOKEN=sk_live_1234567890ABCDEFghij");
    }

    #[test]
    fn preview_supports_custom_string_rules() {
        let result = super::preview(&RedactionPreviewRequest {
            text: "tenant=acme".to_string(),
            input_kind: InputKind::Text,
            enabled: true,
            rules: Default::default(),
            custom_strings: vec![redactor::CustomStringRule {
                pattern: "acme".to_string(),
                match_type: CustomStringMatch::Exact,
                scope: redactor::CustomStringScope::Text,
            }],
        })
        .expect("preview should succeed");

        assert!(result.redacted_text.contains("[[RDX:v2:"));
        assert_eq!(result.stats.applied_replacements, 1);
    }

    #[test]
    fn user_runtime_combines_global_and_private_rules() {
        let _guard = super::TEST_REDACTION_LOCK
            .lock()
            .expect("test lock poisoned");
        apply_configs(
            &RedactionConfig {
                enabled: true,
                custom_strings: vec![redactor::CustomStringRule {
                    pattern: "global-secret".to_string(),
                    match_type: CustomStringMatch::Exact,
                    scope: redactor::CustomStringScope::Text,
                }],
                ..Default::default()
            },
            HashMap::from([(
                42,
                RedactionConfig {
                    enabled: true,
                    custom_strings: vec![redactor::CustomStringRule {
                        pattern: "private-secret".to_string(),
                        match_type: CustomStringMatch::Exact,
                        scope: redactor::CustomStringScope::Text,
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
                redactor::CustomStringRule {
                    pattern: "  acme  ".to_string(),
                    match_type: CustomStringMatch::Exact,
                    scope: redactor::CustomStringScope::Text,
                },
                redactor::CustomStringRule {
                    pattern: "acme".to_string(),
                    match_type: CustomStringMatch::Exact,
                    scope: redactor::CustomStringScope::Text,
                },
                redactor::CustomStringRule {
                    pattern: "   ".to_string(),
                    match_type: CustomStringMatch::Contains,
                    scope: redactor::CustomStringScope::Line,
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
                redactor::CustomStringRule {
                    pattern: " acme ".to_string(),
                    match_type: CustomStringMatch::Exact,
                    scope: redactor::CustomStringScope::Text,
                },
                redactor::CustomStringRule {
                    pattern: "acme".to_string(),
                    match_type: CustomStringMatch::Exact,
                    scope: redactor::CustomStringScope::Text,
                },
            ],
        };

        let normalized = request.normalized();

        assert_eq!(normalized.custom_strings.len(), 1);
        assert_eq!(normalized.custom_strings[0].pattern, "acme");
    }
}
