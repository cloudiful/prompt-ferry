//! Shared setup for the endpoint key persistence tests.
//!
//! Both test binaries need the same endpoint fixtures and the same reads, so
//! they live here and are pulled in with `#[path]`, which keeps each binary its
//! own copy of the fixture code.

#[allow(dead_code)]
#[path = "endpoint_key_persistence_fixtures.rs"]
mod fixtures;

pub(crate) use fixtures::*;
