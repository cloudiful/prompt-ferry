//! Unit tests for the historical usage-token backfill module.
//!
//! Split into `decision` (decide_repair / billing_lookup_key / field_changed)
//! and `outcome` (classify_outcome / BackfillStats). Shared fixture builders
//! live in `fixtures`.

mod decision;
mod fixtures;
mod outcome;
