//! Unit tests for the ChatGPT quota service.
//!
//! The retained cases cover the pure refresh-cycle and cadence policy: the
//! bounded retry cycle, the replica-wait deadline, and the active/idle
//! refresh cadences. Everything that needs a database fixture or an upstream
//! account is out of scope for this suite.

mod policy;
mod refresh_cycle_tests;
