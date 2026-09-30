//! Focused tests for the administrator configuration archive audit trail.
//!
//! `authorization` covers the admin gate on the list endpoint; `pagination`
//! covers the page window; `audit_trail` covers what a real export/import
//! attempt records and proves the stored rows carry no secret material.

mod audit_trail;
mod authorization;
mod harness;
mod pagination;

use super::*;

use harness::{
    PASSPHRASE, attach_session, audit_page, audit_request, close_state, export_archive,
    export_request, import_request, json_body, seed_configuration, test_state,
};
