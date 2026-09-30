//! Focused tests for the administrator configuration import.
//!
//! `authorization` covers the admin gate; `preview` covers the read-only dry
//! run and its rejection paths; `round_trip` covers a committed replace and
//! rollback on a failed write.

mod authorization;
mod harness;
mod preview;
mod round_trip;

use super::*;

use harness::{
    PASSPHRASE, attach_session, close_state, decode, export_archive, fingerprint, import_request,
    json_body, seal, seed_configuration, snapshot, test_state,
};
