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
mod forwarding;
mod handshake;
mod ownership;
mod route;
mod session;
mod topology;
mod transport;
mod wire;

/// The authority of one admitted peer link.
///
/// A token is minted once per admitted link, right after the directory
/// accepted the link. It stays live only while that link is the admitted
/// link of its peer: replacing the link, releasing it, or stopping the node
/// invalidates it. Every action of a peer session carries the token of its
/// link, so a delayed callback of a replaced link is refused instead of
/// resurrecting the state of the node.
pub(crate) mod admission {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use tokio::sync::{OwnedRwLockReadGuard, RwLock};

    use super::{Admission, NodeId, PeerLinkContext};

    /// The authority of one admitted peer link.
    ///
    /// The token is cloneable, so it travels with the session of the link,
    /// but its fields are private to this module: only the link context
    /// mints one, so neither the hub nor a session can authorize an action
    /// on its own.
    #[derive(Clone)]
    pub(crate) struct AdmissionToken {
        /// Whether the link of the token is still the admitted one.
        live: Arc<AtomicBool>,
        /// The fence of the publication of this admission.
        ///
        /// The fence serializes the invalidation of the admission against
        /// the local publication that belongs to it: a publication that is
        /// already admitted finishes before the invalidation linearizes, and
        /// no publication starts after it.
        publication: Arc<RwLock<()>>,
        /// The peer the admitted link belongs to.
        peer: NodeId,
        /// The generation of the link the token was minted for.
        generation: u64,
    }

    impl AdmissionToken {
        /// Whether the link of the token is still the admitted one.
        pub(crate) fn is_live(&self) -> bool {
            self.live.load(Ordering::Acquire)
        }

        /// The peer the admitted link belongs to.
        pub(crate) fn peer(&self) -> NodeId {
            self.peer
        }

        /// The generation of the link the token was minted for.
        pub(crate) fn generation(&self) -> u64 {
            self.generation
        }

        /// The read guard of the publication of this admission.
        ///
        /// The guard is the authority of a publication boundary: the caller
        /// acquires it first and checks the liveness of the admission
        /// second. The check before the guard would race with the
        /// invalidation of the admission.
        pub(crate) async fn read_publication(&self) -> OwnedRwLockReadGuard<()> {
            self.publication.clone().read_owned().await
        }

        /// The read guard of the publication, without waiting.
        ///
        /// The egress of a peer uses it: a frame is only queued while the
        /// admission of its link is still valid, and the peer is never
        /// waited for.
        pub(crate) fn try_read_publication(&self) -> Option<OwnedRwLockReadGuard<()>> {
            self.publication.clone().try_read_owned().ok()
        }

        /// Invalidate the token, so every later action of its link is
        /// refused.
        ///
        /// The invalidation is exclusive: it waits for the publications that
        /// are already admitted, so none of them can publish after the
        /// admission died.
        pub(crate) async fn invalidate_exclusive(&self) {
            let _guard = self.publication.clone().write_owned().await;
            self.live.store(false, Ordering::Release);
        }

        /// Mark the token dead without waiting.
        ///
        /// Only the actor of the hub uses it: the actor is the sole owner of
        /// the state of the node, so it never waits for the fence of its own
        /// publication.
        pub(crate) fn mark_dead(&self) {
            self.live.store(false, Ordering::Release);
        }
    }

    /// Mint the token of an admitted link.
    ///
    /// Only the link context mints a token, and only for a link the
    /// directory accepted, so no other part of the node can authorize a hub
    /// action on its own. The generation of the token is the generation of
    /// the admission, so a token never claims a link the directory refused.
    pub(super) fn mint(
        context: &PeerLinkContext,
        admission: &Admission,
        peer: NodeId,
    ) -> AdmissionToken {
        debug_assert!(!context.is_stopping());
        let Admission::Accepted { generation, .. } = *admission;
        AdmissionToken {
            live: Arc::new(AtomicBool::new(true)),
            publication: Arc::new(RwLock::new(())),
            peer,
            generation,
        }
    }

    /// Mint a token without a link context.
    ///
    /// Only the tests of the peer module use it, so the production code
    /// never authorizes a hub action without an admitted link.
    #[cfg(test)]
    pub(in crate::worker::runtime::node::peer) fn mint_for_test(
        peer: NodeId,
        generation: u64,
    ) -> AdmissionToken {
        AdmissionToken {
            live: Arc::new(AtomicBool::new(true)),
            publication: Arc::new(RwLock::new(())),
            peer,
            generation,
        }
    }
}

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use tokio::sync::{Mutex, Semaphore, oneshot};
use tokio::task::JoinHandle;
use tracing::warn;

use crate::relay::AppState;
use crate::relay_secrets::RelaySecretManager;

use self::forwarding::RouteHub;
use self::handshake::{Hello, PREAUTH_CONCURRENCY};
use self::wire::PEER_PROTOCOL_VERSION;

use self::topology::PeerDirectory;
use crate::worker::runtime::node::identity::NodeId;

pub(crate) use accept::{peer_challenge, peer_upgrade};
pub(crate) use auth::CHALLENGE_PATH;
pub(crate) use dial::{PeerTarget, run_peer_dialer};
pub(crate) use edge::run_edge_node;
pub(crate) use session::{DbPeerContext, run_db_peer_session, run_node_peer_session};
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
            PEER_PROTOCOL_VERSION,
        )
    }
}

/// The links this node owns, one per peer.
struct LinkSlot {
    /// The generation of the link.
    generation: u64,
    /// The stop signal of the link.
    stop: Option<oneshot::Sender<()>>,
    /// The authority of the link.
    ///
    /// The token is invalidated when the link is replaced, when it is
    /// released, and when the node stops, so a callback of a link that is no
    /// longer admitted cannot act on the state of the node.
    token: admission::AdmissionToken,
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
    /// The forwarding hub of a database-less node.
    ///
    /// The hub owns the route of the node and the owners of the requests that
    /// travel over it. A database node has no hub: it is the end of every
    /// path.
    hub: Option<Arc<RouteHub>>,
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
        let hub = match &role {
            PeerRoleContext::Database(_) => None,
            PeerRoleContext::Edge(state) => {
                Some(RouteHub::new(identity.node_id, state.as_ref().clone()))
            }
        };
        Arc::new(Self {
            identity,
            directory: Arc::new(PeerDirectory::new()),
            hub,
            role,
            preauth: Arc::new(Semaphore::new(PREAUTH_CONCURRENCY)),
            epoch: auth::startup_epoch(),
            replay: Arc::new(auth::ReplayCache::new()),
            heartbeat_timeout,
            links: Mutex::new(HashMap::new()),
            sessions: Mutex::new(Vec::new()),
            stopping: AtomicBool::new(false),
            active: AtomicUsize::new(0),
            stopped: tokio::sync::Notify::new(),
        })
    }

    /// The forwarding hub of this node, if it has one.
    pub(crate) fn hub(&self) -> Option<&Arc<RouteHub>> {
        self.hub.as_ref()
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
    /// database node is the group itself, and a database-less node belongs to
    /// the group of the database node it is pinned to.
    #[cfg(test)]
    pub(crate) async fn group_id(&self) -> Option<NodeId> {
        match &self.role {
            PeerRoleContext::Database(_) => Some(self.identity.node_id),
            PeerRoleContext::Edge(_) => match &self.hub {
                Some(hub) => hub.terminal().await,
                None => None,
            },
        }
    }

    /// Stop and join every accepted peer session.
    pub(crate) async fn stop_sessions(&self) {
        self.stopping.store(true, Ordering::Release);
        {
            let mut links = self.links.lock().await;
            for slot in links.values_mut() {
                // The node stops: no link of it stays admitted, so every
                // later action of a session is refused. The invalidation is
                // exclusive, so a publication that is already admitted
                // finishes before the node stops its links.
                slot.token.invalidate_exclusive().await;
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
        // The actor of the hub owns the forwarding state of the node, so it
        // stops after the peer sessions of the node are gone.
        if let Some(hub) = &self.hub {
            hub.stop().await;
        }
    }

    /// Admit a link to a peer.
    ///
    /// The admitted link gets a fresh generation and the authority of that
    /// link. When it replaces an older link, the older link loses its
    /// authority before it is stopped: the star keeps one link per peer, and
    /// a delayed callback of the replaced link cannot act anymore.
    async fn admit(
        &self,
        hello: &Hello,
        outbound: bool,
    ) -> anyhow::Result<(u64, oneshot::Receiver<()>, admission::AdmissionToken)> {
        let remote = NodeId::from_uuid(hello.node_id);
        let mut links = self.links.lock().await;
        if self.is_stopping() {
            anyhow::bail!("the node stopped its peer links");
        }
        let admission = self
            .directory
            .admit(self.identity.node_id, remote, outbound)
            .await?;
        // The token is minted only here, after the directory accepted the
        // link, so an admitted link is the only thing that can authorize an
        // action of the hub.
        let token = admission::mint(self, &admission, remote);
        let Admission::Accepted { generation, .. } = admission;
        if let Some(existing) = links.get_mut(&remote) {
            if existing.generation != generation {
                // The replaced link loses its authority before it is told to
                // stop, so a callback that is already queued cannot publish
                // a route or cancel the replacement. The invalidation is
                // exclusive: the publication that is already admitted
                // finishes before the replacement linearizes.
                existing.token.invalidate_exclusive().await;
                if let Some(stop) = existing.stop.take() {
                    // The replaced link is stopped, but its own cleanup can
                    // never remove the newer entry: the directory only
                    // removes an entry whose generation still matches.
                    let _ = stop.send(());
                }
            }
        }
        let (stop, receiver) = oneshot::channel();
        links.insert(
            remote,
            LinkSlot {
                generation,
                stop: Some(stop),
                token: token.clone(),
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
        Ok((generation, receiver, token))
    }

    /// Release a link.
    ///
    /// The release only removes the entry when it still belongs to the given
    /// generation, so the cleanup of a replaced link never erases the link
    /// that replaced it. The authority of the released generation dies with
    /// it, so a delayed callback of that link cannot act anymore.
    async fn release(&self, remote: NodeId, generation: u64) {
        let mut links = self.links.lock().await;
        self.directory.release(remote, generation).await;
        if links
            .get(&remote)
            .is_some_and(|slot| slot.generation == generation)
        {
            // The released generation dies exclusively before its slot is
            // removed, so a publication that is already admitted finishes
            // before the release linearizes and no publication of it starts
            // afterwards.
            let token = links
                .get(&remote)
                .map(|slot| slot.token.clone())
                .expect("the slot of the released generation exists");
            token.invalidate_exclusive().await;
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
    let (generation, stop, token) = context.admit(&remote_hello, outbound).await?;
    drop(permit);
    let outcome = match &context.role {
        PeerRoleContext::Database(database) => {
            run_db_peer_session(reader, writer, database.clone(), remote, generation, stop).await
        }
        PeerRoleContext::Edge(_) => {
            let hub = context
                .hub()
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("the node has no forwarding hub"))?;
            run_node_peer_session(reader, writer, hub, token, context.heartbeat_timeout, stop).await
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
            while context.directory.len().await == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the old link is admitted");
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
            while context.directory.len().await != 1 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("only the replacement is admitted");
        assert_eq!(context.directory.len().await, 1);
        context.stop_sessions().await;
        new.await.expect("new joined").expect("new clean");
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
    async fn the_group_of_a_database_less_node_is_the_database_node_it_is_pinned_to() {
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
            "a node without a route has no group yet"
        );
        let hub = context.hub().cloned().expect("the node has a hub");
        let peer = NodeId::from_uuid(Uuid::from_u128(5));
        let database = NodeId::from_uuid(Uuid::from_u128(4));
        let (tx, _rx) = crate::worker::runtime::node::peer::forwarding::envelope_channel();
        let (shutdown, _shutdown_rx) = tokio::sync::watch::channel(false);
        let token = super::admission::mint_for_test(peer, 1);
        hub.register_link(&token, tx, shutdown).await;
        let advertised =
            crate::worker::runtime::node::peer::wire::PeerPath::new(vec![peer, database])
                .expect("a bounded path");
        hub.apply_update(
            &token,
            1,
            &advertised,
            crate::protocol::ConfigSnapshot {
                version: 1,
                keys: vec![],
                relay_ip_policy: Default::default(),
            },
        )
        .await
        .expect("the route is applied");
        assert_eq!(
            context.group_id().await,
            Some(database),
            "the group of the star is the identity of its database node"
        );
    }
}
