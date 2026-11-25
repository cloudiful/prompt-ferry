//! The topology of a star and the deduplication of its links.
//!
//! A star is one database node and the edge nodes that belong to it. Two
//! database nodes never form a star, and a node never links to itself. Two
//! database-less nodes may be linked: such a link carries the traffic of a
//! node that has no route of its own towards the database node.
//!
//! When both sides dial at the same time, the link that the
//! lexicographically smaller node id initiated wins, so the star converges on
//! one connection per peer.

use std::collections::HashMap;

use tokio::sync::Mutex;

use crate::worker::runtime::node::identity::NodeId;

/// How many edge nodes one database node accepts.
pub const MAX_PEERS_PER_NODE: usize = crate::relay::MAX_NODE_PEERS;

/// What a node can do inside a star.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeRole {
    /// The node owns the database of its star and executes the requests.
    Database,
    /// The node has no database and forwards its business traffic.
    Edge,
}

impl NodeRole {
    /// The role of a node from its hello.
    pub fn from_has_database(has_database: bool) -> Self {
        if has_database {
            Self::Database
        } else {
            Self::Edge
        }
    }
}

/// Why a peer link was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerError {
    /// The peer claims the identity of this node.
    SelfConnection,
    /// The two roles cannot form a star.
    IncompatibleRole,
    /// The node already serves the maximum number of peers.
    PeerCapReached,
    /// Another link to the same peer is the preferred one.
    DuplicateConnection,
}

impl std::fmt::Display for PeerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SelfConnection => f.write_str("a node cannot link to itself"),
            Self::IncompatibleRole => f.write_str(
                "two database nodes cannot be linked; a database-less node may be linked to any \
                 other node",
            ),
            Self::PeerCapReached => {
                write!(
                    f,
                    "the node already serves the maximum of {MAX_PEERS_PER_NODE} peers"
                )
            }
            Self::DuplicateConnection => {
                f.write_str("another link to this peer is the preferred one")
            }
        }
    }
}

impl std::error::Error for PeerError {}

/// Check that two nodes may be linked.
///
/// Two database nodes never form a star, and a node never links to itself.
/// Two database-less nodes may be linked: the link carries the traffic of a
/// node that has no route of its own.
pub fn check_pairing(
    local_role: NodeRole,
    local_id: NodeId,
    remote_role: NodeRole,
    remote_id: NodeId,
) -> Result<(), PeerError> {
    if local_id == remote_id {
        return Err(PeerError::SelfConnection);
    }
    if local_role == NodeRole::Database && remote_role == NodeRole::Database {
        return Err(PeerError::IncompatibleRole);
    }
    Ok(())
}

/// Whether a new link to a peer replaces the existing one.
///
/// The link that the lexicographically smaller node id initiated is the
/// preferred one, so both sides of a simultaneous dial converge on the same
/// connection.
pub fn prefer_new_connection(local_id: NodeId, remote_id: NodeId, new_is_outbound: bool) -> bool {
    let local_initiates = local_id < remote_id;
    new_is_outbound == local_initiates
}

/// One registered peer link.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PeerSlot {
    /// The generation of the link. A newer generation replaces an older one,
    /// and the cleanup of an older link never removes a newer entry.
    pub generation: u64,
    /// Whether this node initiated the link.
    pub outbound: bool,
}

/// The outcome of a link admission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    /// The link is the current one to the peer.
    Accepted {
        /// The generation of the accepted link.
        generation: u64,
        /// The generation of the link this one replaces, if any.
        replaced: Option<u64>,
    },
}

/// The directory of the peer links of this node.
#[derive(Default)]
pub struct PeerDirectory {
    inner: Mutex<DirectoryState>,
}

#[derive(Default)]
struct DirectoryState {
    entries: HashMap<NodeId, PeerSlot>,
    next_generation: u64,
}

impl PeerDirectory {
    /// A new, empty directory.
    pub fn new() -> Self {
        Self::default()
    }

    /// Admit a link to a peer.
    ///
    /// The admitted link gets a fresh generation. When it replaces an older
    /// link, the older generation is reported so the caller can stop it; the
    /// cleanup of the older link then finds that the directory already holds
    /// the newer generation and leaves it alone.
    pub async fn admit(
        &self,
        local_id: NodeId,
        remote_id: NodeId,
        outbound: bool,
    ) -> Result<Admission, PeerError> {
        let mut state = self.inner.lock().await;
        if let Some(existing) = state.entries.get(&remote_id).copied() {
            if !prefer_new_connection(local_id, remote_id, outbound) {
                return Err(PeerError::DuplicateConnection);
            }
            state.next_generation += 1;
            let generation = state.next_generation;
            state.entries.insert(
                remote_id,
                PeerSlot {
                    generation,
                    outbound,
                },
            );
            return Ok(Admission::Accepted {
                generation,
                replaced: Some(existing.generation),
            });
        }
        if state.entries.len() >= MAX_PEERS_PER_NODE {
            return Err(PeerError::PeerCapReached);
        }
        state.next_generation += 1;
        let generation = state.next_generation;
        state.entries.insert(
            remote_id,
            PeerSlot {
                generation,
                outbound,
            },
        );
        Ok(Admission::Accepted {
            generation,
            replaced: None,
        })
    }

    /// Remove a link from the directory.
    ///
    /// The entry is only removed when it still belongs to the given
    /// generation: the cleanup of a replaced link must never erase the newer
    /// link that replaced it.
    pub async fn release(&self, remote_id: NodeId, generation: u64) {
        let mut state = self.inner.lock().await;
        let remove = state
            .entries
            .get(&remote_id)
            .is_some_and(|slot| slot.generation == generation);
        if remove {
            state.entries.remove(&remote_id);
        }
    }

    /// The generation of the current link to a peer, if any.
    #[cfg(test)]
    pub async fn generation(&self, remote_id: NodeId) -> Option<u64> {
        let state = self.inner.lock().await;
        state.entries.get(&remote_id).map(|slot| slot.generation)
    }

    /// How many peers this node serves.
    pub async fn len(&self) -> usize {
        let state = self.inner.lock().await;
        state.entries.len()
    }

    /// Whether this node serves no peer at all.
    #[cfg(test)]
    pub async fn is_empty(&self) -> bool {
        self.len().await == 0
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::{
        Admission, NodeRole, PeerDirectory, PeerError, check_pairing, prefer_new_connection,
    };
    use crate::worker::runtime::node::identity::NodeId;

    fn id(value: u128) -> NodeId {
        NodeId::from_uuid(Uuid::from_u128(value))
    }

    #[test]
    fn only_a_database_node_and_an_edge_node_form_a_star() {
        assert!(check_pairing(NodeRole::Database, id(1), NodeRole::Edge, id(2)).is_ok());
        assert!(check_pairing(NodeRole::Edge, id(1), NodeRole::Database, id(2)).is_ok());
        assert_eq!(
            check_pairing(NodeRole::Database, id(1), NodeRole::Database, id(2)),
            Err(PeerError::IncompatibleRole)
        );
        assert!(
            check_pairing(NodeRole::Edge, id(1), NodeRole::Edge, id(2)).is_ok(),
            "two database-less nodes may be linked"
        );
    }

    #[test]
    fn a_node_cannot_link_to_itself() {
        assert_eq!(
            check_pairing(NodeRole::Database, id(1), NodeRole::Edge, id(1)),
            Err(PeerError::SelfConnection)
        );
    }

    #[test]
    fn the_link_of_the_smaller_node_id_wins() {
        // The local node has the smaller id, so the link it initiated wins.
        assert!(prefer_new_connection(id(1), id(2), true));
        assert!(!prefer_new_connection(id(1), id(2), false));
        // The remote node has the smaller id, so the link it initiated wins.
        assert!(prefer_new_connection(id(2), id(1), false));
        assert!(!prefer_new_connection(id(2), id(1), true));
    }

    #[tokio::test]
    async fn a_replaced_link_gets_a_new_generation_and_the_old_cleanup_keeps_the_new_entry() {
        let directory = PeerDirectory::new();
        // The remote node owns the smaller identity, so the link it started
        // is the preferred one.
        let local = id(5);
        let remote = id(2);
        let first = directory
            .admit(local, remote, false)
            .await
            .expect("the link of the peer is admitted");
        let Admission::Accepted {
            generation: first_generation,
            replaced,
        } = first;
        assert_eq!(replaced, None);

        // The local node dials the peer as well: the link of the peer stays,
        // because the peer owns the smaller identity.
        assert_eq!(
            directory.admit(local, remote, true).await,
            Err(PeerError::DuplicateConnection)
        );

        // A new link of the peer replaces the older one.
        let second = directory
            .admit(local, remote, false)
            .await
            .expect("the replacing link is admitted");
        let Admission::Accepted {
            generation: second_generation,
            replaced,
        } = second;
        assert_eq!(replaced, Some(first_generation));
        assert_ne!(second_generation, first_generation);

        // The cleanup of the replaced link must not erase the newer entry.
        directory.release(remote, first_generation).await;
        assert_eq!(
            directory.generation(remote).await,
            Some(second_generation),
            "the newer link stays in the directory"
        );

        directory.release(remote, second_generation).await;
        assert!(directory.is_empty().await);
    }

    #[tokio::test]
    async fn the_peer_cap_is_enforced() {
        let directory = PeerDirectory::new();
        for index in 0..super::MAX_PEERS_PER_NODE {
            directory
                .admit(id(1), id(1000 + index as u128), false)
                .await
                .expect("the peer is admitted");
        }
        assert_eq!(directory.len().await, super::MAX_PEERS_PER_NODE);
        assert_eq!(
            directory.admit(id(1), id(9999), false).await,
            Err(PeerError::PeerCapReached)
        );
    }
}
