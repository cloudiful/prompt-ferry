//! The route state of a node.
//!
//! A node learns one route: the path from itself to the database node that
//! executes its traffic. Every live authenticated link may advertise one
//! candidate, the shortest candidate wins, and the database node of the
//! first valid route is pinned for the life of the process.
//!
//! A candidate may only serve the node when it carries a configuration that
//! is at least as new as the highest one the node ever validated for the
//! pinned database node. That watermark lives for the life of the process,
//! so a route that still carries an older configuration never takes over
//! from a route that already served a newer one: a revoked managed key does
//! not come back through a stale alternate.
//!
//! The state is pure: it holds no lock and awaits nothing, so a reader
//! always sees a whole route decision.

use std::collections::HashMap;
use std::fmt;

use crate::protocol::ConfigSnapshot;
use crate::worker::runtime::node::identity::NodeId;

use super::wire::{MAX_PEER_ROUTE_NODES, PeerPath, PeerPathError};

/// How many candidates a node holds.
///
/// The cap is the cap of the peers of a node: every candidate belongs to a
/// peer, so a node never holds more candidates than peers.
pub const MAX_ROUTE_CANDIDATES: usize = crate::relay::MAX_NODE_PEERS;

/// Why a route decision was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteError {
    /// The advertised path holds more nodes than a bounded route may hold.
    PathTooLong,
    /// The advertised path holds no node at all.
    EmptyPath,
    /// The advertised path names the same node twice.
    RepeatedNode,
    /// The advertised path names this node, so it is not a route to another
    /// node.
    ContainsLocal,
    /// The advertised path does not start at the peer that advertised it.
    NotAdjacent,
    /// The route belongs to another database node than the pinned one.
    WrongTerminal,
    /// The advertisement belongs to a link that was already replaced.
    StaleGeneration,
    /// The advertisement repeats a revision that was already applied.
    StaleRevision,
    /// The node already holds as many candidates as it may hold.
    CandidateCapReached,
    /// The node has no route.
    NoRoute,
    /// The path does not match the selected route.
    PathMismatch,
}

impl fmt::Display for RouteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PathTooLong => write!(f, "a route holds at most {MAX_PEER_ROUTE_NODES} nodes"),
            Self::EmptyPath => f.write_str("a path holds no node at all"),
            Self::RepeatedNode => f.write_str("a path names the same node twice"),
            Self::ContainsLocal => f.write_str("a path names this node"),
            Self::NotAdjacent => {
                f.write_str("the path does not start at the peer that advertised it")
            }
            Self::WrongTerminal => {
                f.write_str("the route belongs to another database node than the pinned one")
            }
            Self::StaleGeneration => {
                f.write_str("the advertisement belongs to a link that was already replaced")
            }
            Self::StaleRevision => {
                f.write_str("the advertisement repeats a revision that was already applied")
            }
            Self::CandidateCapReached => write!(
                f,
                "the node already holds {MAX_ROUTE_CANDIDATES} route candidates"
            ),
            Self::NoRoute => f.write_str("the node has no route"),
            Self::PathMismatch => f.write_str("the path does not match the selected route"),
        }
    }
}

impl std::error::Error for RouteError {}

/// What an advertisement did to the selected route.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteOutcome {
    /// The selected route changed, so the epoch moved.
    Changed,
    /// The selected route stayed and only its snapshot was refreshed.
    SnapshotRefreshed,
    /// The selection did not change.
    Unchanged,
}

/// The state of a candidate of a peer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CandidateState {
    /// The peer advertised the route and it may be used.
    Live,
    /// The peer withdrew the route.
    ///
    /// A newer advertisement of the same link may restore it, because the
    /// peer may advertise its route again.
    Withdrawn,
    /// The link of the peer ended.
    ///
    /// The generation of that link is dead for good, so no advertisement of
    /// it may restore the route.
    LinkEnded,
}

/// One advertised candidate of a peer.
///
/// A candidate that is not live stays as a tombstone: it keeps the
/// generation and the revision of the link it arrived on, so an
/// advertisement of a withdrawn or ended link can never restore a route.
/// It keeps no snapshot, so a tombstone costs no memory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteCandidate {
    /// The peer that advertised the candidate.
    pub source: NodeId,
    /// The generation of the link the candidate arrived on.
    pub link_generation: u64,
    /// The generation of the advertisement within the generation of the link.
    pub generation: u64,
    /// The local path of the candidate, this node first.
    pub path: PeerPath,
    /// The database node the candidate leads to.
    pub terminal: NodeId,
    /// The snapshot the database node published.
    pub snapshot: Option<ConfigSnapshot>,
    /// The state of the candidate.
    pub state: CandidateState,
}

/// The route a node selected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectedRoute {
    /// The epoch of the selection.
    pub epoch: u64,
    /// The local path of the route, this node first.
    pub path: PeerPath,
    /// The peer the traffic of this node travels to.
    pub next_hop: NodeId,
    /// The generation of the link to the next hop.
    pub next_link_generation: u64,
    /// The database node the route leads to.
    pub terminal: NodeId,
    /// The snapshot the database node published.
    pub snapshot: ConfigSnapshot,
}

/// The route state of a node.
#[derive(Clone, Debug)]
pub struct RouteState {
    /// The identity of this node.
    local: NodeId,
    /// The database node this node is pinned to.
    terminal: Option<NodeId>,
    /// The highest configuration version this node validated for the pinned
    /// database node.
    ///
    /// The watermark is the freshness floor of the route: only a candidate
    /// that carries this version or a newer one may serve the node. It
    /// advances when a validated advertisement arrives and never falls back,
    /// not even while the node has no route at all, so a stale alternate
    /// cannot roll the configuration of the node back.
    watermark: Option<i64>,
    /// The candidates of the peers, one per peer.
    candidates: HashMap<NodeId, RouteCandidate>,
    /// The selected route.
    selected: Option<SelectedRoute>,
    /// The epoch of the selection.
    epoch: u64,
}

impl RouteState {
    /// An empty route state of a node.
    pub fn new(local: NodeId) -> Self {
        Self {
            local,
            terminal: None,
            watermark: None,
            candidates: HashMap::new(),
            selected: None,
            epoch: 0,
        }
    }

    /// The identity of this node.
    pub fn local(&self) -> NodeId {
        self.local
    }

    /// The candidate of a peer, if the node holds one.
    pub fn candidate(&self, source: NodeId) -> Option<&RouteCandidate> {
        self.candidates.get(&source)
    }

    /// The database node this node is pinned to.
    ///
    /// The pin survives a withdrawal, so a node never binds to another
    /// database node while it runs.
    pub fn terminal(&self) -> Option<NodeId> {
        self.terminal
    }

    /// The selected route, if the node has one.
    pub fn selected(&self) -> Option<&SelectedRoute> {
        self.selected.as_ref()
    }

    /// The epoch of the selection.
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// How many candidates the node holds.
    pub fn candidate_count(&self) -> usize {
        self.candidates.len()
    }

    /// Apply a route advertisement of a peer.
    ///
    /// The advertised path excludes this node and starts at the peer that
    /// advertised it, so the local path is this node followed by the
    /// advertised path.
    pub fn apply_update(
        &mut self,
        source: NodeId,
        link_generation: u64,
        generation: u64,
        advertised: &PeerPath,
        snapshot: ConfigSnapshot,
    ) -> Result<RouteOutcome, RouteError> {
        self.check_fence(source, link_generation, generation)?;
        if advertised.is_empty() {
            return Err(RouteError::EmptyPath);
        }
        if advertised.len() + 1 > MAX_PEER_ROUTE_NODES {
            return Err(RouteError::PathTooLong);
        }
        if advertised.nodes()[0] != source {
            return Err(RouteError::NotAdjacent);
        }
        if advertised.contains(self.local) {
            return Err(RouteError::ContainsLocal);
        }
        let terminal = advertised.nodes()[advertised.len() - 1];
        if let Some(pinned) = self.terminal
            && pinned != terminal
        {
            return Err(RouteError::WrongTerminal);
        }
        if !self.candidates.contains_key(&source) && self.candidates.len() >= MAX_ROUTE_CANDIDATES {
            // The store is bounded, so a new peer makes room by dropping the
            // tombstone of the oldest ended link. A live candidate is never
            // dropped, so the store only refuses a peer when the node
            // really serves the maximum number of peers.
            if !self.evict_oldest_tombstone() {
                return Err(RouteError::CandidateCapReached);
            }
        }
        let mut nodes = Vec::with_capacity(advertised.len() + 1);
        nodes.push(self.local);
        nodes.extend_from_slice(advertised.nodes());
        let path = PeerPath::new(nodes).map_err(|err| match err {
            PeerPathError::TooLong => RouteError::PathTooLong,
            PeerPathError::RepeatedNode => RouteError::RepeatedNode,
        })?;
        self.terminal = Some(terminal);
        // The advertisement passed every check, so the configuration it
        // carries is the newest one this node ever validated for the pinned
        // database node. The watermark only advances: a route that still
        // carries an older configuration never serves the node again.
        self.watermark = Some(match self.watermark {
            Some(mark) => mark.max(snapshot.version),
            None => snapshot.version,
        });
        self.candidates.insert(
            source,
            RouteCandidate {
                source,
                link_generation,
                generation,
                path,
                terminal,
                snapshot: Some(snapshot),
                state: CandidateState::Live,
            },
        );
        Ok(self.reselect())
    }

    /// Apply the withdrawal of the route of a peer.
    ///
    /// The withdrawal removes the candidate of the peer, but it never
    /// unbinds the pinned database node. The withdrawn candidate stays as a
    /// tombstone, so an advertisement of the withdrawn link cannot restore
    /// it without a newer revision.
    pub fn apply_withdraw(
        &mut self,
        source: NodeId,
        link_generation: u64,
        generation: u64,
    ) -> Result<RouteOutcome, RouteError> {
        let Some(existing) = self.candidates.get_mut(&source) else {
            return Ok(RouteOutcome::Unchanged);
        };
        if link_generation < existing.link_generation {
            return Err(RouteError::StaleGeneration);
        }
        if link_generation == existing.link_generation && generation <= existing.generation {
            return Err(RouteError::StaleRevision);
        }
        existing.link_generation = link_generation;
        existing.generation = generation;
        existing.snapshot = None;
        existing.state = CandidateState::Withdrawn;
        Ok(self.reselect())
    }

    /// Release the candidate of a link that ended.
    ///
    /// Only the candidate of the released generation is ended, so the
    /// cleanup of a replaced link never ends the candidate of the link that
    /// replaced it.
    pub fn release_link(&mut self, source: NodeId, link_generation: u64) -> RouteOutcome {
        let Some(existing) = self.candidates.get_mut(&source) else {
            return RouteOutcome::Unchanged;
        };
        if existing.link_generation != link_generation {
            return RouteOutcome::Unchanged;
        }
        existing.snapshot = None;
        existing.state = CandidateState::LinkEnded;
        self.reselect()
    }

    /// Whether the candidate carries the current configuration of the
    /// database node.
    ///
    /// A candidate is current when it carries the highest configuration
    /// version this node ever validated, or a newer one. A candidate without
    /// a configuration never serves the node.
    fn candidate_is_current(&self, candidate: &RouteCandidate) -> bool {
        let Some(snapshot) = candidate.snapshot.as_ref() else {
            return false;
        };
        match self.watermark {
            Some(mark) => snapshot.version >= mark,
            None => true,
        }
    }

    /// Check that an advertisement is newer than the one of its peer.
    fn check_fence(
        &self,
        source: NodeId,
        link_generation: u64,
        generation: u64,
    ) -> Result<(), RouteError> {
        let Some(existing) = self.candidates.get(&source) else {
            return Ok(());
        };
        if link_generation < existing.link_generation {
            return Err(RouteError::StaleGeneration);
        }
        if link_generation == existing.link_generation {
            if existing.state == CandidateState::LinkEnded {
                // The generation of the link is dead for good, so no
                // advertisement of it may restore the route.
                return Err(RouteError::StaleGeneration);
            }
            if generation <= existing.generation {
                return Err(RouteError::StaleRevision);
            }
        }
        Ok(())
    }

    /// Drop the tombstone of the oldest ended link.
    ///
    /// A tombstone only guards the fence of a link that is already gone, so
    /// the oldest one is the safest to drop. The store reports that it
    /// dropped nothing when every candidate is still live.
    fn evict_oldest_tombstone(&mut self) -> bool {
        let oldest = self
            .candidates
            .values()
            .filter(|candidate| candidate.state != CandidateState::Live)
            .min_by(|left, right| {
                left.link_generation
                    .cmp(&right.link_generation)
                    .then_with(|| left.source.cmp(&right.source))
            })
            .map(|candidate| candidate.source);
        match oldest {
            Some(source) => {
                self.candidates.remove(&source);
                true
            }
            None => false,
        }
    }

    /// Select the shortest candidate and report what changed.
    ///
    /// The selection is stable: candidates of the same length are ordered by
    /// the identity of their peer, so the same set of candidates always
    /// selects the same route.
    ///
    /// Only a candidate that carries the current configuration of the
    /// database node is eligible: freshness outranks the distance of the
    /// route. A stale candidate stays a candidate, so the node can use it
    /// again as soon as its peer advertises a current configuration, but it
    /// never serves the node and never advertises an older configuration.
    fn reselect(&mut self) -> RouteOutcome {
        let mut best: Option<&RouteCandidate> = None;
        for candidate in self.candidates.values() {
            if candidate.state != CandidateState::Live {
                continue;
            }
            if !self.candidate_is_current(candidate) {
                continue;
            }
            let shorter = match best {
                None => true,
                Some(current) => {
                    (candidate.path.len(), candidate.source) < (current.path.len(), current.source)
                }
            };
            if shorter {
                best = Some(candidate);
            }
        }
        let Some(best) = best else {
            if self.selected.is_some() {
                self.selected = None;
                self.epoch += 1;
                return RouteOutcome::Changed;
            }
            return RouteOutcome::Unchanged;
        };
        let Some(snapshot) = best.snapshot.clone() else {
            return RouteOutcome::Unchanged;
        };
        let next_hop = best.path.nodes()[1];
        let same = self.selected.as_ref().is_some_and(|current| {
            current.path == best.path
                && current.next_hop == next_hop
                && current.next_link_generation == best.link_generation
        });
        if same {
            let current = self.selected.as_ref().expect("the selection exists");
            if current.snapshot == snapshot {
                return RouteOutcome::Unchanged;
            }
            let epoch = current.epoch;
            self.selected = Some(SelectedRoute {
                epoch,
                path: best.path.clone(),
                next_hop,
                next_link_generation: best.link_generation,
                terminal: best.terminal,
                snapshot,
            });
            return RouteOutcome::SnapshotRefreshed;
        }
        self.epoch += 1;
        let epoch = self.epoch;
        self.selected = Some(SelectedRoute {
            epoch,
            path: best.path.clone(),
            next_hop,
            next_link_generation: best.link_generation,
            terminal: best.terminal,
            snapshot,
        });
        RouteOutcome::Changed
    }

    /// Check the path of a request that arrived from a peer.
    ///
    /// The path is the full path from the origin of the request to the
    /// database node. It must name this node exactly once, the node before
    /// this node must be the peer the request arrived from, the end of the
    /// path must be the pinned database node, and the part of the path that
    /// starts at this node must be the selected route.
    pub fn accept_start_path(&self, path: &PeerPath, incoming: NodeId) -> Result<(), RouteError> {
        let Some(current) = &self.selected else {
            return Err(RouteError::NoRoute);
        };
        let Some(pinned) = self.terminal else {
            return Err(RouteError::NoRoute);
        };
        let position = path.position(self.local).ok_or(RouteError::ContainsLocal)?;
        if path
            .nodes()
            .iter()
            .filter(|node| **node == self.local)
            .count()
            != 1
        {
            return Err(RouteError::RepeatedNode);
        }
        if position == 0 {
            return Err(RouteError::NotAdjacent);
        }
        if path.nodes()[position - 1] != incoming {
            return Err(RouteError::NotAdjacent);
        }
        let terminal = path.nodes()[path.len() - 1];
        if terminal != pinned {
            return Err(RouteError::WrongTerminal);
        }
        let suffix = path.suffix(position).ok_or(RouteError::PathMismatch)?;
        if suffix != current.path.nodes() {
            return Err(RouteError::PathMismatch);
        }
        Ok(())
    }

    /// Check the path of a request that arrived at the database node.
    ///
    /// The database node is the end of every path, so it only checks that
    /// the path ends at itself and that the peer before it is the peer the
    /// request arrived from.
    pub fn accept_terminal_path(
        &self,
        path: &PeerPath,
        incoming: NodeId,
    ) -> Result<(), RouteError> {
        if path.is_empty() {
            return Err(RouteError::EmptyPath);
        }
        let terminal = path.nodes()[path.len() - 1];
        if terminal != self.local {
            return Err(RouteError::WrongTerminal);
        }
        if path.len() > 1 && path.nodes()[path.len() - 2] != incoming {
            return Err(RouteError::NotAdjacent);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::{MAX_ROUTE_CANDIDATES, RouteError, RouteOutcome, RouteState};
    use crate::protocol::{ClientRoute, ConfigSnapshot};
    use crate::worker::runtime::node::identity::NodeId;
    use crate::worker::runtime::node::peer::wire::PeerPath;

    fn id(value: u128) -> NodeId {
        NodeId::from_uuid(Uuid::from_u128(value))
    }

    fn path(nodes: Vec<NodeId>) -> PeerPath {
        PeerPath::new(nodes).expect("a bounded path")
    }

    fn snapshot(version: i64) -> ConfigSnapshot {
        ConfigSnapshot {
            version,
            keys: vec![ClientRoute {
                key_hash: "hash".to_string(),
                key_prefix: "prefix".to_string(),
                user_id: 1,
                route_id: "route".to_string(),
            }],
            relay_ip_policy: Default::default(),
        }
    }

    const LOCAL: u128 = 1;
    const MAC: u128 = 2;
    const NAS: u128 = 3;
    const OTHER: u128 = 4;
    const OTHER_DB: u128 = 5;

    #[test]
    fn a_direct_route_is_selected() {
        let mut state = RouteState::new(id(LOCAL));
        let outcome = state
            .apply_update(id(NAS), 1, 1, &path(vec![id(NAS)]), snapshot(1))
            .expect("the advertisement is accepted");
        assert_eq!(outcome, RouteOutcome::Changed);
        let selected = state.selected().expect("the route is selected");
        assert_eq!(selected.path.nodes(), &[id(LOCAL), id(NAS)]);
        assert_eq!(selected.next_hop, id(NAS));
        assert_eq!(selected.terminal, id(NAS));
        assert_eq!(selected.epoch, 1);
        assert_eq!(state.terminal(), Some(id(NAS)));
    }

    #[test]
    fn a_two_hop_route_is_selected() {
        let mut state = RouteState::new(id(LOCAL));
        state
            .apply_update(id(MAC), 1, 1, &path(vec![id(MAC), id(NAS)]), snapshot(1))
            .expect("the advertisement is accepted");
        let selected = state.selected().expect("the route is selected");
        assert_eq!(selected.path.nodes(), &[id(LOCAL), id(MAC), id(NAS)]);
        assert_eq!(selected.next_hop, id(MAC));
    }

    #[test]
    fn the_shortest_candidate_wins() {
        let mut state = RouteState::new(id(LOCAL));
        state
            .apply_update(id(MAC), 1, 1, &path(vec![id(MAC), id(NAS)]), snapshot(1))
            .expect("the two hop route is accepted");
        state
            .apply_update(id(NAS), 1, 1, &path(vec![id(NAS)]), snapshot(1))
            .expect("the direct route is accepted");
        let selected = state.selected().expect("a route is selected");
        assert_eq!(selected.path.nodes(), &[id(LOCAL), id(NAS)]);
        assert_eq!(state.candidate_count(), 2);
    }

    #[test]
    fn equal_candidates_select_the_same_route_every_time() {
        let mut first = RouteState::new(id(LOCAL));
        let mut second = RouteState::new(id(LOCAL));
        for state in [&mut first, &mut second] {
            state
                .apply_update(id(MAC), 1, 1, &path(vec![id(MAC), id(NAS)]), snapshot(1))
                .expect("the route is accepted");
            state
                .apply_update(
                    id(OTHER),
                    1,
                    1,
                    &path(vec![id(OTHER), id(NAS)]),
                    snapshot(1),
                )
                .expect("the route is accepted");
        }
        assert_eq!(
            first.selected().map(|route| route.next_hop),
            second.selected().map(|route| route.next_hop)
        );
        let selected = first.selected().expect("a route is selected");
        assert_eq!(selected.next_hop, id(MAC));
    }

    #[test]
    fn an_advertisement_that_does_not_start_at_its_peer_is_refused() {
        let mut state = RouteState::new(id(LOCAL));
        let err = state
            .apply_update(id(MAC), 1, 1, &path(vec![id(NAS)]), snapshot(1))
            .expect_err("the advertisement is refused");
        assert_eq!(err, RouteError::NotAdjacent);
    }

    #[test]
    fn an_advertisement_that_names_this_node_is_refused() {
        let mut state = RouteState::new(id(LOCAL));
        let err = state
            .apply_update(id(MAC), 1, 1, &path(vec![id(MAC), id(LOCAL)]), snapshot(1))
            .expect_err("the advertisement is refused");
        assert_eq!(err, RouteError::ContainsLocal);
    }

    #[test]
    fn an_advertisement_with_too_many_nodes_is_refused() {
        let mut state = RouteState::new(id(LOCAL));
        let advertised =
            PeerPath::new(vec![id(MAC), id(OTHER), id(NAS)]).expect("three nodes fit the wire");
        let err = state
            .apply_update(id(MAC), 1, 1, &advertised, snapshot(1))
            .expect_err("the advertisement is refused");
        assert_eq!(err, RouteError::PathTooLong);
    }

    #[test]
    fn a_route_to_another_database_node_is_refused() {
        let mut state = RouteState::new(id(LOCAL));
        state
            .apply_update(id(NAS), 1, 1, &path(vec![id(NAS)]), snapshot(1))
            .expect("the first route is accepted");
        let err = state
            .apply_update(
                id(OTHER),
                1,
                1,
                &path(vec![id(OTHER), id(OTHER_DB)]),
                snapshot(1),
            )
            .expect_err("the second database node is refused");
        assert_eq!(err, RouteError::WrongTerminal);
        assert_eq!(state.terminal(), Some(id(NAS)));
    }

    #[test]
    fn a_stale_revision_is_refused() {
        let mut state = RouteState::new(id(LOCAL));
        state
            .apply_update(id(NAS), 1, 5, &path(vec![id(NAS)]), snapshot(1))
            .expect("the advertisement is accepted");
        let err = state
            .apply_update(id(NAS), 1, 5, &path(vec![id(NAS)]), snapshot(2))
            .expect_err("the repeated revision is refused");
        assert_eq!(err, RouteError::StaleRevision);
        let err = state
            .apply_update(id(NAS), 1, 4, &path(vec![id(NAS)]), snapshot(2))
            .expect_err("the older revision is refused");
        assert_eq!(err, RouteError::StaleRevision);
    }

    #[test]
    fn an_advertisement_of_a_replaced_link_is_refused() {
        let mut state = RouteState::new(id(LOCAL));
        state
            .apply_update(id(NAS), 4, 1, &path(vec![id(NAS)]), snapshot(1))
            .expect("the advertisement is accepted");
        let err = state
            .apply_update(id(NAS), 3, 9, &path(vec![id(NAS)]), snapshot(2))
            .expect_err("the replaced link is refused");
        assert_eq!(err, RouteError::StaleGeneration);
    }

    #[test]
    fn a_new_link_generation_may_start_its_revision_over() {
        let mut state = RouteState::new(id(LOCAL));
        state
            .apply_update(id(NAS), 4, 9, &path(vec![id(NAS)]), snapshot(1))
            .expect("the advertisement is accepted");
        state
            .apply_update(id(NAS), 5, 1, &path(vec![id(NAS)]), snapshot(2))
            .expect("the new link starts over");
        assert_eq!(state.candidate(id(NAS)).map(|c| c.link_generation), Some(5));
    }

    #[test]
    fn a_stale_withdrawal_is_refused() {
        let mut state = RouteState::new(id(LOCAL));
        state
            .apply_update(id(NAS), 4, 9, &path(vec![id(NAS)]), snapshot(1))
            .expect("the advertisement is accepted");
        let err = state
            .apply_withdraw(id(NAS), 3, 12)
            .expect_err("the replaced link is refused");
        assert_eq!(err, RouteError::StaleGeneration);
        let err = state
            .apply_withdraw(id(NAS), 4, 9)
            .expect_err("the repeated revision is refused");
        assert_eq!(err, RouteError::StaleRevision);
        assert!(state.selected().is_some());
    }

    #[test]
    fn a_withdrawal_keeps_the_pin_and_advances_the_epoch() {
        let mut state = RouteState::new(id(LOCAL));
        state
            .apply_update(id(NAS), 1, 1, &path(vec![id(NAS)]), snapshot(1))
            .expect("the advertisement is accepted");
        let epoch = state.epoch();
        let outcome = state
            .apply_withdraw(id(NAS), 1, 2)
            .expect("the withdrawal is accepted");
        assert_eq!(outcome, RouteOutcome::Changed);
        assert!(state.selected().is_none());
        assert_eq!(state.epoch(), epoch + 1);
        assert_eq!(
            state.terminal(),
            Some(id(NAS)),
            "the pin survives the withdrawal"
        );
        state
            .apply_update(id(NAS), 2, 1, &path(vec![id(NAS)]), snapshot(2))
            .expect("the same database node restores the route");
        assert!(state.selected().is_some());
    }

    #[test]
    fn a_withdrawn_link_cannot_restore_its_route() {
        let mut state = RouteState::new(id(LOCAL));
        state
            .apply_update(id(NAS), 4, 9, &path(vec![id(NAS)]), snapshot(1))
            .expect("the advertisement is accepted");
        state
            .apply_withdraw(id(NAS), 4, 10)
            .expect("the withdrawal is accepted");
        let err = state
            .apply_update(id(NAS), 4, 9, &path(vec![id(NAS)]), snapshot(2))
            .expect_err("the withdrawn link is refused");
        assert_eq!(err, RouteError::StaleRevision);
        assert!(state.selected().is_none());
    }

    #[test]
    fn an_ended_link_never_restores_its_route() {
        let mut state = RouteState::new(id(LOCAL));
        state
            .apply_update(id(NAS), 4, 9, &path(vec![id(NAS)]), snapshot(1))
            .expect("the advertisement is accepted");
        state.release_link(id(NAS), 4);
        let err = state
            .apply_update(id(NAS), 4, 11, &path(vec![id(NAS)]), snapshot(2))
            .expect_err("the ended link is refused");
        assert_eq!(err, RouteError::StaleGeneration);
        assert!(state.selected().is_none());
    }

    #[test]
    fn a_live_link_may_advertise_its_route_again() {
        let mut state = RouteState::new(id(LOCAL));
        state
            .apply_update(id(NAS), 4, 9, &path(vec![id(NAS)]), snapshot(1))
            .expect("the advertisement is accepted");
        state
            .apply_withdraw(id(NAS), 4, 10)
            .expect("the withdrawal is accepted");
        let outcome = state
            .apply_update(id(NAS), 4, 11, &path(vec![id(NAS)]), snapshot(2))
            .expect("the peer advertises its route again");
        assert_eq!(outcome, RouteOutcome::Changed);
        let selected = state.selected().expect("the route is selected again");
        assert_eq!(selected.snapshot, snapshot(2));
    }

    #[test]
    fn a_snapshot_refresh_keeps_the_epoch() {
        let mut state = RouteState::new(id(LOCAL));
        state
            .apply_update(id(NAS), 1, 1, &path(vec![id(NAS)]), snapshot(1))
            .expect("the advertisement is accepted");
        let epoch = state.epoch();
        let outcome = state
            .apply_update(id(NAS), 1, 2, &path(vec![id(NAS)]), snapshot(2))
            .expect("the advertisement is accepted");
        assert_eq!(outcome, RouteOutcome::SnapshotRefreshed);
        assert_eq!(state.epoch(), epoch);
        let selected = state.selected().expect("the route stays selected");
        assert_eq!(selected.snapshot, snapshot(2));
    }

    #[test]
    fn a_route_change_advances_the_epoch() {
        let mut state = RouteState::new(id(LOCAL));
        state
            .apply_update(id(NAS), 1, 1, &path(vec![id(NAS)]), snapshot(1))
            .expect("the advertisement is accepted");
        let epoch = state.epoch();
        let outcome = state
            .apply_update(id(MAC), 1, 1, &path(vec![id(MAC), id(NAS)]), snapshot(1))
            .expect("the advertisement is accepted");
        assert_eq!(outcome, RouteOutcome::Unchanged, "the shorter route stays");
        let outcome = state
            .apply_withdraw(id(NAS), 1, 2)
            .expect("the withdrawal is accepted");
        assert_eq!(outcome, RouteOutcome::Changed);
        assert_eq!(state.epoch(), epoch + 1);
        let selected = state.selected().expect("the other route is selected");
        assert_eq!(selected.next_hop, id(MAC));
    }

    #[test]
    fn the_cleanup_of_a_replaced_link_keeps_the_new_candidate() {
        let mut state = RouteState::new(id(LOCAL));
        state
            .apply_update(id(NAS), 4, 1, &path(vec![id(NAS)]), snapshot(1))
            .expect("the advertisement is accepted");
        state
            .apply_update(id(NAS), 5, 1, &path(vec![id(NAS)]), snapshot(2))
            .expect("the new link is accepted");
        let outcome = state.release_link(id(NAS), 4);
        assert_eq!(outcome, RouteOutcome::Unchanged);
        assert!(state.selected().is_some());
        assert_eq!(state.candidate(id(NAS)).map(|c| c.link_generation), Some(5));
    }

    #[test]
    fn the_cleanup_of_the_current_link_withdraws_its_route() {
        let mut state = RouteState::new(id(LOCAL));
        state
            .apply_update(id(NAS), 4, 1, &path(vec![id(NAS)]), snapshot(1))
            .expect("the advertisement is accepted");
        let outcome = state.release_link(id(NAS), 4);
        assert_eq!(outcome, RouteOutcome::Changed);
        assert!(state.selected().is_none());
    }

    #[test]
    fn the_candidate_store_never_grows_beyond_the_peer_cap() {
        let mut state = RouteState::new(id(LOCAL));
        for index in 0..MAX_ROUTE_CANDIDATES {
            let peer = id(1000 + index as u128);
            state
                .apply_update(peer, 1, 1, &path(vec![peer, id(NAS)]), snapshot(1))
                .expect("the advertisement is accepted");
        }
        assert_eq!(state.candidate_count(), MAX_ROUTE_CANDIDATES);
        let err = state
            .apply_update(id(9999), 1, 1, &path(vec![id(9999), id(NAS)]), snapshot(1))
            .expect_err("the cap holds");
        assert_eq!(err, RouteError::CandidateCapReached);
        assert_eq!(state.candidate_count(), MAX_ROUTE_CANDIDATES);
    }

    #[test]
    fn an_ended_link_makes_room_for_a_new_peer() {
        let mut state = RouteState::new(id(LOCAL));
        for index in 0..MAX_ROUTE_CANDIDATES {
            let peer = id(1000 + index as u128);
            state
                .apply_update(peer, 1, 1, &path(vec![peer, id(NAS)]), snapshot(1))
                .expect("the advertisement is accepted");
        }
        state.release_link(id(1000), 1);
        state
            .apply_update(id(9999), 1, 1, &path(vec![id(9999), id(NAS)]), snapshot(1))
            .expect("the ended link made room");
        assert_eq!(state.candidate_count(), MAX_ROUTE_CANDIDATES);
        assert!(state.candidate(id(9999)).is_some());
    }

    #[test]
    fn a_stale_candidate_never_takes_over_the_route() {
        let mut state = RouteState::new(id(LOCAL));
        // The direct route of the database node carries the current
        // configuration.
        state
            .apply_update(id(NAS), 1, 1, &path(vec![id(NAS)]), snapshot(2))
            .expect("the advertisement is accepted");
        // The alternate two hop route still carries the older configuration.
        state
            .apply_update(id(MAC), 1, 1, &path(vec![id(MAC), id(NAS)]), snapshot(1))
            .expect("the advertisement is accepted");
        let selected = state.selected().expect("a route is selected");
        assert_eq!(selected.next_hop, id(NAS));
        assert_eq!(selected.snapshot, snapshot(2));

        // The direct link ends: the stale alternate never serves the node.
        let outcome = state.release_link(id(NAS), 1);
        assert_eq!(outcome, RouteOutcome::Changed);
        assert!(
            state.selected().is_none(),
            "a route with an older configuration never serves the node"
        );
    }

    #[test]
    fn a_newer_revision_with_a_stale_configuration_cannot_restore_the_route() {
        let mut state = RouteState::new(id(LOCAL));
        state
            .apply_update(id(NAS), 1, 1, &path(vec![id(NAS)]), snapshot(2))
            .expect("the advertisement is accepted");
        state.release_link(id(NAS), 1);
        assert!(state.selected().is_none());

        // The alternate advertises the same older configuration again, with a
        // newer revision: the route stays gone.
        let outcome = state
            .apply_update(id(MAC), 1, 2, &path(vec![id(MAC), id(NAS)]), snapshot(1))
            .expect("the advertisement is accepted");
        assert_eq!(outcome, RouteOutcome::Unchanged);
        assert!(state.selected().is_none());
    }

    #[test]
    fn a_current_configuration_restores_the_route() {
        let mut state = RouteState::new(id(LOCAL));
        state
            .apply_update(id(NAS), 1, 1, &path(vec![id(NAS)]), snapshot(2))
            .expect("the advertisement is accepted");
        state.release_link(id(NAS), 1);
        assert!(state.selected().is_none());

        // The alternate carries the current configuration: the route is
        // served again.
        let outcome = state
            .apply_update(id(MAC), 1, 2, &path(vec![id(MAC), id(NAS)]), snapshot(2))
            .expect("the advertisement is accepted");
        assert_eq!(outcome, RouteOutcome::Changed);
        let selected = state.selected().expect("the route is selected again");
        assert_eq!(selected.next_hop, id(MAC));
        assert_eq!(selected.snapshot, snapshot(2));
    }

    #[test]
    fn the_watermark_survives_a_withdrawal_and_a_reconnect() {
        let mut state = RouteState::new(id(LOCAL));
        state
            .apply_update(id(NAS), 1, 1, &path(vec![id(NAS)]), snapshot(5))
            .expect("the advertisement is accepted");
        state
            .apply_withdraw(id(NAS), 1, 2)
            .expect("the withdrawal is accepted");
        assert!(state.selected().is_none());

        // The link comes back with an older configuration: the node stays
        // without a route instead of rolling its configuration back.
        let outcome = state
            .apply_update(id(NAS), 2, 1, &path(vec![id(NAS)]), snapshot(4))
            .expect("the advertisement is accepted");
        assert_eq!(outcome, RouteOutcome::Unchanged);
        assert!(state.selected().is_none());

        // The link comes back with the current configuration: the node serves
        // requests again.
        let outcome = state
            .apply_update(id(NAS), 2, 2, &path(vec![id(NAS)]), snapshot(5))
            .expect("the advertisement is accepted");
        assert_eq!(outcome, RouteOutcome::Changed);
        assert!(state.selected().is_some());
    }

    #[test]
    fn a_refused_advertisement_cannot_raise_the_watermark() {
        let mut state = RouteState::new(id(LOCAL));
        state
            .apply_update(id(NAS), 1, 1, &path(vec![id(NAS)]), snapshot(5))
            .expect("the advertisement is accepted");

        // A route of another database node is refused.
        let err = state
            .apply_update(
                id(MAC),
                1,
                1,
                &path(vec![id(MAC), id(OTHER_DB)]),
                snapshot(9),
            )
            .expect_err("the route of another database node is refused");
        assert_eq!(err, RouteError::WrongTerminal);
        assert_eq!(
            state.watermark,
            Some(5),
            "a refused advertisement never raises the watermark"
        );

        // A repeated revision is refused as well.
        let err = state
            .apply_update(id(NAS), 1, 1, &path(vec![id(NAS)]), snapshot(9))
            .expect_err("the repeated revision is refused");
        assert_eq!(err, RouteError::StaleRevision);
        assert_eq!(state.watermark, Some(5));
        assert_eq!(
            state.selected().expect("the route stays").snapshot,
            snapshot(5)
        );
    }

    #[test]
    fn a_current_configuration_outranks_the_distance_of_the_route() {
        let mut state = RouteState::new(id(LOCAL));
        // The two hop route carries the current configuration.
        state
            .apply_update(id(MAC), 1, 1, &path(vec![id(MAC), id(NAS)]), snapshot(3))
            .expect("the advertisement is accepted");
        // The direct route carries an older configuration.
        state
            .apply_update(id(NAS), 1, 1, &path(vec![id(NAS)]), snapshot(2))
            .expect("the advertisement is accepted");
        let selected = state.selected().expect("a route is selected");
        assert_eq!(
            selected.next_hop,
            id(MAC),
            "the current configuration outranks the distance of the route"
        );
        assert_eq!(selected.snapshot, snapshot(3));
    }

    #[test]
    fn a_newer_snapshot_of_the_same_path_keeps_the_epoch() {
        let mut state = RouteState::new(id(LOCAL));
        state
            .apply_update(id(NAS), 1, 1, &path(vec![id(NAS)]), snapshot(1))
            .expect("the advertisement is accepted");
        state
            .apply_update(id(MAC), 1, 1, &path(vec![id(MAC), id(NAS)]), snapshot(1))
            .expect("the advertisement is accepted");
        let epoch = state.epoch();

        // The selected route carries a newer configuration: the route and the
        // running work of the node stay as they are.
        let outcome = state
            .apply_update(id(NAS), 1, 2, &path(vec![id(NAS)]), snapshot(3))
            .expect("the advertisement is accepted");
        assert_eq!(outcome, RouteOutcome::SnapshotRefreshed);
        assert_eq!(state.epoch(), epoch);
        let selected = state.selected().expect("the route stays selected");
        assert_eq!(selected.snapshot, snapshot(3));
    }

    #[test]
    fn a_forwarded_path_must_name_the_peer_it_arrived_from() {
        let mut state = RouteState::new(id(LOCAL));
        state
            .apply_update(id(NAS), 1, 1, &path(vec![id(NAS)]), snapshot(1))
            .expect("the advertisement is accepted");
        state
            .accept_start_path(&path(vec![id(LOCAL), id(NAS)]), id(LOCAL))
            .expect_err("the path does not start at the peer");
    }

    #[test]
    fn a_forwarded_path_must_match_the_selected_route() {
        let mut state = RouteState::new(id(LOCAL));
        state
            .apply_update(id(MAC), 1, 1, &path(vec![id(MAC), id(NAS)]), snapshot(1))
            .expect("the advertisement is accepted");
        let err = state
            .accept_start_path(&path(vec![id(OTHER), id(LOCAL), id(NAS)]), id(OTHER))
            .expect_err("the path does not follow the selected route");
        assert_eq!(err, RouteError::PathMismatch);
    }

    #[test]
    fn a_forwarded_path_that_follows_the_selected_route_is_accepted() {
        let mut state = RouteState::new(id(LOCAL));
        state
            .apply_update(id(NAS), 1, 1, &path(vec![id(NAS)]), snapshot(1))
            .expect("the advertisement is accepted");
        state
            .accept_start_path(&path(vec![id(OTHER), id(LOCAL), id(NAS)]), id(OTHER))
            .expect("the path follows the selected route");
    }

    #[test]
    fn a_forwarded_path_that_names_another_database_node_is_refused() {
        let mut state = RouteState::new(id(LOCAL));
        state
            .apply_update(id(NAS), 1, 1, &path(vec![id(NAS)]), snapshot(1))
            .expect("the advertisement is accepted");
        let err = state
            .accept_start_path(&path(vec![id(OTHER), id(LOCAL), id(OTHER_DB)]), id(OTHER))
            .expect_err("the path leads to another database node");
        assert_eq!(err, RouteError::WrongTerminal);
    }

    #[test]
    fn the_database_node_accepts_a_path_that_ends_at_itself() {
        let state = RouteState::new(id(NAS));
        assert_eq!(state.local(), id(NAS));
        state
            .accept_terminal_path(&path(vec![id(LOCAL), id(NAS)]), id(LOCAL))
            .expect("the path ends at the database node");
        let err = state
            .accept_terminal_path(&path(vec![id(LOCAL), id(NAS)]), id(OTHER))
            .expect_err("the path did not arrive from that peer");
        assert_eq!(err, RouteError::NotAdjacent);
        let err = state
            .accept_terminal_path(&path(vec![id(LOCAL), id(MAC)]), id(LOCAL))
            .expect_err("the path does not end at the database node");
        assert_eq!(err, RouteError::WrongTerminal);
    }
}
