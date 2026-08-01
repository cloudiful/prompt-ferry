use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use sqlx::FromRow;
use uuid::Uuid;

use crate::usage::TokenUsage;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BillingMeter {
    Input,
    CacheRead,
    CacheWrite,
    Output,
}

impl BillingMeter {
    pub const ALL: [Self; 4] = [Self::Input, Self::CacheRead, Self::CacheWrite, Self::Output];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::CacheRead => "cache_read",
            Self::CacheWrite => "cache_write",
            Self::Output => "output",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NormalizedBillingUsage {
    pub input_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    pub output_tokens: i64,
}

impl NormalizedBillingUsage {
    pub fn from_usage(usage: &TokenUsage) -> Option<Self> {
        if usage.input_tokens.is_none()
            && usage.output_tokens.is_none()
            && usage.total_tokens.is_none()
            && usage.cached_tokens.is_none()
            && usage.cache_read_tokens.is_none()
            && usage.cache_write_tokens.is_none()
        {
            return None;
        }

        let cache_read_tokens = usage
            .cache_read_tokens
            .or(usage.cached_tokens)
            .unwrap_or_default()
            .max(0);
        let cache_write_tokens = usage.cache_write_tokens.unwrap_or_default().max(0);
        let input_tokens = usage.input_tokens.unwrap_or_default().max(0);
        let ordinary_input_tokens = input_tokens
            .saturating_sub(cache_read_tokens)
            .saturating_sub(cache_write_tokens);
        Some(Self {
            input_tokens: ordinary_input_tokens,
            cache_read_tokens,
            cache_write_tokens,
            output_tokens: usage.output_tokens.unwrap_or_default().max(0),
        })
    }

    pub fn token_count(self, meter: BillingMeter) -> i64 {
        match meter {
            BillingMeter::Input => self.input_tokens,
            BillingMeter::CacheRead => self.cache_read_tokens,
            BillingMeter::CacheWrite => self.cache_write_tokens,
            BillingMeter::Output => self.output_tokens,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::usage::TokenUsage;

    use super::{BillingMeter, NormalizedBillingUsage};

    #[test]
    fn splits_cached_input_into_four_billing_meters() {
        let usage = NormalizedBillingUsage::from_usage(&TokenUsage {
            input_tokens: Some(120),
            output_tokens: Some(40),
            total_tokens: Some(160),
            cached_tokens: Some(30),
            cache_read_tokens: Some(30),
            cache_write_tokens: Some(10),
        })
        .unwrap();

        assert_eq!(usage.token_count(BillingMeter::Input), 80);
        assert_eq!(usage.token_count(BillingMeter::CacheRead), 30);
        assert_eq!(usage.token_count(BillingMeter::CacheWrite), 10);
        assert_eq!(usage.token_count(BillingMeter::Output), 40);
    }

    #[test]
    fn missing_provider_usage_is_unknown() {
        assert!(NormalizedBillingUsage::from_usage(&TokenUsage::default()).is_none());
    }
}

#[derive(Debug, Clone, FromRow)]
pub struct BillingPriceRuleRow {
    pub price_rule_id: Uuid,
    pub public_model: String,
    pub input_rate: Decimal,
    pub cache_read_rate: Decimal,
    pub cache_write_rate: Decimal,
    pub output_rate: Decimal,
    pub currency: String,
    pub effective_from: DateTime<Utc>,
    pub effective_to: Option<DateTime<Utc>>,
    pub enabled: bool,
    pub created_by_user_id: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl BillingPriceRuleRow {
    pub fn rate(&self, meter: BillingMeter) -> Decimal {
        match meter {
            BillingMeter::Input => self.input_rate,
            BillingMeter::CacheRead => self.cache_read_rate,
            BillingMeter::CacheWrite => self.cache_write_rate,
            BillingMeter::Output => self.output_rate,
        }
    }
}

#[derive(Debug, Clone)]
pub struct BillingPriceRuleCreate {
    pub public_model: String,
    pub input_rate: Decimal,
    pub cache_read_rate: Decimal,
    pub cache_write_rate: Decimal,
    pub output_rate: Decimal,
    pub effective_from: DateTime<Utc>,
    pub created_by_user_id: i64,
}

#[derive(Debug, Clone)]
pub struct BillingPriceRuleUpdate {
    pub public_model: String,
    pub input_rate: Decimal,
    pub cache_read_rate: Decimal,
    pub cache_write_rate: Decimal,
    pub output_rate: Decimal,
    pub effective_from: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct BillingChargeFilter {
    pub user_id: Option<i64>,
    pub client_key_id: Option<i64>,
    pub requested_model: Option<String>,
    pub endpoint_id: Option<Uuid>,
    pub usage_status: Option<String>,
    pub pricing_status: Option<String>,
    pub request_id: Option<Uuid>,
    pub start_at: Option<DateTime<Utc>>,
    pub end_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, FromRow)]
pub struct BillingChargeRow {
    pub charge_id: i64,
    pub event_id: Option<i64>,
    pub request_id: Uuid,
    pub user_id: Option<i64>,
    pub user_login_name: Option<String>,
    pub client_key_id: Option<i64>,
    pub client_key_label: Option<String>,
    pub requested_model: Option<String>,
    pub upstream_model: Option<String>,
    pub endpoint_id: Option<Uuid>,
    pub endpoint_name: Option<String>,
    pub endpoint_key_id: Option<Uuid>,
    pub usage_status: String,
    pub pricing_status: String,
    pub currency: String,
    pub customer_amount: Option<Decimal>,
    pub input_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    pub output_tokens: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct BillingChargeLineRow {
    pub line_id: i64,
    pub charge_id: i64,
    pub meter: String,
    pub token_count: i64,
    pub unit_rate: Decimal,
    pub amount: Decimal,
    pub price_rule_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct BillingSummaryRow {
    pub request_count: i64,
    pub known_count: i64,
    pub unknown_count: i64,
    pub priced_count: i64,
    pub unpriced_count: i64,
    pub customer_amount: Decimal,
}

#[derive(Debug, Clone, FromRow)]
pub struct BillingBreakdownRow {
    pub grouping_key: String,
    pub request_count: i64,
    pub input_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    pub output_tokens: i64,
    pub customer_amount: Decimal,
}

#[derive(Debug, Clone, FromRow)]
pub struct BillingExportRow {
    pub charge_id: i64,
    pub request_id: Uuid,
    pub user_login_name: Option<String>,
    pub client_key_label: Option<String>,
    pub requested_model: Option<String>,
    pub upstream_model: Option<String>,
    pub endpoint_name: Option<String>,
    pub usage_status: String,
    pub pricing_status: String,
    pub input_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    pub output_tokens: i64,
    pub customer_amount: Option<Decimal>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct BillingMonthlyExportRow {
    pub month: DateTime<Utc>,
    pub request_count: i64,
    pub known_count: i64,
    pub unknown_count: i64,
    pub priced_count: i64,
    pub unpriced_count: i64,
    pub customer_amount: Decimal,
}
