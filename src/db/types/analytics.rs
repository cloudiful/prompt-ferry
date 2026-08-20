use serde::{Deserialize, Serialize};
use sqlx::Type;
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Type, ToSchema)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "text")]
#[sqlx(rename_all = "snake_case")]
pub enum RequestFailureFamily {
    Auth,
    RateLimit,
    Quota,
    Timeout,
    #[serde(rename = "upstream_4xx")]
    #[sqlx(rename = "upstream_4xx")]
    Upstream4xx,
    #[serde(rename = "upstream_5xx")]
    #[sqlx(rename = "upstream_5xx")]
    Upstream5xx,
    Network,
    EmptySuccess,
    Policy,
    Unknown,
}

impl RequestFailureFamily {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auth => "auth",
            Self::RateLimit => "rate_limit",
            Self::Quota => "quota",
            Self::Timeout => "timeout",
            Self::Upstream4xx => "upstream_4xx",
            Self::Upstream5xx => "upstream_5xx",
            Self::Network => "network",
            Self::EmptySuccess => "empty_success",
            Self::Policy => "policy",
            Self::Unknown => "unknown",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RequestFailureFamily;

    #[test]
    fn request_failure_family_uses_legacy_db_wire_names() {
        let four_xx: RequestFailureFamily = serde_json::from_str("\"upstream_4xx\"").unwrap();
        let five_xx: RequestFailureFamily = serde_json::from_str("\"upstream_5xx\"").unwrap();

        assert_eq!(four_xx, RequestFailureFamily::Upstream4xx);
        assert_eq!(five_xx, RequestFailureFamily::Upstream5xx);
        assert_eq!(
            serde_json::to_string(&RequestFailureFamily::Upstream4xx).unwrap(),
            "\"upstream_4xx\""
        );
        assert_eq!(
            serde_json::to_string(&RequestFailureFamily::Upstream5xx).unwrap(),
            "\"upstream_5xx\""
        );
        assert_eq!(RequestFailureFamily::Upstream4xx.as_str(), "upstream_4xx");
        assert_eq!(RequestFailureFamily::Upstream5xx.as_str(), "upstream_5xx");
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, Type, ToSchema)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "text")]
#[sqlx(rename_all = "snake_case")]
pub enum RouteSelectionReason {
    #[default]
    Default,
    SessionAffinity,
    SessionLoadBalance,
    ConversationOverride,
}

impl RouteSelectionReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::SessionAffinity => "session_affinity",
            Self::SessionLoadBalance => "session_load_balance",
            Self::ConversationOverride => "conversation_override",
        }
    }
}
