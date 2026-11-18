//! The persistent identity of a node.
//!
//! A node owns exactly one node id. It is generated once, stored under the
//! selected app data root with owner-only permissions and never replaced:
//! a second start of the same installation must keep the same identity, and a
//! damaged file must fail closed instead of silently minting a new identity
//! that would look like a different node to its peers.

use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{relay_secrets, runtime_env};

/// The file name of the node id under the app data root.
pub const NODE_ID_FILE: &str = "node-id";

/// How often a concurrent creation is re-read before the start fails closed.
const CONCURRENT_CREATE_ATTEMPTS: usize = 50;

/// The delay between two attempts to read a concurrently created file.
/// The length of the canonical hyphenated UUID form.
const UUID_TEXT_LENGTH: usize = 36;

const CONCURRENT_CREATE_DELAY: std::time::Duration = std::time::Duration::from_millis(20);

/// The identity of a node.
///
/// The node id is the identity of the installation: it is persisted locally,
/// it never leaves the installation in plaintext and it is the group id of
/// the star the node belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NodeId(Uuid);

impl NodeId {
    /// Build a node id from a UUID.
    pub fn from_uuid(value: Uuid) -> Self {
        Self(value)
    }

    /// The UUID behind the node id.
    pub fn as_uuid(&self) -> Uuid {
        self.0
    }

    /// Parse a node id from its persisted text form.
    ///
    /// The parse is strict: only the canonical hyphenated UUID form is
    /// accepted, so a truncated or padded file is a damaged file.
    pub fn parse(value: &str) -> anyhow::Result<Self> {
        let trimmed = value.trim();
        if trimmed.len() != UUID_TEXT_LENGTH {
            return Err(anyhow!("the node id must be a canonical UUID"));
        }
        let parsed = Uuid::parse_str(trimmed)
            .map_err(|_| anyhow!("the node id must be a canonical UUID"))?;
        Ok(Self(parsed))
    }

    /// The persisted text form of the node id.
    pub fn to_persisted_text(&self) -> String {
        self.0.to_string()
    }
}

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// What the persisted file means for this start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeIdAction {
    /// The file holds a valid node id: it is reused unchanged.
    UseExisting,
    /// No file exists yet: a new node id is created and persisted.
    Create,
    /// The file exists but does not hold a valid node id.
    ///
    /// The file is never overwritten: the start fails closed and the
    /// operator decides what to do.
    Corrupt,
}

/// Decide what the persisted content means.
///
/// The decision is pure, so the corrupt-file behaviour is covered without
/// touching the filesystem.
pub fn decide_node_id_action(existing: Option<&str>) -> NodeIdAction {
    match existing {
        None => NodeIdAction::Create,
        Some(content) => {
            if NodeId::parse(content).is_ok() {
                NodeIdAction::UseExisting
            } else {
                NodeIdAction::Corrupt
            }
        }
    }
}

/// The location of the node id file under the selected app data root.
pub fn node_id_path(secrets_dir: Option<&Path>) -> anyhow::Result<PathBuf> {
    relay_secrets::resolve_data_file(NODE_ID_FILE, secrets_dir)
}

/// Load the node id of this installation, creating it when it is missing.
///
/// An existing valid node id is never replaced. A damaged file fails the
/// start closed: overwriting it would silently change the identity of the
/// node. A concurrent creation is re-read a bounded number of times, because
/// the other starter may still be writing the file.
pub fn load_or_create_node_id(path: &Path) -> anyhow::Result<NodeId> {
    match read_node_id_file(path)? {
        Some(content) => match decide_node_id_action(Some(&content)) {
            NodeIdAction::UseExisting => NodeId::parse(&content),
            NodeIdAction::Corrupt => Err(anyhow!(
                "the node id file {} is damaged; refusing to replace it so the node keeps its \
                 identity",
                path.display()
            )),
            NodeIdAction::Create => unreachable!("an existing file is never a creation"),
        },
        None => create_node_id_file(path),
    }
}

/// Read the raw content of the node id file.
fn read_node_id_file(path: &Path) -> anyhow::Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(content) => Ok(Some(content)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(anyhow!(error).context(format!(
            "failed to read the node id file {}",
            path.display()
        ))),
    }
}

/// Create the node id file with owner-only permissions.
///
/// The file is created exclusively and written in one step, so a concurrent
/// starter either sees the complete file or creates it itself. When another
/// starter won the race, its file is re-read a bounded number of times.
fn create_node_id_file(path: &Path) -> anyhow::Result<NodeId> {
    let node_id = NodeId::from_uuid(Uuid::new_v4());
    let created = runtime_env::create_private_file_exclusive(
        path,
        &format!("{}\n", node_id.to_persisted_text()),
    )
    .context("failed to persist the node id")?;
    if created {
        return Ok(node_id);
    }
    for attempt in 0..CONCURRENT_CREATE_ATTEMPTS {
        std::thread::sleep(CONCURRENT_CREATE_DELAY);
        let content = read_node_id_file(path)?.ok_or_else(|| {
            anyhow!(
                "the node id file {} disappeared while another starter created it",
                path.display()
            )
        })?;
        match decide_node_id_action(Some(&content)) {
            NodeIdAction::UseExisting => return NodeId::parse(&content),
            NodeIdAction::Corrupt => {
                if attempt + 1 == CONCURRENT_CREATE_ATTEMPTS {
                    return Err(anyhow!(
                        "the node id file {} is damaged; refusing to replace it so the node keeps \
                         its identity",
                        path.display()
                    ));
                }
            }
            NodeIdAction::Create => unreachable!("the file exists at this point"),
        }
    }
    Err(anyhow!(
        "the node id file {} could not be read after a concurrent creation",
        path.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::{NodeId, NodeIdAction, decide_node_id_action};
    use uuid::Uuid;

    #[test]
    fn a_missing_file_creates_a_new_identity() {
        assert_eq!(decide_node_id_action(None), NodeIdAction::Create);
    }

    #[test]
    fn a_valid_file_keeps_its_identity() {
        let node_id = NodeId::from_uuid(Uuid::new_v4());
        let content = format!("{}\n", node_id);
        assert_eq!(
            decide_node_id_action(Some(&content)),
            NodeIdAction::UseExisting
        );
        assert_eq!(NodeId::parse(&content).expect("valid id"), node_id);
    }

    #[test]
    fn a_damaged_file_is_never_replaced() {
        for content in [
            "",
            "   ",
            "not-a-uuid",
            "00000000-0000-0000-0000-00000000000",
            "00000000-0000-0000-0000-0000000000000",
            "  00000000-0000-0000-0000-00000000000x  ",
        ] {
            assert_eq!(
                decide_node_id_action(Some(content)),
                NodeIdAction::Corrupt,
                "expected {content:?} to be damaged"
            );
            assert!(NodeId::parse(content).is_err());
        }
    }

    #[test]
    fn the_persisted_form_is_the_canonical_uuid() {
        let node_id = NodeId::from_uuid(Uuid::new_v4());
        assert_eq!(
            NodeId::parse(&node_id.to_persisted_text()).expect("round trip"),
            node_id
        );
        assert_eq!(node_id.to_persisted_text().len(), 36);
    }
}
