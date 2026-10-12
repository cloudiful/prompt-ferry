//! Deciding which stored endpoint API key rows a save actually has to change.
//!
//! Saving a key set used to rewrite all of it: every position was shifted out
//! of the way, everything not retained was deleted, and then every incoming key
//! was updated. A key the operator never touched therefore still got a full row
//! write and a fresh `updated_at`. This module answers the same question with no
//! database access, so the writer issues only the statements the request implies.
//!
//! The request is described by the caller's `key_id` when it sends one and by
//! `key_label` otherwise, which is the matching the writer has always used, so
//! a client that omits `key_id` still keeps the identity and creation time of
//! the key it means.

use std::collections::{HashMap, HashSet};

use anyhow::{Result, bail};
use uuid::Uuid;

use crate::db::types::{EndpointApiKey, EndpointApiKeyCreate};

/// How far a moving key is shifted while the ones that stay put hold their
/// slots. The band it reserves has to be free of both stored and requested
/// positions, which [`plan_key_persistence`] checks.
pub(crate) const RENUMBER_OFFSET: i32 = 1_000_000;

/// A stored key whose label, secret or enabled flag differs from the request.
///
/// Carries a secret, so it deliberately does not derive `Debug`.
pub(crate) struct KeyRewrite {
    pub key_id: Uuid,
    pub key_label: String,
    pub api_key: String,
    pub enabled: bool,
    pub position: i32,
}

/// A key that is not stored yet.
///
/// Carries a secret, so it deliberately does not derive `Debug`.
pub(crate) struct KeyInsert {
    pub key_label: String,
    pub api_key: String,
    pub enabled: bool,
    pub position: i32,
}

/// The minimum set of statements that turns the stored key set into the
/// requested one.
pub(crate) struct DiffPlan {
    /// Keys whose edited fields changed. Rewriting a key is what advances its
    /// `updated_at`; its `created_at` is never reset.
    pub rewrites: Vec<KeyRewrite>,
    /// Keys whose position changed, as `(key_id, requested position)`. These
    /// are shifted out of the way first so a reorder cannot collide with a row
    /// that has not moved yet, then put back at their requested position.
    pub repositions: Vec<(Uuid, i32)>,
    /// Keys to insert, in request order.
    pub inserts: Vec<KeyInsert>,
    /// Keys the request drops.
    pub removed: Vec<Uuid>,
    /// Every key that survives, which is what the delete statement keeps.
    pub retained: Vec<Uuid>,
}

impl DiffPlan {
    /// Whether the stored set already is the requested set.
    pub fn is_empty(&self) -> bool {
        self.rewrites.is_empty()
            && self.repositions.is_empty()
            && self.inserts.is_empty()
            && self.removed.is_empty()
    }
}

/// Compare the stored key set of one endpoint against the requested one.
///
/// Returns an error — before any statement runs, so the caller's transaction
/// rolls back — when the request cannot be stored at all: a key claimed twice,
/// a position claimed twice, or an identity that is not part of this endpoint.
pub(crate) fn plan_key_persistence(
    current: &[EndpointApiKey],
    incoming: &[EndpointApiKeyCreate],
) -> Result<DiffPlan> {
    let resolved = resolve_identities(current, incoming)?;

    let mut plan = DiffPlan {
        rewrites: Vec::new(),
        repositions: Vec::new(),
        inserts: Vec::new(),
        removed: Vec::new(),
        retained: Vec::new(),
    };
    for (requested, key_id) in incoming.iter().zip(resolved) {
        let Some(key_id) = key_id else {
            plan.inserts.push(KeyInsert {
                key_label: requested.key_label.clone(),
                api_key: requested.api_key.clone(),
                enabled: requested.enabled,
                position: requested.position,
            });
            continue;
        };
        let stored = current
            .iter()
            .find(|key| key.key_id == key_id)
            .expect("identity was resolved against `current`");
        plan.retained.push(key_id);
        if stored.key_label != requested.key_label
            || stored.api_key != requested.api_key
            || stored.enabled != requested.enabled
        {
            plan.rewrites.push(KeyRewrite {
                key_id,
                key_label: requested.key_label.clone(),
                api_key: requested.api_key.clone(),
                enabled: requested.enabled,
                position: requested.position,
            });
        }
        if stored.position != requested.position {
            plan.repositions.push((key_id, requested.position));
        }
    }

    let kept = plan.retained.iter().copied().collect::<HashSet<_>>();
    plan.removed = current
        .iter()
        .filter(|key| !kept.contains(&key.key_id))
        .map(|key| key.key_id)
        .collect();

    check_renumber_band(current, incoming, &plan)?;
    Ok(plan)
}

/// Bind every requested key to the stored row it refers to, or to nothing when
/// it is new.
fn resolve_identities(
    current: &[EndpointApiKey],
    incoming: &[EndpointApiKeyCreate],
) -> Result<Vec<Option<Uuid>>> {
    let stored = current.iter().map(|key| key.key_id).collect::<HashSet<_>>();
    let mut claimed = HashSet::<Uuid>::new();
    let mut positions = HashMap::<i32, ()>::new();
    let mut resolved = Vec::with_capacity(incoming.len());

    for requested in incoming {
        // The caller's key id is authoritative when it names a stored row, so a
        // save that renames a key keeps its identity. An id that is no longer
        // stored is stale — a concurrent save may have replaced it — and the
        // label still identifies what the request means.
        let key_id = requested
            .key_id
            .filter(|key_id| stored.contains(key_id))
            .or_else(|| {
                current
                    .iter()
                    .find(|key| key.key_label == requested.key_label)
                    .map(|key| key.key_id)
            });
        if let Some(key_id) = key_id {
            if !claimed.insert(key_id) {
                bail!("endpoint api key {key_id} is requested twice");
            }
        }
        if positions.insert(requested.position, ()).is_some() {
            bail!(
                "endpoint api key position {} is requested twice",
                requested.position
            );
        }
        resolved.push(key_id);
    }
    Ok(resolved)
}

/// Keep the reserved reorder band free and inside `i32`.
///
/// The writer shifts a moving key by [`RENUMBER_OFFSET`] while the keys that
/// stay put hold their slots, so a key's restored position has to be outside
/// the shifted range — otherwise it would be put back on top of another key.
/// The shifted range is derived from the stored positions because those are the
/// ones that move.
fn check_renumber_band(
    current: &[EndpointApiKey],
    incoming: &[EndpointApiKeyCreate],
    plan: &DiffPlan,
) -> Result<()> {
    if plan.repositions.is_empty() {
        return Ok(());
    }
    let Some(&lowest) = current.iter().map(|key| key.position).min().as_ref() else {
        return Ok(());
    };
    let highest = current
        .iter()
        .map(|key| key.position)
        .max()
        .unwrap_or(lowest);
    let (Some(band_start), Some(band_end)) = (
        lowest.checked_add(RENUMBER_OFFSET),
        highest.checked_add(RENUMBER_OFFSET),
    ) else {
        bail!("endpoint api key position has no room to reorder");
    };
    if incoming
        .iter()
        .any(|key| key.position >= band_start && key.position <= band_end)
    {
        bail!("endpoint api key position collides with the reorder band");
    }
    Ok(())
}
