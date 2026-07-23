use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(
    Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, ValueEnum, ToSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum TlsMode {
    #[default]
    Off,
    Server,
    Mtls,
}

impl TlsMode {
    pub fn enabled(self) -> bool {
        self != Self::Off
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Server => "server",
            Self::Mtls => "mtls",
        }
    }
}

#[derive(
    Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, ValueEnum, ToSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum BridgeEncryptionMode {
    #[default]
    Off,
    Required,
}

impl BridgeEncryptionMode {
    pub fn required(self) -> bool {
        self == Self::Required
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Required => "required",
        }
    }
}

#[derive(
    Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, ValueEnum, ToSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum WorkerTlsMode {
    #[default]
    Auto,
    Off,
    Server,
    Mtls,
}

impl WorkerTlsMode {
    pub fn explicit(self) -> Option<TlsMode> {
        match self {
            Self::Auto => None,
            Self::Off => Some(TlsMode::Off),
            Self::Server => Some(TlsMode::Server),
            Self::Mtls => Some(TlsMode::Mtls),
        }
    }
}

#[derive(
    Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, ValueEnum, ToSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum NativeApi {
    AnthropicMessages,
    Chat,
    #[default]
    Responses,
    Realtime,
}

impl NativeApi {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AnthropicMessages => "anthropic_messages",
            Self::Chat => "chat",
            Self::Responses => "responses",
            Self::Realtime => "realtime",
        }
    }

    pub fn path(self) -> &'static str {
        match self {
            Self::AnthropicMessages => "/v1/messages",
            Self::Chat => "/v1/chat/completions",
            Self::Responses => "/v1/responses",
            Self::Realtime => "/v1/realtime",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum NativeApiSource {
    Detected,
    #[default]
    Manual,
}

impl NativeApiSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Detected => "detected",
            Self::Manual => "manual",
        }
    }
}
