//! Fixed client diagnostics for the builtin MiniMax tools (issue #1179 P2).
//!
//! Every builtin failure that reaches the MCP client is one of these values.
//! The text is fixed and carries bounded numeric codes only: a provider
//! message, a transport exception, a trace header, the query, the image source
//! and any credential — configured, disabled or inline — never appear in it.
//! The value is turned into a plain message error with no source chain, so no
//! debug or alternate formatting can reveal the original failure.

use std::fmt;

/// One builtin failure with a fixed, safe description.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Diagnostic {
    SourceEndpointMissing,
    SourceEndpointUnavailable,
    SourceEndpointNotMinimax,
    SourceEndpointDisabled,
    SourceEndpointRegionMissing,
    ProxyRejected,
    UnsupportedTool,
    MissingToolName,
    QueryRequired,
    PromptRequired,
    ImageSourceRequired,
    ImageSourceRejected,
    NoEndpointKey,
    PoolExhausted,
    PoolCooling,
    PoolProbeBusy,
    PoolBlocked,
    Deadline,
    RequestTimedOut,
    TransportFailed { status: Option<u16> },
    UpstreamStatus { status: u16 },
    RateLimited { status: u16 },
    ProviderError { provider_code: i64 },
    QuotaExhausted { provider_code: i64 },
    MalformedResponse { tool: &'static str },
    Superseded,
}

impl Diagnostic {
    /// The client-facing error of this diagnostic.
    ///
    /// The returned error has no source chain: the original provider or
    /// transport failure is dropped instead of being wrapped.
    pub(super) fn into_error(self) -> anyhow::Error {
        anyhow::anyhow!("{self}")
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SourceEndpointMissing => {
                write!(formatter, "MiniMax MCP server has no source endpoint")
            }
            Self::SourceEndpointUnavailable => {
                write!(formatter, "MiniMax MCP source endpoint is unavailable")
            }
            Self::SourceEndpointNotMinimax => {
                write!(
                    formatter,
                    "MiniMax MCP source endpoint is not a MiniMax endpoint"
                )
            }
            Self::SourceEndpointDisabled => {
                write!(formatter, "MiniMax MCP source endpoint is disabled")
            }
            Self::SourceEndpointRegionMissing => {
                write!(formatter, "MiniMax MCP source endpoint has no region")
            }
            Self::ProxyRejected => {
                write!(
                    formatter,
                    "MiniMax MCP endpoint proxy configuration is invalid"
                )
            }
            Self::UnsupportedTool => write!(formatter, "unknown MiniMax MCP tool"),
            Self::MissingToolName => write!(formatter, "MCP tools/call is missing params.name"),
            Self::QueryRequired => write!(formatter, "web_search requires a non-empty query"),
            Self::PromptRequired => {
                write!(formatter, "understand_image requires a non-empty prompt")
            }
            Self::ImageSourceRequired => {
                write!(formatter, "understand_image requires image_source")
            }
            Self::ImageSourceRejected => {
                write!(formatter, "image_source is not an allowed image source")
            }
            Self::NoEndpointKey => {
                write!(
                    formatter,
                    "MiniMax MCP source endpoint has no enabled API key"
                )
            }
            Self::PoolExhausted => write!(
                formatter,
                "MiniMax MCP: every enabled API key reports an exhausted quota"
            ),
            Self::PoolCooling => write!(
                formatter,
                "MiniMax MCP: every enabled API key is rate limited, retry later"
            ),
            Self::PoolProbeBusy => write!(
                formatter,
                "MiniMax MCP: every enabled API key is being probed by another request"
            ),
            Self::PoolBlocked => {
                write!(
                    formatter,
                    "MiniMax MCP: no enabled API key is currently usable"
                )
            }
            Self::Deadline => write!(formatter, "MiniMax MCP call deadline exceeded"),
            Self::RequestTimedOut => write!(formatter, "MiniMax MCP request timed out"),
            Self::TransportFailed {
                status: Some(status),
            } => {
                write!(
                    formatter,
                    "MiniMax MCP upstream call failed with HTTP {status}"
                )
            }
            Self::TransportFailed { status: None } => {
                write!(formatter, "MiniMax MCP upstream call failed")
            }
            Self::UpstreamStatus { status } => {
                write!(formatter, "MiniMax API returned HTTP {status}")
            }
            Self::RateLimited { status } => {
                write!(
                    formatter,
                    "MiniMax API rate limited the request (HTTP {status})"
                )
            }
            Self::ProviderError { provider_code } => {
                write!(formatter, "MiniMax API returned error {provider_code}")
            }
            Self::QuotaExhausted { provider_code } => {
                write!(
                    formatter,
                    "MiniMax API reported an exhausted plan quota ({provider_code})"
                )
            }
            Self::MalformedResponse { tool } => write!(
                formatter,
                "MiniMax API returned a response that is not the documented {tool} payload"
            ),
            Self::Superseded => {
                write!(
                    formatter,
                    "MiniMax MCP call ended without a successful upstream attempt"
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_diagnostic_is_fixed_and_carries_only_bounded_numerics() {
        for diagnostic in [
            Diagnostic::SourceEndpointMissing,
            Diagnostic::SourceEndpointUnavailable,
            Diagnostic::SourceEndpointNotMinimax,
            Diagnostic::SourceEndpointDisabled,
            Diagnostic::SourceEndpointRegionMissing,
            Diagnostic::ProxyRejected,
            Diagnostic::UnsupportedTool,
            Diagnostic::MissingToolName,
            Diagnostic::QueryRequired,
            Diagnostic::PromptRequired,
            Diagnostic::ImageSourceRequired,
            Diagnostic::ImageSourceRejected,
            Diagnostic::NoEndpointKey,
            Diagnostic::PoolExhausted,
            Diagnostic::PoolCooling,
            Diagnostic::PoolProbeBusy,
            Diagnostic::PoolBlocked,
            Diagnostic::Deadline,
            Diagnostic::RequestTimedOut,
            Diagnostic::TransportFailed { status: Some(502) },
            Diagnostic::TransportFailed { status: None },
            Diagnostic::UpstreamStatus { status: 401 },
            Diagnostic::RateLimited { status: 429 },
            Diagnostic::ProviderError {
                provider_code: 1000,
            },
            Diagnostic::QuotaExhausted {
                provider_code: 2056,
            },
            Diagnostic::MalformedResponse { tool: "web_search" },
            Diagnostic::Superseded,
        ] {
            let error = diagnostic.into_error();
            assert_eq!(error.to_string(), diagnostic.to_string());
            assert!(
                error.source().is_none(),
                "the fixed diagnostic never wraps a source: {error:?}"
            );
            // anyhow prints a wrapped error as a `Caused by:` chain: its
            // absence is the proof that no raw failure can surface through
            // debug formatting.
            assert!(
                !format!("{error:?}").contains("Caused by"),
                "the fixed diagnostic has no source chain: {error:?}"
            );
        }
    }

    #[test]
    fn the_quota_diagnostic_keeps_its_provider_code() {
        assert_eq!(
            Diagnostic::QuotaExhausted {
                provider_code: 2067
            }
            .to_string(),
            "MiniMax API reported an exhausted plan quota (2067)"
        );
        assert_eq!(
            Diagnostic::RateLimited { status: 429 }.to_string(),
            "MiniMax API rate limited the request (HTTP 429)"
        );
    }
}
