//! Handler tests for the administrator configuration export.
//!
//! The shared SQLite admin-state harness lives in [`harness`]; the authorization
//! surface and the archive/metadata behavior are asserted by their own
//! submodules so each file stays single-purpose.

use super::*;

mod archive_output;
mod authorization;
mod harness;

use harness::{
    PASSPHRASE, attach_session, close_state, export_request, metadata_request, seed_configuration,
    test_state,
};
