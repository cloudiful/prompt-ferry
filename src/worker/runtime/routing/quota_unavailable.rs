use std::fmt;

#[derive(Debug, Clone, Copy)]
pub(in crate::worker::runtime) struct QuotaUnavailableError;

impl QuotaUnavailableError {
    pub(in crate::worker::runtime) const CODE: &'static str = "quota_unavailable";
    pub(in crate::worker::runtime) const MESSAGE: &'static str =
        "no eligible quota capacity is currently available";
}

impl fmt::Display for QuotaUnavailableError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(Self::MESSAGE)
    }
}
impl std::error::Error for QuotaUnavailableError {}
