mod capture;
#[cfg(test)]
mod capture_tests;

pub use capture::{
    AssistantArtifact, AssistantArtifactCapture, ResponsesArtifactCapture, fallback_text_artifact,
};
