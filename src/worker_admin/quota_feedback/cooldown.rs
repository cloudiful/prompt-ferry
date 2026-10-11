use super::{PROBE_TTL, QuotaFeedbackEligibility};
use std::time::{Duration, Instant};

pub(super) struct Feedback {
    pub touched: Instant,
    pub failed_at: Instant,
    until: Instant,
    strikes: u32,
    pub probe: Option<Instant>,
}

impl Feedback {
    pub fn new(now: Instant) -> Self {
        Self {
            touched: now,
            failed_at: now,
            until: now,
            strikes: 0,
            probe: None,
        }
    }

    pub fn eligibility(&self, now: Instant) -> QuotaFeedbackEligibility {
        if now < self.until {
            return QuotaFeedbackEligibility::Cooling;
        }
        if self
            .probe
            .is_some_and(|start| now.saturating_duration_since(start) < PROBE_TTL)
        {
            QuotaFeedbackEligibility::ProbeBusy
        } else {
            QuotaFeedbackEligibility::ProbeRequired
        }
    }

    pub fn remaining_until(&self, now: Instant) -> Option<Duration> {
        if now < self.until {
            Some(self.until.saturating_duration_since(now))
        } else {
            self.probe
                .map(|start| PROBE_TTL.saturating_sub(now.saturating_duration_since(start)))
                .filter(|remaining| !remaining.is_zero())
        }
    }

    pub fn fail(&mut self, retry_after: Option<Duration>, now: Instant, attempt_start: Instant) {
        self.strikes = self.strikes.saturating_add(1);
        let delay = retry_after
            .map(|delay| delay.min(Duration::from_secs(3600)))
            .unwrap_or_else(|| {
                Duration::from_secs(
                    (30_u64.saturating_mul(1_u64 << self.strikes.saturating_sub(1).min(4)))
                        .min(300),
                )
            });
        self.until = self.until.max(now + delay);
        self.failed_at = self.failed_at.max(now);
        self.touched = now;
        if self.probe.is_some_and(|start| start <= attempt_start) {
            self.probe = None;
        }
    }
}
