//! The peer link of a star.
//!
//! A node of a star links to its peers over the same listener it serves its
//! clients on. The link is authenticated with the mutual proof of the
//! handshake, and both sides may dial: the link that the lexicographically
//! smaller node id initiated is the one that stays.

mod accept;
mod auth;
mod dial;
mod edge;
mod handshake;
mod ownership;
mod session;
mod topology;
mod transport;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use tokio::sync::{Mutex, Semaphore, oneshot};
use tokio::task::JoinHandle;
use tracing::warn;

use crate::relay::AppState;
use crate::relay_secrets::RelaySecretManager;
use crate::worker::runtime::node::peer::topology::check_pinned_database;

use self::handshake::{Hello, PREAUTH_CONCURRENCY};

use self::topology::PeerDirectory;
use crate::worker::runtime::node::identity::NodeId;

pub(crate) use accept::{peer_challenge, peer_upgrade};
pub(crate) use auth::CHALLENGE_PATH;
pub(crate) use dial::{PeerTarget, run_peer_dialer};
pub(crate) use edge::run_edge_node;
pub(crate) use session::{DbPeerContext, run_db_peer_session, run_edge_peer_session};
pub(crate) use topology::{Admission, NodeRole};
pub(crate) use transport::{PeerReader, PeerWriter};

/// The context of the side that owns the link.
///
/// A database node executes the requests of its edges; an edge node forwards
/// the requests of its own clients to its database node.
#[derive(Clone)]
pub(crate) enum PeerRoleContext {
    /// The database side of a link.
    Database(Arc<DbPeerContext>),
    /// The edge side of a link.
    Edge(Arc<AppState>),
}

/// The identity and the capabilities of this node.
#[derive(Clone)]
pub(crate) struct NodeIdentity {
    /// The persistent identity of this node.
    pub node_id: NodeId,
    /// The role of this node inside its star.
    pub role: NodeRole,
    /// The master secret of this node, used as the proof primitive.
    pub manager: RelaySecretManager,
}

impl NodeIdentity {
    /// The hello of this node.
    pub fn hello(&self) -> Hello {
        Hello::new(
            self.node_id.as_uuid(),
            self.role == NodeRole::Database,
            env!("CARGO_PKG_VERSION"),
            crate::db::gate::NODE_PROTOCOL_VERSION,
        )
    }
}

/// The links this node owns, one per peer.
#[derive(Default)]
struct LinkSlot {
    /// The generation of the link.
    generation: u64,
    /// The stop signal of the link.
    stop: Option<oneshot::Sender<()>>,
}

/// Everything the peer links of this node share.
///
/// The context owns the identity of the node, the directory of its peers and
/// the semaphore that bounds the unauthenticated handshakes.
pub(crate) struct PeerLinkContext {
    /// The identity of this node.
    pub(crate) identity: NodeIdentity,
    /// The directory of the peer links.
    pub(crate) directory: Arc<PeerDirectory>,
    /// The role of this node.
    pub(crate) role: PeerRoleContext,
    /// The semaphore that bounds the unauthenticated handshakes.
    pub(crate) preauth: Arc<Semaphore>,
    /// The startup epoch of this process.
    ///
    /// A ticket of a previous process of this node is worthless, so a peer
    /// cannot use a ticket that it captured earlier.
    pub(crate) epoch: String,
    /// The tickets that were already used.
    pub(crate) replay: Arc<auth::ReplayCache>,
    /// How long a link waits for the peer before it closes it.
    pub(crate) heartbeat_timeout: Duration,
    links: Mutex<HashMap<NodeId, LinkSlot>>,
    /// The database node an edge node is pinned to.
    pinned: Mutex<Option<NodeId>>,
    /// The accepted peer sessions of this node.
    sessions: Mutex<Vec<JoinHandle<()>>>,
    /// Whether the node stopped its peer links.
    stopping: AtomicBool,
    active: AtomicUsize,
    stopped: tokio::sync::Notify,
}

impl PeerLinkContext {
    /// Build the peer context of this node.
    pub(crate) fn new(
        identity: NodeIdentity,
        role: PeerRoleContext,
        heartbeat_timeout: Duration,
    ) -> Arc<Self> {
        Arc::new(Self {
            identity,
            directory: Arc::new(PeerDirectory::new()),
            role,
            preauth: Arc::new(Semaphore::new(PREAUTH_CONCURRENCY)),
            epoch: auth::startup_epoch(),
            replay: Arc::new(auth::ReplayCache::new()),
            heartbeat_timeout,
            links: Mutex::new(HashMap::new()),
            pinned: Mutex::new(None),
            sessions: Mutex::new(Vec::new()),
            stopping: AtomicBool::new(false),
            active: AtomicUsize::new(0),
            stopped: tokio::sync::Notify::new(),
        })
    }

    /// Register an accepted peer session.
    ///
    /// The session is owned by the node, so the shutdown path can close it
    /// before the resources of the node are released.
    ///
    /// The registry only keeps the sessions that are still running: a session
    /// that ended is joined right away, so the registry stays proportional to
    /// the live links instead of growing with every connection the node ever
    /// accepted.
    pub(crate) async fn register_session(&self, session: JoinHandle<()>) {
        let mut sessions = self.sessions.lock().await;
        let mut running = Vec::with_capacity(sessions.len() + 1);
        let mut finished = Vec::new();
        for handle in sessions.drain(..) {
            if handle.is_finished() {
                finished.push(handle);
            } else {
                running.push(handle);
            }
        }
        running.push(session);
        *sessions = running;
        drop(sessions);
        for handle in finished {
            // The task is finished, so this returns immediately and observes
            // a panic of the ended session.
            if let Err(err) = handle.await {
                warn!(error = %err, "an accepted peer session ended with an error");
            }
        }
    }

    /// The number of admitted peer links.
    #[cfg(test)]
    pub(crate) fn active_count(&self) -> usize {
        self.active.load(Ordering::Acquire)
    }

    /// The number of accepted peer sessions the registry still holds.
    #[cfg(test)]
    pub(crate) async fn retained_session_count(&self) -> usize {
        self.sessions.lock().await.len()
    }

    /// Whether the node stopped its peer links.
    ///
    /// A handshake that is still running when the node stops must not start
    /// a session afterwards.
    pub(crate) fn is_stopping(&self) -> bool {
        self.stopping.load(Ordering::Acquire)
    }

    /// The group of this node.
    ///
    /// The group of a star is the identity of its database node: the
    /// database node is the group itself, and an edge node belongs to the
    /// group of the database node it is pinned to.
    #[cfg(test)]
    pub(crate) async fn group_id(&self) -> Option<NodeId> {
        match &self.role {
            PeerRoleContext::Database(_) => Some(self.identity.node_id),
            PeerRoleContext::Edge(_) => *self.pinned.lock().await,
        }
    }

    /// Stop and join every accepted peer session.
    pub(crate) async fn stop_sessions(&self) {
        self.stopping.store(true, Ordering::Release);
        {
            let mut links = self.links.lock().await;
            for slot in links.values_mut() {
                if let Some(stop) = slot.stop.take() {
                    let _ = stop.send(());
                }
            }
        }
        loop {
            let notified = self.stopped.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.active.load(Ordering::Acquire) == 0 {
                break;
            }
            notified.await;
        }
        let sessions = std::mem::take(&mut *self.sessions.lock().await);
        for session in sessions {
            let _ = session.await;
        }
    }

    /// Admit a link to a peer.
    ///
    /// The admitted link gets a fresh generation. When it replaces an older
    /// link, the older link is stopped: the star keeps one link per peer.
    async fn admit(
        &self,
        hello: &Hello,
        outbound: bool,
    ) -> anyhow::Result<(u64, oneshot::Receiver<()>)> {
        let remote = NodeId::from_uuid(hello.node_id);
        let mut links = self.links.lock().await;
        if self.is_stopping() {
            anyhow::bail!("the node stopped its peer links");
        }
        let admission = self
            .directory
            .admit(self.identity.node_id, remote, outbound)
            .await?;
        let Admission::Accepted { generation, .. } = admission;
        if let Some(existing) = links.get_mut(&remote)
            && existing.generation != generation
            && let Some(stop) = existing.stop.take()
        {
            // The replaced link is stopped, but its own cleanup can never
            // remove the newer entry: the directory only removes an entry
            // whose generation still matches.
            let _ = stop.send(());
        }
        let (stop, receiver) = oneshot::channel();
        links.insert(
            remote,
            LinkSlot {
                generation,
                stop: Some(stop),
            },
        );
        if let PeerRoleContext::Database(database) = &self.role
            && let Some(status) = &database.admin_state.node_status
        {
            let mut state = status.lock().unwrap_or_else(|e| e.into_inner());
            state.admit(
                generation,
                crate::worker_admin_types::NodePeerSummary {
                    node_id: hello.node_id,
                    has_database: hello.has_database,
                    runtime_version: hello.runtime_version.clone(),
                    protocol_version: hello.protocol_version.to_string(),
                    connected_at: Some(chrono::Utc::now()),
                },
            );
        }
        self.active.fetch_add(1, Ordering::Release);
        Ok((generation, receiver))
    }

    /// Release a link.
    ///
    /// The release only removes the entry when it still belongs to the given
    /// generation, so the cleanup of a replaced link never erases the link
    /// that replaced it.
    async fn release(&self, remote: NodeId, generation: u64) {
        let mut links = self.links.lock().await;
        self.directory.release(remote, generation).await;
        if links
            .get(&remote)
            .is_some_and(|slot| slot.generation == generation)
        {
            links.remove(&remote);
            if let PeerRoleContext::Database(database) = &self.role
                && let Some(status) = &database.admin_state.node_status
            {
                let mut state = status.lock().unwrap_or_else(|e| e.into_inner());
                state.release(remote.as_uuid(), generation);
            }
        }
        self.active.fetch_sub(1, Ordering::AcqRel);
        self.stopped.notify_waiters();
    }
}

/// Run an established peer link.
///
/// The link is admitted to the directory first, so the deduplication of a
/// simultaneous dial happens before any traffic is exchanged, and it is
/// released again when it ends.
pub(crate) async fn run_link<R: PeerReader + Send, W: PeerWriter + Send + 'static>(
    context: &Arc<PeerLinkContext>,
    reader: R,
    writer: W,
    remote_hello: Hello,
    outbound: bool,
    permit: Option<tokio::sync::OwnedSemaphorePermit>,
) -> anyhow::Result<()> {
    let remote = NodeId::from_uuid(remote_hello.node_id);
    // An edge node belongs to one database node for the life of the
    // process: a link to another database node is refused.
    if let PeerRoleContext::Edge(_) = &context.role {
        let mut pinned = context.pinned.lock().await;
        check_pinned_database(*pinned, remote)?;
        *pinned = Some(remote);
    }
    let (generation, stop) = context.admit(&remote_hello, outbound).await?;
    drop(permit);
    let outcome = match &context.role {
        PeerRoleContext::Database(database) => {
            run_db_peer_session(reader, writer, database.clone(), remote, generation, stop).await
        }
        PeerRoleContext::Edge(state) => {
            let worker_id = state.allocate_worker_id();
            run_edge_peer_session(
                reader,
                writer,
                state.as_ref().clone(),
                worker_id,
                context.heartbeat_timeout,
                stop,
            )
            .await
        }
    };
    context.release(remote, generation).await;
    outcome
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use uuid::Uuid;

    use super::{NodeIdentity, PeerLinkContext, PeerRoleContext};
    use crate::relay::test_state;
    use crate::relay_secrets::RelaySecretManager;
    use crate::worker::runtime::node::identity::NodeId;
    use crate::worker::runtime::node::peer::topology::NodeRole;

    #[tokio::test]
    async fn preferred_replacement_stops_old_session_without_removing_new_slot() {
        use super::handshake::Hello;
        use super::transport::MemoryPeer;
        let state = test_state();
        let context = PeerLinkContext::new(
            NodeIdentity {
                node_id: NodeId::from_uuid(Uuid::from_u128(1)),
                role: NodeRole::Edge,
                manager: RelaySecretManager::from_key_bytes([3; 32]),
            },
            PeerRoleContext::Edge(Arc::new(state.clone())),
            Duration::from_secs(90),
        );
        let hello = Hello::new(Uuid::from_u128(2), true, "1", 1);
        let (old_peer, old_local) = MemoryPeer::pair(16);
        let (old_reader, old_writer) = old_local.into_halves();
        let old_context = context.clone();
        let old_hello = hello.clone();
        let old = tokio::spawn(async move {
            super::run_link(&old_context, old_reader, old_writer, old_hello, false, None).await
        });
        tokio::time::timeout(Duration::from_secs(5), async {
            while state.inner.workers.lock().await.is_empty() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("old worker registered");
        let (new_peer, new_local) = MemoryPeer::pair(16);
        let (new_reader, new_writer) = new_local.into_halves();
        let new_context = context.clone();
        let new = tokio::spawn(async move {
            super::run_link(&new_context, new_reader, new_writer, hello, true, None).await
        });
        tokio::time::timeout(Duration::from_secs(5), old)
            .await
            .expect("old stopped")
            .expect("old joined")
            .expect("old clean");
        tokio::time::timeout(Duration::from_secs(5), async {
            while state.inner.workers.lock().await.len() != 1 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("only replacement registered");
        assert_eq!(context.directory.len().await, 1);
        context.stop_sessions().await;
        new.await.expect("new joined").expect("new clean");
        assert!(state.inner.workers.lock().await.is_empty());
        assert_eq!(context.directory.len().await, 0);
        drop((old_peer, new_peer));
    }

    #[tokio::test]
    async fn the_accepted_session_registry_stays_bounded_under_churn() {
        let context = PeerLinkContext::new(
            NodeIdentity {
                node_id: NodeId::from_uuid(Uuid::from_u128(21)),
                role: NodeRole::Edge,
                manager: RelaySecretManager::from_key_bytes([3; 32]),
            },
            PeerRoleContext::Edge(Arc::new(test_state())),
            Duration::from_secs(90),
        );

        // Every accepted session that ended is reaped by the next
        // registration, so the registry never keeps the whole history of the
        // connections of the node.
        for _ in 0..64 {
            let handle = tokio::spawn(async {});
            tokio::task::yield_now().await;
            context.register_session(handle).await;
        }
        assert!(
            context.retained_session_count().await <= 2,
            "the registry kept the sessions of every connection the node accepted"
        );

        // The shutdown joins the sessions that are still running.
        let (tx, rx) = tokio::sync::oneshot::channel();
        let handle = tokio::spawn(async move {
            let _ = rx.await;
        });
        context.register_session(handle).await;
        tx.send(()).expect("the session is released");
        context.stop_sessions().await;
        assert_eq!(context.retained_session_count().await, 0);
    }

    #[tokio::test]
    async fn the_group_of_an_edge_is_the_database_node_it_is_pinned_to() {
        let context = PeerLinkContext::new(
            NodeIdentity {
                node_id: NodeId::from_uuid(Uuid::from_u128(9)),
                role: NodeRole::Edge,
                manager: RelaySecretManager::from_key_bytes([3; 32]),
            },
            PeerRoleContext::Edge(Arc::new(test_state())),
            Duration::from_secs(90),
        );
        assert_eq!(
            context.group_id().await,
            None,
            "an edge without a link has no group yet"
        );
        let database = NodeId::from_uuid(Uuid::from_u128(4));
        *context.pinned.lock().await = Some(database);
        assert_eq!(
            context.group_id().await,
            Some(database),
            "the group of the star is the identity of its database node"
        );
    }
}
