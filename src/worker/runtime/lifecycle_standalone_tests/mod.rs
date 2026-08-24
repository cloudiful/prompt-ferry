//! Focused tests for the standalone request-lease slice. The store
//! tests exercise the SQL contract directly; the lifecycle tests cover
//! the heartbeat and stale-reconciler boundaries without driving the
//! full worker runtime.

mod lifecycle_tests;
mod store_tests;
mod support;
