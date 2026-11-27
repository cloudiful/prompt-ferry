//! The forwarding hub of a database-less node.
//!
//! A database-less node owns one route: the path from itself to the database
//! node that executes its traffic. Every live peer link may advertise one
//! candidate, the shortest candidate wins, and the node forwards the traffic
//! of its own clients and of its peers over that single route.
//!
//! The hub is an actor: one task owns the whole forwarding state of the
//! node. Every decision arrives as a command, the actor applies it to its own
//! state and never waits for a peer, a sink or a byte budget while it does
//! so. The lifecycle and the route controls travel on a reserved priority
//! path, the traffic travels on a data path that is bounded by a count and
//! by a byte budget. A command of a link that is no longer admitted is
//! refused when the actor executes it, so a callback that was queued before
//! the link was replaced never acts on the state of the node.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;

use anyhow::{anyhow, bail};
use tokio::sync::{
    Mutex, OwnedRwLockReadGuard, OwnedSemaphorePermit, Semaphore, mpsc, oneshot, watch,
};
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

use crate::bridge::link::BRIDGE_LINK_BUFFER;
use crate::protocol::{BridgeMessage, ResponseError};
use crate::relay::{
    AppState, WorkerSender, cleanup_worker, dispatch_worker_bridge_message, register_worker,
};
use crate::worker::runtime::node::identity::NodeId;

use super::admission::AdmissionToken;
use super::route::{RouteError, RouteOutcome, RouteState};
use super::transport::PeerWriter;
use super::wire::{MAX_PEER_ROUTE_NODES, PeerEnvelope, PeerEnvelopeKind, PeerPath};

/// How many encoded frames a peer link may hold before its sender is
/// refused.
///
/// A slot is one encoded outbound peer envelope, not one request: a request
/// may travel as several frames. The actor never waits for a peer, so the
/// slots are the headroom a producer burst finds before the egress of the
/// link drains it. The count is a bounded headroom and not a scheduling
/// guarantee: a producer that outruns the egress of the link still finds a
/// full queue and the link is evicted.
pub(crate) const PEER_ENVELOPE_BUFFER: usize = 256;

/// How many bytes of unaccepted envelopes a peer link may hold.
///
/// The budget covers the whole encoded envelope, so the overhead of the peer
/// wrapper is accounted for as well as the bridge payload. It matches the
/// outbound budget of the bridge, with room for the envelope of a full bridge
/// frame.
pub(crate) const PEER_ENVELOPE_BUDGET_BYTES: usize = 16 * 1024 * 1024 + 64 * 1024;

/// How long a session waits for the outbound queue of its link before it
/// gives up.
///
/// The wait is a bounded refusal and not a network timeout: it only caps how
/// long a session blocks on a control frame of its own link. The queue of the
/// peer is never held by this constant, and a peer that stops draining its
/// queue is disconnected instead of being waited for.
pub(crate) const PEER_SEND_TIMEOUT: Duration = Duration::from_secs(5);

/// How many requests a node forwards at the same time.
pub(crate) const MAX_FORWARDED_REQUESTS: usize = 1024;

/// The longest request id that may travel over a peer link.
pub(crate) const MAX_ID_BYTES: usize = 256;

/// How many traffic commands the actor may hold.
///
/// The count bounds the traffic that waits for the actor, so a peer that
/// floods the node cannot make it queue without a limit.
pub(crate) const PEER_INGRESS_BUFFER: usize = 256;

/// How many cooperative yields a full ingress gets before it refuses.
///
/// The yield is a best effort of the scheduler: it gives the consumer of the
/// ingress a chance to run, but it never promises that it runs. The recovery
/// is finite and it never waits for the consumer.
const PEER_INGRESS_RECOVERY_YIELDS: usize = 4;

/// The byte budget of the traffic that waits for the actor.
///
/// The budget is the outbound budget of a link, so the traffic that waits
/// for a decision never holds more bytes than one link may hold.
pub(crate) const PEER_INGRESS_BUDGET_BYTES: usize = PEER_ENVELOPE_BUDGET_BYTES;

/// How many lifecycle and route commands the actor may hold.
///
/// The control path has its own capacity, so a flood of traffic never
/// starves the cleanup of a link.
pub(crate) const PEER_CONTROL_BUFFER: usize = 64;

/// The byte budget of the route controls that wait for the actor.
///
/// The budget matches the traffic budget, so a route snapshot that carries
/// the peers of the node always fits the control path next to the reserve of
/// the envelope wrapper and of the rewrite of the request id.
pub(crate) const PEER_CONTROL_BUDGET_BYTES: usize = PEER_ENVELOPE_BUDGET_BYTES;

/// The reserve of an ingressed message.
///
/// The budget of the ingress covers the encoded message and the reserve of
/// the envelope wrapper and of the rewrite of the request id.
const PEER_FRAME_RESERVE_BYTES: usize = 512;

/// The kind of a request, by the surface it belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum RequestKind {
    /// An ordinary HTTP request.
    Http,
    /// An MCP request.
    Mcp,
    /// A realtime session.
    Realtime,
}

/// The fence of a local start that waits in the ingress of the actor.
///
/// The fence is private to the pump and the actor: it is never part of an
/// owner and never part of the protocol, and it dies with the command that
/// carries it.
struct StartFence {
    cancelled: AtomicBool,
}

impl StartFence {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            cancelled: AtomicBool::new(false),
        })
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
}

/// Why the ingress of the node refused a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IngressRefusal {
    /// The data channel of the actor holds every slot.
    ChannelFull,
    /// The byte budget of the ingress is exhausted.
    ByteBudgetExhausted,
    /// The data channel of the actor is closed.
    ChannelClosed,
    /// The control path of the actor cannot carry the retirement.
    ControlUnavailable,
    /// The bounded tracking of the retired flows is exhausted.
    TrackingExhausted,
}

impl IngressRefusal {
    /// The stable name of the reason, free of any content of the frame.
    fn reason(self) -> &'static str {
        match self {
            Self::ChannelFull => "channel_full",
            Self::ByteBudgetExhausted => "byte_budget_exhausted",
            Self::ChannelClosed => "channel_closed",
            Self::ControlUnavailable => "control_unavailable",
            Self::TrackingExhausted => "retirement_tracking_exhausted",
        }
    }
}

/// What a scoped retirement of a request did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Retirement {
    /// The request of the frame was retired.
    Retired,
    /// The frame named no request of the link, so nothing was retired.
    NoOwner,
}

/// The safe counters of the ingress of the node.
///
/// The counters are a concurrent observation of the ingress at the moment of
/// the refusal, not the exact peak it reached.
#[derive(Clone, Copy, Debug)]
struct IngressState {
    pending: usize,
    capacity: usize,
    available_bytes: usize,
    total_bytes: usize,
}

/// The context of a refusal of the ingress of the node.
struct IngressRefusalContext<'a> {
    source: &'a str,
    reason: IngressRefusal,
    frame: FrameVariant,
    peer: Option<NodeId>,
    generation: Option<u64>,
    worker: Option<usize>,
    epoch: Option<u64>,
    state: IngressState,
    outcome: &'a str,
}

/// Report a frame that the ingress of the node refused.
///
/// The report names the source of the frame, the variant of the frame, the
/// distinct reason of the refusal, the safe counters of the ingress and what
/// the node did with the refusal. It never carries the content of the frame:
/// no body, no message, no route, no key and no token ever reaches the log.
fn report_ingress_refusal(context: IngressRefusalContext<'_>) {
    let reason = context.reason.reason();
    match context.reason {
        // A closed channel is the ordinary end of the actor, not a pressure
        // of the ingress.
        IngressRefusal::ChannelClosed => info!(
            category = "peer_ingress_diag",
            source = context.source,
            reason,
            frame = context.frame.name(),
            surface = ?context.frame.surface(),
            peer = ?context.peer,
            generation = ?context.generation,
            worker = ?context.worker,
            epoch = ?context.epoch,
            pending = context.state.pending,
            capacity = context.state.capacity,
            available_bytes = context.state.available_bytes,
            total_bytes = context.state.total_bytes,
            outcome = context.outcome,
            "the ingress of the node refused a frame"
        ),
        _ => warn!(
            category = "peer_ingress_diag",
            source = context.source,
            reason,
            frame = context.frame.name(),
            surface = ?context.frame.surface(),
            peer = ?context.peer,
            generation = ?context.generation,
            worker = ?context.worker,
            epoch = ?context.epoch,
            pending = context.state.pending,
            capacity = context.state.capacity,
            available_bytes = context.state.available_bytes,
            total_bytes = context.state.total_bytes,
            outcome = context.outcome,
            "the ingress of the node refused a frame"
        ),
    }
}

fn ingress_counters(
    data: &mpsc::Sender<Command>,
    budget: &Arc<Semaphore>,
    total_bytes: usize,
) -> IngressState {
    IngressState {
        pending: data.max_capacity() - data.capacity(),
        capacity: data.max_capacity(),
        available_bytes: budget.available_permits(),
        total_bytes,
    }
}

/// What the pump does with a frame that the ingress of the node refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RefusalAction {
    FailRequest,
    RetireRequest,
    FailWorker,
}

/// The action for a frame the ingress of the node refused.
///
/// A new start never reached the actor, so it is failed to the pending
/// request of the relay state. A continuation of a running request is
/// retired in its own scope, as long as the bounded tracking of the failed
/// flows has room. A frame that belongs to no request, and a tracking that
/// is exhausted, fail the worker of the route as they did before.
fn refusal_action(role: MessageRole, tracking_has_room: bool) -> RefusalAction {
    match role {
        MessageRole::Start(_) => RefusalAction::FailRequest,
        MessageRole::Stream(_) | MessageRole::End(_) | MessageRole::Cancel(_) => {
            if tracking_has_room {
                RefusalAction::RetireRequest
            } else {
                RefusalAction::FailWorker
            }
        }
        _ => RefusalAction::FailWorker,
    }
}

/// The content-free variant of a frame.
///
/// The variant names the frame of the protocol without any content of it, so
/// a diagnostic and an admission decision can name the frame without
/// carrying a body, a header, a key or an id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FrameVariant {
    RequestStart,
    RequestChunk,
    RequestEnd,
    RequestCancel,
    McpRequestStart,
    McpRequestChunk,
    McpRequestEnd,
    McpRequestCancel,
    RealtimeSessionStart,
    RealtimeClientEvent,
    RealtimeServerEvent,
    RealtimeSessionClose,
    ResponseStart,
    ResponseChunk,
    ResponseEnd,
    McpResponseStart,
    McpResponseChunk,
    McpResponseEnd,
    ResponseError,
    Control,
}

impl FrameVariant {
    /// The name of the variant in a diagnostic.
    fn name(self) -> &'static str {
        match self {
            Self::RequestStart => "request_start",
            Self::RequestChunk => "request_chunk",
            Self::RequestEnd => "request_end",
            Self::RequestCancel => "request_cancel",
            Self::McpRequestStart => "mcp_request_start",
            Self::McpRequestChunk => "mcp_request_chunk",
            Self::McpRequestEnd => "mcp_request_end",
            Self::McpRequestCancel => "mcp_request_cancel",
            Self::RealtimeSessionStart => "realtime_session_start",
            Self::RealtimeClientEvent => "realtime_client_event",
            Self::RealtimeServerEvent => "realtime_server_event",
            Self::RealtimeSessionClose => "realtime_session_close",
            Self::ResponseStart => "response_start",
            Self::ResponseChunk => "response_chunk",
            Self::ResponseEnd => "response_end",
            Self::McpResponseStart => "mcp_response_start",
            Self::McpResponseChunk => "mcp_response_chunk",
            Self::McpResponseEnd => "mcp_response_end",
            Self::ResponseError => "response_error",
            Self::Control => "control",
        }
    }

    /// The request surface of the frame, if it belongs to one.
    ///
    /// The surface is the kind of the request, not the kind of the frame: a
    /// frame of the shared error belongs to no surface of its own.
    fn surface(self) -> Option<RequestKind> {
        match self {
            Self::RequestStart
            | Self::RequestChunk
            | Self::RequestEnd
            | Self::RequestCancel
            | Self::ResponseStart
            | Self::ResponseChunk
            | Self::ResponseEnd => Some(RequestKind::Http),
            Self::McpRequestStart
            | Self::McpRequestChunk
            | Self::McpRequestEnd
            | Self::McpRequestCancel
            | Self::McpResponseStart
            | Self::McpResponseChunk
            | Self::McpResponseEnd => Some(RequestKind::Mcp),
            Self::RealtimeSessionStart
            | Self::RealtimeClientEvent
            | Self::RealtimeServerEvent
            | Self::RealtimeSessionClose => Some(RequestKind::Realtime),
            Self::ResponseError | Self::Control => None,
        }
    }

    /// Whether the frame is a response of the executor of the route.
    fn is_response(self) -> bool {
        matches!(
            self,
            Self::ResponseStart
                | Self::ResponseChunk
                | Self::ResponseEnd
                | Self::McpResponseStart
                | Self::McpResponseChunk
                | Self::McpResponseEnd
                | Self::RealtimeServerEvent
                | Self::RealtimeSessionClose
                | Self::ResponseError
        )
    }

    /// Whether the frame starts a request.
    fn is_start(self) -> bool {
        matches!(
            self,
            Self::RequestStart | Self::McpRequestStart | Self::RealtimeSessionStart
        )
    }
}

/// The variant of a frame.
fn frame_variant(message: &BridgeMessage) -> FrameVariant {
    match message {
        BridgeMessage::RequestStart(_) => FrameVariant::RequestStart,
        BridgeMessage::RequestChunk(_) => FrameVariant::RequestChunk,
        BridgeMessage::RequestEnd(_) => FrameVariant::RequestEnd,
        BridgeMessage::RequestCancel(_) => FrameVariant::RequestCancel,
        BridgeMessage::McpRequestStart(_) => FrameVariant::McpRequestStart,
        BridgeMessage::McpRequestChunk(_) => FrameVariant::McpRequestChunk,
        BridgeMessage::McpRequestEnd(_) => FrameVariant::McpRequestEnd,
        BridgeMessage::McpRequestCancel(_) => FrameVariant::McpRequestCancel,
        BridgeMessage::RealtimeSessionStart(_) => FrameVariant::RealtimeSessionStart,
        BridgeMessage::RealtimeClientEvent(_) => FrameVariant::RealtimeClientEvent,
        BridgeMessage::RealtimeServerEvent(_) => FrameVariant::RealtimeServerEvent,
        BridgeMessage::RealtimeSessionClose(_) => FrameVariant::RealtimeSessionClose,
        BridgeMessage::ResponseStart(_) => FrameVariant::ResponseStart,
        BridgeMessage::ResponseChunk(_) => FrameVariant::ResponseChunk,
        BridgeMessage::ResponseEnd(_) => FrameVariant::ResponseEnd,
        BridgeMessage::McpResponseStart(_) => FrameVariant::McpResponseStart,
        BridgeMessage::McpResponseChunk(_) => FrameVariant::McpResponseChunk,
        BridgeMessage::McpResponseEnd(_) => FrameVariant::McpResponseEnd,
        BridgeMessage::ResponseError(_) => FrameVariant::ResponseError,
        BridgeMessage::Ping | BridgeMessage::Pong | BridgeMessage::ConfigSnapshot(_) => {
            FrameVariant::Control
        }
    }
}

/// Whether a frame of a peer is valid for the direction it travels in.
///
/// The predicate is the acceptance of the normal dispatch: only a response
/// of the executor is a frame of the upstream direction, and a start of a
/// peer never travels in the downstream direction. A frame that the normal
/// dispatch would drop retires nothing.
fn frame_accepted(direction: &Direction, variant: FrameVariant) -> bool {
    match direction {
        Direction::Upstream => variant.is_response(),
        Direction::Downstream(_) => !variant.is_start(),
    }
}

/// Drop the fences of the starts that already retired.
///
/// A fence dies with the command that carries it, so the tracking of the
/// pump never grows beyond the starts that still wait in the ingress.
fn prune_fences(fences: &mut HashMap<(RequestKind, String), Vec<Weak<StartFence>>>) {
    fences.retain(|_, entries| {
        entries.retain(|entry| entry.strong_count() > 0);
        !entries.is_empty()
    });
}

/// Abort the starts of a flow that wait in the ingress.
///
/// Every fence of the flow is aborted, so a duplicate start of the same flow
/// cannot create an owner behind the abort.
fn cancel_fences(
    fences: &mut HashMap<(RequestKind, String), Vec<Weak<StartFence>>>,
    flow: &(RequestKind, String),
) {
    let Some(entries) = fences.get(flow) else {
        return;
    };
    for entry in entries {
        if let Some(fence) = entry.upgrade() {
            fence.cancel();
        }
    }
}

/// What a bridge message does to a request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MessageRole {
    /// The message starts a request.
    Start(RequestKind),
    /// The message carries more of the body of a request.
    Stream(RequestKind),
    /// The message ends the body of a request.
    End(RequestKind),
    /// The message cancels a request.
    Cancel(RequestKind),
    /// The message carries the beginning of a response.
    Response(RequestKind),
    /// The message ends a response.
    Terminal(RequestKind),
    /// The message is a terminal failure shared by every request kind.
    Error,
    /// The message is not bound to a request.
    Control,
}

/// Classify a bridge message.
fn classify(message: &BridgeMessage) -> MessageRole {
    use BridgeMessage::*;
    match message {
        RequestStart(_) => MessageRole::Start(RequestKind::Http),
        RequestChunk(_) => MessageRole::Stream(RequestKind::Http),
        RequestEnd(_) => MessageRole::End(RequestKind::Http),
        RequestCancel(_) => MessageRole::Cancel(RequestKind::Http),
        McpRequestStart(_) => MessageRole::Start(RequestKind::Mcp),
        McpRequestChunk(_) => MessageRole::Stream(RequestKind::Mcp),
        McpRequestEnd(_) => MessageRole::End(RequestKind::Mcp),
        McpRequestCancel(_) => MessageRole::Cancel(RequestKind::Mcp),
        RealtimeSessionStart(_) => MessageRole::Start(RequestKind::Realtime),
        RealtimeClientEvent(_) => MessageRole::Stream(RequestKind::Realtime),
        RealtimeServerEvent(_) => MessageRole::Response(RequestKind::Realtime),
        RealtimeSessionClose(_) => MessageRole::Terminal(RequestKind::Realtime),
        McpResponseStart(_) => MessageRole::Response(RequestKind::Mcp),
        McpResponseChunk(_) => MessageRole::Response(RequestKind::Mcp),
        McpResponseEnd(_) => MessageRole::Terminal(RequestKind::Mcp),
        ResponseStart(_) => MessageRole::Response(RequestKind::Http),
        ResponseChunk(_) => MessageRole::Response(RequestKind::Http),
        ResponseEnd(_) => MessageRole::Terminal(RequestKind::Http),
        ResponseError(_) => MessageRole::Error,
        Ping | Pong | ConfigSnapshot(_) => MessageRole::Control,
    }
}

/// The kind of an executor response, if the message carries one.
///
/// A [`BridgeMessage::ResponseError`] is shared by all three request kinds, so
/// it has no kind of its own and is correlated through its owner.
fn response_kind(message: &BridgeMessage) -> Option<RequestKind> {
    use BridgeMessage::*;
    match message {
        ResponseStart(_) | ResponseChunk(_) | ResponseEnd(_) => Some(RequestKind::Http),
        McpResponseStart(_) | McpResponseChunk(_) | McpResponseEnd(_) => Some(RequestKind::Mcp),
        RealtimeServerEvent(_) | RealtimeSessionClose(_) => Some(RequestKind::Realtime),
        _ => None,
    }
}

/// Whether a client frame ends the request on the executor side.
///
/// A realtime close is a client-side cancellation, not an executor response:
/// the client that owns the session ended it and the executor must stop.
fn is_client_terminal(message: &BridgeMessage) -> bool {
    matches!(
        message,
        BridgeMessage::RequestCancel(_)
            | BridgeMessage::McpRequestCancel(_)
            | BridgeMessage::RealtimeSessionClose(_)
    )
}

/// The cancellation that stops a request of the given kind upstream.
fn cancellation(kind: RequestKind, request_id: &str) -> BridgeMessage {
    use BridgeMessage::*;
    match kind {
        RequestKind::Http => RequestCancel(crate::protocol::BridgeRequestCancel {
            request_id: request_id.to_string(),
            reason: "peer_cancelled".to_string(),
            response_started: false,
        }),
        RequestKind::Mcp => McpRequestCancel(crate::protocol::McpRequestCancel {
            request_id: request_id.to_string(),
            reason: "peer_cancelled".to_string(),
            response_started: false,
        }),
        RequestKind::Realtime => RealtimeSessionClose(crate::protocol::RealtimeSessionClose {
            request_id: request_id.to_string(),
            code: None,
            reason: Some("peer_cancelled".to_string()),
            response_started: false,
        }),
    }
}

/// The request id of a bridge message, if it carries one.
fn request_id_of(message: &BridgeMessage) -> Option<&str> {
    use BridgeMessage::*;
    match message {
        RequestStart(v) => Some(&v.request_id),
        RequestChunk(v) => Some(&v.request_id),
        RequestEnd(v) => Some(&v.request_id),
        RequestCancel(v) => Some(&v.request_id),
        McpRequestStart(v) => Some(&v.request_id),
        McpRequestChunk(v) => Some(&v.request_id),
        McpRequestEnd(v) => Some(&v.request_id),
        McpRequestCancel(v) => Some(&v.request_id),
        RealtimeSessionStart(v) => Some(&v.request_id),
        RealtimeClientEvent(v) => Some(&v.request_id),
        RealtimeServerEvent(v) => Some(&v.request_id),
        RealtimeSessionClose(v) => Some(&v.request_id),
        McpResponseStart(v) => Some(&v.request_id),
        McpResponseChunk(v) => Some(&v.request_id),
        McpResponseEnd(v) => Some(&v.request_id),
        ResponseStart(v) => Some(&v.request_id),
        ResponseChunk(v) => Some(&v.request_id),
        ResponseEnd(v) => Some(&v.request_id),
        ResponseError(v) => Some(&v.request_id),
        Ping | Pong | ConfigSnapshot(_) => None,
    }
}

/// Replace the request id of a bridge message.
fn set_request_id(message: &mut BridgeMessage, id: &str) -> bool {
    use BridgeMessage::*;
    match message {
        RequestStart(v) => v.request_id = id.to_string(),
        RequestChunk(v) => v.request_id = id.to_string(),
        RequestEnd(v) => v.request_id = id.to_string(),
        RequestCancel(v) => v.request_id = id.to_string(),
        McpRequestStart(v) => v.request_id = id.to_string(),
        McpRequestChunk(v) => v.request_id = id.to_string(),
        McpRequestEnd(v) => v.request_id = id.to_string(),
        McpRequestCancel(v) => v.request_id = id.to_string(),
        RealtimeSessionStart(v) => v.request_id = id.to_string(),
        RealtimeClientEvent(v) => v.request_id = id.to_string(),
        RealtimeServerEvent(v) => v.request_id = id.to_string(),
        RealtimeSessionClose(v) => v.request_id = id.to_string(),
        McpResponseStart(v) => v.request_id = id.to_string(),
        McpResponseChunk(v) => v.request_id = id.to_string(),
        McpResponseEnd(v) => v.request_id = id.to_string(),
        ResponseStart(v) => v.request_id = id.to_string(),
        ResponseChunk(v) => v.request_id = id.to_string(),
        ResponseEnd(v) => v.request_id = id.to_string(),
        ResponseError(v) => v.request_id = id.to_string(),
        Ping | Pong | ConfigSnapshot(_) => return false,
    }
    true
}

/// The byte cost of a message that enters the ingress of the actor.
///
/// The cost covers the encoded message and the reserve of the envelope
/// wrapper and of the rewrite of the request id.
fn ingress_bytes(message: &BridgeMessage) -> usize {
    crate::bridge_wire::encode_message(message)
        .map(|bytes| bytes.len())
        .unwrap_or_default()
        + PEER_FRAME_RESERVE_BYTES
}

/// One live peer link of a node.
#[derive(Clone)]
struct Link {
    /// The peer the link belongs to.
    peer: NodeId,
    /// The generation of the link.
    generation: u64,
    /// The sender of the envelopes of the link.
    tx: EnvelopeSender,
    /// Stops the session of the link when the hub can no longer use it.
    shutdown: watch::Sender<bool>,
    /// The authority of the link.
    ///
    /// The actor invalidates the token when it stops the link, so the
    /// session of the link loses the authority before it is told to stop.
    token: AdmissionToken,
}

impl Link {
    /// The guard of the publication of the link, if the link may still carry
    /// a frame.
    ///
    /// A link that is gone, replaced or dead cannot carry a frame, and such
    /// a refusal is never a full queue.
    fn try_admit(&self) -> Result<OwnedRwLockReadGuard<()>, EnqueueError> {
        let Some(guard) = self.token.try_read_publication() else {
            return Err(EnqueueError::LinkUnavailable);
        };
        if !self.token.is_live() {
            drop(guard);
            return Err(EnqueueError::LinkUnavailable);
        }
        Ok(guard)
    }
}

/// One encoded envelope waiting for the sink of its link.
pub(crate) struct QueuedEnvelope {
    /// The exact bytes the sink writes.
    bytes: Vec<u8>,
    /// The byte budget of the envelope, released when the sink accepted it.
    permit: OwnedSemaphorePermit,
}

impl QueuedEnvelope {
    /// Split the bytes from their byte budget.
    pub(crate) fn into_parts(self) -> (Vec<u8>, OwnedSemaphorePermit) {
        (self.bytes, self.permit)
    }
}

/// Why an envelope could not be queued on a link.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EnqueueError {
    /// The envelope does not encode.
    EncodeFailed,
    /// The envelope exceeds the byte budget of the link.
    TooLarge,
    /// The byte budget of the link is exhausted.
    BudgetExhausted,
    /// The queue of the link is full.
    QueueFull,
    /// The queue of the link is closed.
    ///
    /// The receiving half of the link is gone, so the frame has nowhere to
    /// go. A closed queue is not a full queue: the link cannot carry the
    /// frame because it ended, not because it is saturated.
    QueueClosed,
    /// The link is no longer admitted.
    ///
    /// The link is gone, replaced or dead, so the frame is refused before the
    /// queue of the link is touched. Such a refusal is never a full queue.
    LinkUnavailable,
}

impl EnqueueError {
    /// The stable name of the reason.
    ///
    /// The name is part of the diagnostic of a refusal, so it stays short
    /// and free of any content of the frame.
    pub(crate) fn reason(self) -> &'static str {
        match self {
            Self::EncodeFailed => "encode_failed",
            Self::TooLarge => "too_large",
            Self::BudgetExhausted => "byte_budget_exhausted",
            Self::QueueFull => "queue_full",
            Self::QueueClosed => "queue_closed",
            Self::LinkUnavailable => "link_unavailable",
        }
    }
}

/// The safe counters of the outbound queue of a link.
///
/// The counters are a concurrent observation of the queue at the moment of
/// the refusal, not the exact peak it reached.
#[derive(Clone, Copy, Debug)]
struct QueueState {
    /// The frames that wait in the queue.
    pending_frames: usize,
    /// The frames the queue may hold.
    frame_capacity: usize,
    /// The bytes the queue may still accept.
    available_bytes: usize,
    /// The bytes the queue may hold.
    byte_capacity: usize,
}

/// The bounded, byte-accounted outbound queue of one peer link.
///
/// The queue holds a bounded number of frames and a bounded number of bytes:
/// a slow sink makes the producer wait for the byte budget instead of
/// retaining frames outside the budget of the bridge.
#[derive(Clone)]
pub(crate) struct EnvelopeSender {
    tx: mpsc::Sender<QueuedEnvelope>,
    budget: Arc<Semaphore>,
    capacity: usize,
}

/// The receiving side of a peer envelope queue.
pub(crate) struct EnvelopeReceiver(mpsc::Receiver<QueuedEnvelope>);

impl EnvelopeReceiver {
    /// The next queued envelope, or `None` when the link ended.
    pub(crate) async fn recv(&mut self) -> Option<QueuedEnvelope> {
        self.0.recv().await
    }
}

/// Build the bounded outbound queue of a peer link.
pub(crate) fn envelope_channel() -> (EnvelopeSender, EnvelopeReceiver) {
    envelope_channel_with_budget(PEER_ENVELOPE_BUDGET_BYTES)
}

/// Build a bounded outbound queue with an explicit byte budget.
pub(crate) fn envelope_channel_with_budget(budget: usize) -> (EnvelopeSender, EnvelopeReceiver) {
    let (tx, rx) = mpsc::channel(PEER_ENVELOPE_BUFFER);
    (
        EnvelopeSender {
            tx,
            budget: Arc::new(Semaphore::new(budget)),
            capacity: budget,
        },
        EnvelopeReceiver(rx),
    )
}

impl EnvelopeSender {
    /// Queue an envelope, waiting for the byte budget of the link.
    pub(crate) async fn send(&self, envelope: PeerEnvelope) -> anyhow::Result<()> {
        let bytes = envelope.encode()?;
        let size = bytes.len().max(1);
        if size > self.capacity {
            bail!("the peer envelope exceeds the outbound byte budget");
        }
        let permit = self
            .budget
            .clone()
            .acquire_many_owned(size as u32)
            .await
            .map_err(|_| anyhow!("the peer link closed its byte budget"))?;
        self.tx
            .send(QueuedEnvelope { bytes, permit })
            .await
            .map_err(|_| anyhow!("the peer link closed its outbound queue"))
    }

    /// Queue an envelope without waiting.
    ///
    /// The actor never waits for a peer, so a link that does not accept the
    /// frame now is reported to the caller instead of being waited for.
    ///
    /// A closed queue is recognized before the frame is encoded and before
    /// the byte budget is taken, so a link that ended is never reported as a
    /// byte exhaustion. The frame is still matched against the result of the
    /// send itself, because the queue may close while the frame is queued.
    pub(crate) fn try_send(&self, envelope: PeerEnvelope) -> Result<(), EnqueueError> {
        if self.tx.is_closed() {
            return Err(EnqueueError::QueueClosed);
        }
        let bytes = envelope.encode().map_err(|_| EnqueueError::EncodeFailed)?;
        let size = bytes.len().max(1);
        if size > self.capacity {
            return Err(EnqueueError::TooLarge);
        }
        let permit = self
            .budget
            .clone()
            .try_acquire_many_owned(size as u32)
            .map_err(|_| EnqueueError::BudgetExhausted)?;
        match self.tx.try_send(QueuedEnvelope { bytes, permit }) {
            Ok(()) => Ok(()),
            Err(mpsc::error::TrySendError::Full(_)) => Err(EnqueueError::QueueFull),
            Err(mpsc::error::TrySendError::Closed(_)) => Err(EnqueueError::QueueClosed),
        }
    }

    /// The safe counters of the queue.
    fn queue_state(&self) -> QueueState {
        QueueState {
            pending_frames: self.tx.max_capacity() - self.tx.capacity(),
            frame_capacity: self.tx.max_capacity(),
            available_bytes: self.budget.available_permits(),
            byte_capacity: self.capacity,
        }
    }
}

/// The link that may carry a frame of the given generation.
///
/// A frame of a generation that no longer has a link is refused, and such a
/// refusal is never a full queue.
fn admitted_link(
    hub: &HubState,
    peer: &NodeId,
    link_generation: u64,
) -> Result<Link, EnqueueError> {
    hub.link_of(peer, link_generation)
        .ok_or(EnqueueError::LinkUnavailable)
}

/// Report a frame that the queue of a peer link refused.
///
/// The report names the link, the kind of the frame, the distinct reason of
/// the refusal and the safe counters of the queue. It never carries the
/// content of the frame: no body, no message, no route, no key and no token
/// ever reaches the log.
fn report_enqueue_refusal(
    peer: &NodeId,
    link_generation: u64,
    frame_kind: PeerEnvelopeKind,
    error: EnqueueError,
    state: QueueState,
) {
    let reason = error.reason();
    match error {
        // A closed queue is the ordinary end of a link, so it is a teardown
        // observation and not a saturation of the link.
        EnqueueError::QueueClosed => info!(
            category = "peer_queue_diag",
            peer = %peer,
            link_generation,
            frame_kind = ?frame_kind,
            reason,
            pending_frames = state.pending_frames,
            frame_capacity = state.frame_capacity,
            available_bytes = state.available_bytes,
            byte_capacity = state.byte_capacity,
            "the peer queue is closed"
        ),
        _ => warn!(
            category = "peer_queue_diag",
            peer = %peer,
            link_generation,
            frame_kind = ?frame_kind,
            reason,
            pending_frames = state.pending_frames,
            frame_capacity = state.frame_capacity,
            available_bytes = state.available_bytes,
            byte_capacity = state.byte_capacity,
            "the peer queue did not accept the frame"
        ),
    }
}

/// Write the queued envelopes of a link to the peer.
///
/// The byte budget of an envelope is released only after the sink accepted
/// the frame, so a slow sink keeps its frames inside the budget.
pub(crate) async fn write_envelopes_to_peer<W: PeerWriter + Send + 'static>(
    mut writer: W,
    mut receiver: EnvelopeReceiver,
) -> anyhow::Result<()> {
    while let Some(envelope) = receiver.recv().await {
        let (bytes, permit) = envelope.into_parts();
        writer.send_binary(bytes).await?;
        drop(permit);
    }
    Ok(())
}

/// The advertisement counter of a link.
struct Counter {
    /// The generation of the link the counter belongs to.
    link_generation: u64,
    /// The number of advertisements the link already sent.
    value: u64,
    /// Whether the peer of the link learned the route of the node.
    ///
    /// A peer that learned the route learns that it is withdrawn when the
    /// route of the node changes. A peer that never learned it has nothing
    /// to withdraw, so it is not troubled with a frame it would ignore.
    learned: bool,
}

/// A request that travels over the route of the node.
#[derive(Clone)]
struct Owner {
    /// The surface the request belongs to.
    kind: RequestKind,
    /// The epoch of the route the request was admitted on.
    epoch: u64,
    /// The peer the request travels to.
    upstream_peer: NodeId,
    /// The generation of the link the request travels on.
    upstream_generation: u64,
    /// The peer the request came from, if it was forwarded from a peer.
    downstream: Option<Downstream>,
}

/// The peer a forwarded request came from.
#[derive(Clone)]
struct Downstream {
    /// The peer the request came from.
    peer: NodeId,
    /// The generation of the link the request came in on.
    link_generation: u64,
    /// The request id the peer knows the request by.
    request_id: String,
}

/// The key of a forwarded request on the link it came in on.
#[derive(Clone, PartialEq, Eq, Hash)]
struct DownstreamKey {
    peer: NodeId,
    link_generation: u64,
    request_id: String,
}

/// The virtual worker of the selected route.
struct VirtualWorker {
    /// The worker id of the relay state.
    id: usize,
    /// The epoch of the route the worker serves.
    epoch: u64,
    /// The peer the traffic of the worker travels to.
    upstream_peer: NodeId,
    /// The generation of the link the traffic of the worker travels on.
    upstream_generation: u64,
    /// The task that forwards the traffic of the relay state.
    task: JoinHandle<()>,
}

/// The admission that carries the traffic of the selected route.
enum Source {
    /// The node has no route.
    None,
    /// The node has a route, but the link of that route is gone.
    Missing,
    /// The admission of the link that carries the traffic of the route.
    Token(AdmissionToken),
}

/// A frame that the actor sends to a peer.
struct PeerFrame {
    /// The peer the frame goes to.
    peer: NodeId,
    /// The generation of the link the frame travels on.
    link_generation: u64,
    /// The frame itself.
    envelope: PeerEnvelope,
    /// The admission of the source of the route, if the frame advertises it.
    ///
    /// An advertisement is only sent while the source of the route is still
    /// admitted. A withdrawal or a failure is sent even when the source is
    /// gone, so a peer never keeps a route the node cannot serve.
    source: Option<AdmissionToken>,
}

/// The state of the hub.
struct HubState {
    /// The route of the node.
    route: RouteState,
    /// The live peer links of the node.
    links: HashMap<NodeId, Link>,
    /// The advertisement counter of every link.
    counters: HashMap<NodeId, Counter>,
    /// The owners of the admitted requests, by the id they carry upstream.
    owners: HashMap<String, Owner>,
    /// The upstream id of every forwarded request, by the id it carried
    /// downstream.
    downstream: HashMap<DownstreamKey, String>,
    /// The virtual worker of the current epoch.
    worker: Option<VirtualWorker>,
}

impl HubState {
    /// A fresh state of a node.
    fn new(local: NodeId) -> Self {
        Self {
            route: RouteState::new(local),
            links: HashMap::new(),
            counters: HashMap::new(),
            owners: HashMap::new(),
            downstream: HashMap::new(),
            worker: None,
        }
    }

    /// The next advertisement counter of a link.
    ///
    /// The counter belongs to the generation of the link: a link that is
    /// replaced starts over, because the peer of the link scopes the counter
    /// of its advertisements by its own link generation as well. A replaced
    /// link never learned the route of the node, so its counter starts
    /// without a learned route as well.
    fn next_counter(&mut self, peer: NodeId, link_generation: u64) -> u64 {
        let counter = self.counters.entry(peer).or_insert(Counter {
            link_generation,
            value: 0,
            learned: false,
        });
        if counter.link_generation != link_generation {
            counter.link_generation = link_generation;
            counter.value = 0;
            counter.learned = false;
        }
        counter.value += 1;
        counter.value
    }

    /// The link of a peer, if it is still the link of the given generation.
    ///
    /// A link that was replaced is not the link of the old generation any
    /// more, so a frame of the old generation finds no link to travel on.
    fn link_of(&self, peer: &NodeId, link_generation: u64) -> Option<Link> {
        self.links
            .get(peer)
            .filter(|link| link.generation == link_generation)
            .cloned()
    }

    /// Whether the peer of a link learned the route of the node.
    fn learned_route(&self, peer: NodeId) -> bool {
        self.counters
            .get(&peer)
            .is_some_and(|counter| counter.learned)
    }

    /// Note whether the peer of a link learned the route of the node.
    fn set_learned_route(&mut self, peer: NodeId, learned: bool) {
        if let Some(counter) = self.counters.get_mut(&peer) {
            counter.learned = learned;
        }
    }
}

/// A decision of the actor.
///
/// Every command carries the answer of the actor, so a caller learns the
/// outcome of its decision. The decision itself never depends on the answer:
/// a caller that abandons its reply does not cancel the work of the actor.
enum Command {
    /// Register a live peer link.
    RegisterLink {
        /// The authority of the link.
        token: AdmissionToken,
        /// The sender of the envelopes of the link.
        tx: EnvelopeSender,
        /// The stop signal of the session of the link.
        shutdown: watch::Sender<bool>,
        /// The answer of the actor.
        reply: oneshot::Sender<bool>,
    },
    /// Release the link of a generation that ended.
    ReleaseLink {
        /// The peer the link belongs to.
        peer: NodeId,
        /// The generation of the link that ended.
        generation: u64,
        /// The answer of the actor.
        reply: oneshot::Sender<()>,
    },
    /// Apply a route advertisement of a peer.
    ApplyUpdate {
        /// The authority of the link the advertisement arrived on.
        token: AdmissionToken,
        /// The encoded advertisement of the peer.
        ///
        /// The encoded frame is the unit of the control budget: the actor
        /// decodes it when it applies it, so the mailbox never holds a
        /// decoded copy of a snapshot.
        bytes: Vec<u8>,
        /// The byte budget of the queued command.
        ///
        /// The permit travels with the command, so the budget of a snapshot
        /// that waits in the control path is held until the command retires,
        /// even when the caller abandons its answer.
        permit: OwnedSemaphorePermit,
        /// The answer of the actor.
        reply: oneshot::Sender<Result<RouteOutcome, RouteError>>,
    },
    /// Apply the withdrawal of the route of a peer.
    ApplyWithdraw {
        /// The authority of the link the withdrawal arrived on.
        token: AdmissionToken,
        /// The revision of the withdrawal.
        generation: u64,
        /// The answer of the actor.
        reply: oneshot::Sender<Result<RouteOutcome, RouteError>>,
    },
    /// Forward the start of a request that arrived from a peer.
    ForwardStart {
        /// The authority of the link the request arrived on.
        token: AdmissionToken,
        /// The path of the request.
        path: PeerPath,
        /// The message of the request.
        message: BridgeMessage,
        /// The byte budget of the queued command.
        permit: OwnedSemaphorePermit,
        /// The answer of the actor.
        reply: oneshot::Sender<anyhow::Result<()>>,
    },
    /// Handle a bridge message that arrived from a peer.
    HandlePeerMessage {
        /// The authority of the link the message arrived on.
        token: AdmissionToken,
        /// The message of the peer.
        message: BridgeMessage,
        /// The byte budget of the queued command.
        permit: OwnedSemaphorePermit,
        /// The answer of the actor.
        reply: oneshot::Sender<()>,
    },
    /// Forward a message of the relay state of this node.
    ForwardLocal {
        /// The worker of the relay state that produced the message.
        ///
        /// The worker and the epoch are captured with the command, so work
        /// of a worker that was replaced is retired instead of executed on
        /// the route of the current worker.
        worker_id: usize,
        /// The epoch of the route the worker served.
        epoch: u64,
        /// The message of the relay state.
        message: BridgeMessage,
        /// The fence of a start of the relay state.
        ///
        /// The fence is set when the start was aborted while it waited in
        /// the ingress, so the actor never creates an owner for it.
        fence: Option<Arc<StartFence>>,
        /// The byte budget of the queued command.
        permit: OwnedSemaphorePermit,
    },
    /// Retire the request of a peer whose frame the ingress refused.
    AbortPeer {
        /// The authority of the link the frame arrived on.
        token: AdmissionToken,
        /// The id of the request the frame belongs to.
        request_id: String,
        /// The variant of the refused frame.
        ///
        /// The variant decides whether the frame is valid for the direction
        /// it travels in, exactly as the normal dispatch decides.
        frame: FrameVariant,
        /// The answer of the actor.
        reply: oneshot::Sender<Retirement>,
    },
    /// Retire a local request whose continuation the ingress refused.
    AbortLocal {
        /// The worker of the relay state that produced the message.
        worker_id: usize,
        /// The epoch of the route the worker served.
        epoch: u64,
        /// The kind of the request that is retired.
        kind: RequestKind,
        /// The id of the request that is retired.
        request_id: String,
        /// The answer of the actor.
        reply: oneshot::Sender<()>,
    },
    /// Fail a local request that could not be admitted.
    FailLocal {
        /// The worker of the relay state that produced the message.
        worker_id: usize,
        /// The epoch of the route the worker served.
        epoch: u64,
        /// The id of the request.
        request_id: String,
        /// The status of the failure.
        status: u16,
        /// The code of the failure.
        code: String,
        /// The answer of the actor.
        reply: oneshot::Sender<()>,
    },
    /// Invalidate the link of a generation that failed.
    InvalidateLink {
        /// The peer the link belongs to.
        peer: NodeId,
        /// The generation of the link that failed.
        generation: u64,
        /// The answer of the actor.
        reply: oneshot::Sender<()>,
    },
    /// Invalidate the virtual worker that ended.
    InvalidateWorker {
        /// The worker id of the relay state.
        worker_id: usize,
        /// The answer of the actor.
        reply: oneshot::Sender<()>,
    },
    /// The virtual worker of the route could not take the traffic of the
    /// relay state.
    WorkerFailed {
        /// The worker id of the relay state.
        worker_id: usize,
        /// The epoch of the route the worker served.
        epoch: u64,
        /// The answer of the actor.
        reply: oneshot::Sender<()>,
    },
    /// Stop the actor.
    Stop {
        /// The answer of the actor.
        reply: oneshot::Sender<()>,
    },
    /// The database node of the route of the node.
    #[cfg(test)]
    Terminal {
        /// The answer of the actor.
        reply: oneshot::Sender<Option<NodeId>>,
    },
    /// How many requests the node owns.
    #[cfg(test)]
    OwnerCount {
        /// The answer of the actor.
        reply: oneshot::Sender<usize>,
    },
    /// The epoch of the selected route.
    #[cfg(test)]
    Epoch {
        /// The answer of the actor.
        reply: oneshot::Sender<u64>,
    },
    /// The selected route of the node.
    #[cfg(test)]
    SelectedPath {
        /// The answer of the actor.
        reply: oneshot::Sender<Option<Vec<NodeId>>>,
    },
    /// Fill the owners of the node, so the bound of the owners is reached.
    #[cfg(test)]
    FillOwners {
        /// How many owners the node holds.
        count: usize,
        /// The answer of the actor.
        reply: oneshot::Sender<()>,
    },
    /// The advertisement counters the node holds.
    #[cfg(test)]
    CounterCount {
        /// The answer of the actor.
        reply: oneshot::Sender<usize>,
    },
}

impl Command {
    /// Take the message out of a command that carries one.
    ///
    /// A command that the ingress of the node refused gives its message back,
    /// so the caller can name the request the frame belongs to.
    fn take_message(self) -> Option<BridgeMessage> {
        match self {
            Command::HandlePeerMessage { message, .. } => Some(message),
            _ => None,
        }
    }
}

/// The result of one attempt to admit a command into the ingress.
enum Attempt {
    /// The command entered the ingress.
    Queued,
    /// The ingress was full and the command was not admitted.
    Full(Command),
    /// The ingress is closed and the command was not admitted.
    Closed(Command),
}

/// What may stop the admission of a command.
enum AdmissionGuard<'a> {
    /// The command of a peer: the token of its link decides.
    Peer(&'a AdmissionToken),
    /// The command of the relay state: the failure watch of its worker decides.
    Local(&'a watch::Receiver<()>),
}

/// Why the guard of an admission stopped it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stop {
    /// The link of the peer is no longer admitted.
    Stale,
    /// The worker of the route failed.
    Failed,
    /// The failure watch of the worker ended.
    Ended,
}

/// How the ingress of the node admitted a command.
enum Admission {
    /// The command entered the ingress.
    Queued,
    /// The ingress was full and the command was not admitted.
    Full(Command),
    /// The ingress is closed and the command was not admitted.
    Closed(Command),
    /// The guard of the admission stopped it.
    Stopped(Command, Stop),
}

/// Admit a command into the ingress of the node.
///
/// The admission is bounded: one initial try and at most four further tries,
/// each preceded by one cooperative yield. A full ingress keeps the original
/// command and its permit, so a retry never rebuilds the message and never
/// reserves the byte budget a second time. The yield is a best effort of the
/// scheduler and never a promise that the consumer of the ingress runs.
///
/// The guard is checked before every attempt, so a command is never sent
/// after its link ended or after its worker failed. The check of the failure
/// watch reads its state without marking it as seen, so a failure that is
/// pending is never lost, and the end of the watch is told apart from a
/// failure notification.
async fn admit_into_ingress<A>(
    mut command: Command,
    mut attempt: A,
    guard: &AdmissionGuard<'_>,
) -> Admission
where
    A: FnMut(Command) -> Attempt,
{
    for tries in 0..=PEER_INGRESS_RECOVERY_YIELDS {
        match guard {
            AdmissionGuard::Peer(token) => {
                if !token.is_live() {
                    return Admission::Stopped(command, Stop::Stale);
                }
            }
            AdmissionGuard::Local(failure) => match failure.has_changed() {
                Ok(true) => return Admission::Stopped(command, Stop::Failed),
                Ok(false) => {}
                Err(_) => {
                    // The watch is closed. A failure that was notified but
                    // never seen still counts, so it is never lost.
                    return if failure.borrow().has_changed() {
                        Admission::Stopped(command, Stop::Failed)
                    } else {
                        Admission::Stopped(command, Stop::Ended)
                    };
                }
            },
        }
        match attempt(command) {
            Attempt::Queued => return Admission::Queued,
            Attempt::Full(unsent) => {
                command = unsent;
                if tries == PEER_INGRESS_RECOVERY_YIELDS {
                    return Admission::Full(command);
                }
                tokio::task::yield_now().await;
            }
            Attempt::Closed(unsent) => return Admission::Closed(unsent),
        }
    }
    Admission::Full(command)
}

/// The forwarding hub of a database-less node.
pub(crate) struct RouteHub {
    /// The relay state of this node.
    state: AppState,
    /// The identity of this node.
    local: NodeId,
    /// The byte budget of the traffic that waits for the actor.
    ingress_budget: usize,
    /// The byte budget of the route controls that wait for the actor.
    control_budget: usize,
    /// The actor of the hub.
    actor: Mutex<Option<Arc<ActorHandle>>>,
}

/// The handles of the actor of a hub.
struct ActorHandle {
    /// The sender of the lifecycle and route commands.
    control: mpsc::Sender<Command>,
    /// The sender of the traffic commands.
    data: mpsc::Sender<Command>,
    /// The byte budget of the traffic commands.
    data_budget: Arc<Semaphore>,
    /// The byte budget of the route controls.
    control_budget: Arc<Semaphore>,
    /// The task of the actor.
    task: Mutex<Option<JoinHandle<()>>>,
}

impl RouteHub {
    /// Build the hub of a node.
    ///
    /// The actor is not started here: the constructor of the hub is used
    /// outside a runtime, so the actor is created by the first command or by
    /// the startup of the node.
    pub(crate) fn new(local: NodeId, state: AppState) -> Arc<Self> {
        Self::with_budgets(
            local,
            state,
            PEER_INGRESS_BUDGET_BYTES,
            PEER_CONTROL_BUDGET_BYTES,
        )
    }

    /// Build the hub of a node with explicit budgets of the ingress.
    fn with_budgets(
        local: NodeId,
        state: AppState,
        ingress_budget: usize,
        control_budget: usize,
    ) -> Arc<Self> {
        Arc::new(Self {
            state,
            local,
            ingress_budget,
            control_budget,
            actor: Mutex::new(None),
        })
    }

    /// Start the actor of the hub.
    ///
    /// The startup of the node calls this, so the actor exists before the
    /// first peer connects. The call is idempotent.
    pub(crate) async fn start(&self) -> anyhow::Result<()> {
        self.ensure_actor().await.map(|_| ())
    }

    /// The actor of the hub, started on the first use.
    async fn ensure_actor(&self) -> anyhow::Result<Arc<ActorHandle>> {
        let mut guard = self.actor.lock().await;
        if let Some(handle) = guard.as_ref() {
            return Ok(handle.clone());
        }
        let (control, control_rx) = mpsc::channel(PEER_CONTROL_BUFFER);
        let (data, data_rx) = mpsc::channel(PEER_INGRESS_BUFFER);
        let data_budget = Arc::new(Semaphore::new(self.ingress_budget));
        let control_budget = Arc::new(Semaphore::new(self.control_budget));
        let task = tokio::spawn(actor_loop(
            self.state.clone(),
            self.local,
            control_rx,
            data_rx,
            control.clone(),
            data.clone(),
            data_budget.clone(),
            self.ingress_budget,
        ));
        let handle = Arc::new(ActorHandle {
            control,
            data,
            data_budget,
            control_budget,
            task: Mutex::new(Some(task)),
        });
        *guard = Some(handle.clone());
        Ok(handle)
    }

    /// The database node this node is pinned to.
    #[cfg(test)]
    pub(crate) async fn terminal(&self) -> Option<NodeId> {
        let handle = self.ensure_actor().await.ok()?;
        let (reply, wait) = oneshot::channel();
        handle
            .control
            .send(Command::Terminal { reply })
            .await
            .ok()?;
        wait.await.ok()?
    }

    /// How many requests the node owns at the moment.
    #[cfg(test)]
    pub(crate) async fn owner_count(&self) -> usize {
        let Ok(handle) = self.ensure_actor().await else {
            return 0;
        };
        let (reply, wait) = oneshot::channel();
        if handle
            .control
            .send(Command::OwnerCount { reply })
            .await
            .is_err()
        {
            return 0;
        }
        wait.await.unwrap_or_default()
    }

    /// The advertisement counters the node holds.
    #[cfg(test)]
    pub(crate) async fn counter_count(&self) -> usize {
        let Ok(handle) = self.ensure_actor().await else {
            return 0;
        };
        let (reply, wait) = oneshot::channel();
        if handle
            .control
            .send(Command::CounterCount { reply })
            .await
            .is_err()
        {
            return 0;
        }
        wait.await.unwrap_or_default()
    }

    /// The epoch of the selected route.
    #[cfg(test)]
    pub(crate) async fn epoch(&self) -> u64 {
        let Ok(handle) = self.ensure_actor().await else {
            return 0;
        };
        let (reply, wait) = oneshot::channel();
        if handle.control.send(Command::Epoch { reply }).await.is_err() {
            return 0;
        }
        wait.await.unwrap_or_default()
    }

    /// The selected route of the node, if it has one.
    #[cfg(test)]
    pub(crate) async fn selected_path(&self) -> Option<Vec<NodeId>> {
        let Ok(handle) = self.ensure_actor().await else {
            return None;
        };
        let (reply, wait) = oneshot::channel();
        if handle
            .control
            .send(Command::SelectedPath { reply })
            .await
            .is_err()
        {
            return None;
        }
        wait.await.unwrap_or_default()
    }

    /// Register a live peer link.
    ///
    /// The link replaces the older link to the same peer, so the route that
    /// used the older generation is gone and the node stops admitting work on
    /// it. A delayed registration of an older generation is refused: the
    /// token of that link lost its authority when the link was replaced, so
    /// it can never overwrite the link that replaced it.
    pub(crate) async fn register_link(
        self: &Arc<Self>,
        token: &AdmissionToken,
        tx: EnvelopeSender,
        shutdown: watch::Sender<bool>,
    ) -> bool {
        let Ok(handle) = self.ensure_actor().await else {
            return false;
        };
        let (reply, wait) = oneshot::channel();
        let command = Command::RegisterLink {
            token: token.clone(),
            tx,
            shutdown,
            reply,
        };
        if handle.control.send(command).await.is_err() {
            return false;
        }
        wait.await.unwrap_or(false)
    }

    /// Release a peer link.
    ///
    /// Only the link of the given generation is released, so the cleanup of a
    /// replaced link never touches the link that replaced it. The release
    /// runs even when the authority of the link already died, because the
    /// cleanup of a link belongs to its own generation alone.
    pub(crate) async fn release_link(self: &Arc<Self>, peer: NodeId, generation: u64) {
        let Ok(handle) = self.ensure_actor().await else {
            return;
        };
        let (reply, wait) = oneshot::channel();
        let command = Command::ReleaseLink {
            peer,
            generation,
            reply,
        };
        if handle.control.send(command).await.is_err() {
            return;
        }
        let _ = wait.await;
    }

    /// Apply a route advertisement of a peer.
    ///
    /// The advertisement must arrive on the link that is still admitted: a
    /// frame of a replaced link never touches the route state, and an
    /// advertisement of a link whose authority died never restores a route.
    ///
    /// The advertisement is encoded before it is budgeted, so the control
    /// path accounts the bytes that really travel over the link.
    #[cfg(test)]
    pub(crate) async fn apply_update(
        self: &Arc<Self>,
        token: &AdmissionToken,
        generation: u64,
        advertised: &PeerPath,
        snapshot: crate::protocol::ConfigSnapshot,
    ) -> Result<RouteOutcome, RouteError> {
        let envelope = PeerEnvelope::route_update(advertised.clone(), generation, &snapshot);
        let bytes = envelope.encode().map_err(|_| RouteError::StaleGeneration)?;
        self.apply_update_bytes(token, bytes).await
    }

    /// Apply the encoded route advertisement of a peer.
    ///
    /// The encoded frame is the unit of the control budget: a snapshot whose
    /// JSON is bigger than the budget of the control path still applies as
    /// long as its encoded frame fits the budget of the link.
    pub(crate) async fn apply_update_bytes(
        self: &Arc<Self>,
        token: &AdmissionToken,
        bytes: Vec<u8>,
    ) -> Result<RouteOutcome, RouteError> {
        let handle = self
            .ensure_actor()
            .await
            .map_err(|_| RouteError::StaleGeneration)?;
        // A frame that is bigger than the whole budget of the control path
        // can never be admitted, so it is refused right away instead of
        // waiting for a budget that never frees.
        let size = bytes.len().max(1);
        if size > self.control_budget {
            warn!(
                size,
                budget = self.control_budget,
                "the route advertisement of a peer exceeds the control budget"
            );
            return Err(RouteError::StaleGeneration);
        }
        // The encoded frame is byte budgeted, so a peer cannot fill the
        // memory of the node with route controls.
        let permit = handle
            .control_budget
            .clone()
            .acquire_many_owned(size as u32)
            .await
            .map_err(|_| RouteError::StaleGeneration)?;
        let (reply, wait) = oneshot::channel();
        let command = Command::ApplyUpdate {
            token: token.clone(),
            bytes,
            permit,
            reply,
        };
        if handle.control.send(command).await.is_err() {
            return Err(RouteError::StaleGeneration);
        }
        // The permit travels with the command: a caller that abandons its
        // answer never frees the budget of a snapshot that still waits in
        // the control path.
        wait.await.unwrap_or(Err(RouteError::StaleGeneration))
    }

    /// Apply the withdrawal of the route of a peer.
    ///
    /// The withdrawal must arrive on the link that is still admitted, like
    /// an advertisement does.
    pub(crate) async fn apply_withdraw(
        self: &Arc<Self>,
        token: &AdmissionToken,
        generation: u64,
    ) -> Result<RouteOutcome, RouteError> {
        let handle = self
            .ensure_actor()
            .await
            .map_err(|_| RouteError::StaleGeneration)?;
        let (reply, wait) = oneshot::channel();
        let command = Command::ApplyWithdraw {
            token: token.clone(),
            generation,
            reply,
        };
        if handle.control.send(command).await.is_err() {
            return Err(RouteError::StaleGeneration);
        }
        wait.await.unwrap_or(Err(RouteError::StaleGeneration))
    }

    /// Forward the start of a request that arrived from a peer.
    ///
    /// The path of the request must be the path of the selected route, so a
    /// request that carries a stale or a foreign path never reaches the
    /// database node. A duplicate start never replaces or cross-cancels the
    /// owner that already exists. The request must arrive on the link that
    /// is still admitted, so a delayed start of a replaced link is refused.
    pub(crate) async fn forward_start(
        self: &Arc<Self>,
        token: &AdmissionToken,
        path: PeerPath,
        message: BridgeMessage,
    ) -> anyhow::Result<()> {
        let handle = self.ensure_actor().await?;
        let state = ingress_counters(&handle.data, &handle.data_budget, self.ingress_budget);
        let frame = frame_variant(&message);
        // A closed ingress is the ordinary end of the actor, so it is never
        // a pressure of the ingress.
        if handle.data.is_closed() {
            report_ingress_refusal(IngressRefusalContext {
                source: "peer",
                reason: IngressRefusal::ChannelClosed,
                frame,
                peer: Some(token.peer()),
                generation: Some(token.generation()),
                worker: None,
                epoch: None,
                state,
                outcome: "request_refused",
            });
            return Err(anyhow!("the ingress of the node is closed"));
        }
        let permit = match handle
            .data_budget
            .clone()
            .try_acquire_many_owned(ingress_bytes(&message) as u32)
        {
            Ok(permit) => permit,
            Err(_) => {
                report_ingress_refusal(IngressRefusalContext {
                    source: "peer",
                    reason: IngressRefusal::ByteBudgetExhausted,
                    frame,
                    peer: Some(token.peer()),
                    generation: Some(token.generation()),
                    worker: None,
                    epoch: None,
                    state,
                    outcome: "request_refused",
                });
                return Err(anyhow!("the ingress of the node is exhausted"));
            }
        };
        let (reply, wait) = oneshot::channel();
        let command = Command::ForwardStart {
            token: token.clone(),
            path,
            message,
            permit,
            reply,
        };
        let outcome = admit_into_ingress(
            command,
            |command| {
                // The closed state of the ingress is checked before every
                // attempt, so a closed ingress is never a pressure.
                if handle.data.is_closed() {
                    return Attempt::Closed(command);
                }
                match handle.data.try_send(command) {
                    Ok(()) => Attempt::Queued,
                    Err(mpsc::error::TrySendError::Full(unsent)) => Attempt::Full(unsent),
                    Err(mpsc::error::TrySendError::Closed(unsent)) => Attempt::Closed(unsent),
                }
            },
            &AdmissionGuard::Peer(token),
        )
        .await;
        match outcome {
            Admission::Queued => {}
            Admission::Full(command) => {
                drop(command);
                report_ingress_refusal(IngressRefusalContext {
                    source: "peer",
                    reason: IngressRefusal::ChannelFull,
                    frame,
                    peer: Some(token.peer()),
                    generation: Some(token.generation()),
                    worker: None,
                    epoch: None,
                    state,
                    outcome: "request_refused",
                });
                return Err(anyhow!("the ingress of the node is full"));
            }
            Admission::Closed(command) => {
                drop(command);
                report_ingress_refusal(IngressRefusalContext {
                    source: "peer",
                    reason: IngressRefusal::ChannelClosed,
                    frame,
                    peer: Some(token.peer()),
                    generation: Some(token.generation()),
                    worker: None,
                    epoch: None,
                    state,
                    outcome: "request_refused",
                });
                return Err(anyhow!("the ingress of the node is closed"));
            }
            Admission::Stopped(command, _stop) => {
                // The guard of a peer only stops on a link that ended, so the
                // start is a stale no-op.
                drop(command);
                return Err(anyhow!("the link of the peer is no longer admitted"));
            }
        }
        wait.await
            .unwrap_or_else(|_| Err(anyhow!("the actor of the node is gone")))
    }

    /// Handle a bridge message that arrived from a peer.
    ///
    /// The direction of a frame is decided by the owner it belongs to, not by
    /// its type alone: a realtime close from a client is a cancellation on the
    /// way up, while the same frame from the executor is a terminal response
    /// on the way down. A frame of a link that is no longer admitted is
    /// dropped, so a delayed frame never cancels the work of the link that
    /// replaced it.
    ///
    /// The answer tells the caller whether the frame entered the ingress of
    /// the actor. A frame that does not fit the ingress is refused, so the
    /// caller can stop the link that floods the node.
    pub(crate) async fn handle_peer_message(
        self: &Arc<Self>,
        token: &AdmissionToken,
        message: BridgeMessage,
    ) -> bool {
        let Ok(handle) = self.ensure_actor().await else {
            return false;
        };
        // A closed ingress is the ordinary end of the actor, so it is never
        // a pressure of the ingress.
        if handle.data.is_closed() {
            let state = ingress_counters(&handle.data, &handle.data_budget, self.ingress_budget);
            report_ingress_refusal(IngressRefusalContext {
                source: "peer",
                reason: IngressRefusal::ChannelClosed,
                frame: frame_variant(&message),
                peer: Some(token.peer()),
                generation: Some(token.generation()),
                worker: None,
                epoch: None,
                state,
                outcome: "link_fallback",
            });
            return false;
        }
        let permit = match handle
            .data_budget
            .clone()
            .try_acquire_many_owned(ingress_bytes(&message) as u32)
        {
            Ok(permit) => permit,
            Err(_) => {
                return self
                    .refuse_peer_frame(&handle, token, message, IngressRefusal::ByteBudgetExhausted)
                    .await;
            }
        };
        let (reply, wait) = oneshot::channel();
        let command = Command::HandlePeerMessage {
            token: token.clone(),
            message,
            permit,
            reply,
        };
        let outcome = admit_into_ingress(
            command,
            |command| {
                // The closed state of the ingress is checked before every
                // attempt, so a closed ingress is never a pressure.
                if handle.data.is_closed() {
                    return Attempt::Closed(command);
                }
                match handle.data.try_send(command) {
                    Ok(()) => Attempt::Queued,
                    Err(mpsc::error::TrySendError::Full(unsent)) => Attempt::Full(unsent),
                    Err(mpsc::error::TrySendError::Closed(unsent)) => Attempt::Closed(unsent),
                }
            },
            &AdmissionGuard::Peer(token),
        )
        .await;
        match outcome {
            Admission::Queued => {
                let _ = wait.await;
                true
            }
            Admission::Full(command) => {
                let Some(message) = command.take_message() else {
                    return false;
                };
                self.refuse_peer_frame(&handle, token, message, IngressRefusal::ChannelFull)
                    .await
            }
            Admission::Closed(command) => {
                let Some(message) = command.take_message() else {
                    return false;
                };
                self.refuse_peer_frame(&handle, token, message, IngressRefusal::ChannelClosed)
                    .await
            }
            Admission::Stopped(command, _stop) => {
                // The guard of a peer only stops on a link that ended, so the
                // frame is a stale no-op like the normal dispatch.
                drop(command);
                true
            }
        }
    }

    /// Retire the request of a peer frame that the ingress of the node
    /// refused.
    ///
    /// The retirement is scoped to the request the frame belongs to: the
    /// link of the peer and the other requests of the node keep working. A
    /// frame that carries no request is ignored by the normal dispatch as
    /// well, so it is not a reason to stop the link. When the retirement
    /// cannot be scheduled or acknowledged, the link ends as before.
    async fn refuse_peer_frame(
        &self,
        handle: &ActorHandle,
        token: &AdmissionToken,
        message: BridgeMessage,
        reason: IngressRefusal,
    ) -> bool {
        let state = ingress_counters(&handle.data, &handle.data_budget, self.ingress_budget);
        let frame = frame_variant(&message);
        if reason == IngressRefusal::ChannelClosed {
            // The actor is gone, so no retirement can be scheduled. The
            // closed channel is the ordinary end of the node, not a
            // pressure of the ingress.
            report_ingress_refusal(IngressRefusalContext {
                source: "peer",
                reason,
                frame,
                peer: Some(token.peer()),
                generation: Some(token.generation()),
                worker: None,
                epoch: None,
                state,
                outcome: "link_fallback",
            });
            return false;
        }
        let request_id = request_id_of(&message).map(|id| id.to_string());
        let Some(request_id) = request_id.filter(|id| !id.is_empty() && id.len() <= MAX_ID_BYTES)
        else {
            report_ingress_refusal(IngressRefusalContext {
                source: "peer",
                reason,
                frame,
                peer: Some(token.peer()),
                generation: Some(token.generation()),
                worker: None,
                epoch: None,
                state,
                outcome: "not_request_addressable",
            });
            return true;
        };
        let (reply, wait) = oneshot::channel();
        let command = Command::AbortPeer {
            token: token.clone(),
            request_id,
            frame,
            reply,
        };
        if handle.control.try_send(command).is_err() {
            report_ingress_refusal(IngressRefusalContext {
                source: "peer",
                reason: IngressRefusal::ControlUnavailable,
                frame,
                peer: Some(token.peer()),
                generation: Some(token.generation()),
                worker: None,
                epoch: None,
                state,
                outcome: "link_fallback",
            });
            return false;
        }
        match wait.await {
            Ok(Retirement::Retired) => {
                report_ingress_refusal(IngressRefusalContext {
                    source: "peer",
                    reason,
                    frame,
                    peer: Some(token.peer()),
                    generation: Some(token.generation()),
                    worker: None,
                    epoch: None,
                    state,
                    outcome: "scoped_retirement",
                });
                true
            }
            Ok(Retirement::NoOwner) => {
                report_ingress_refusal(IngressRefusalContext {
                    source: "peer",
                    reason,
                    frame,
                    peer: Some(token.peer()),
                    generation: Some(token.generation()),
                    worker: None,
                    epoch: None,
                    state,
                    outcome: "no_owner_noop",
                });
                true
            }
            Err(_) => {
                report_ingress_refusal(IngressRefusalContext {
                    source: "peer",
                    reason: IngressRefusal::ControlUnavailable,
                    frame,
                    peer: Some(token.peer()),
                    generation: Some(token.generation()),
                    worker: None,
                    epoch: None,
                    state,
                    outcome: "link_fallback",
                });
                false
            }
        }
    }

    /// Invalidate the link of a generation that failed.
    ///
    /// The link loses its authority and its generation cleanup runs, so a
    /// link that cannot carry its traffic never keeps the state of the node.
    pub(crate) async fn invalidate_link(
        self: &Arc<Self>,
        peer: NodeId,
        generation: u64,
    ) -> anyhow::Result<()> {
        let handle = self.ensure_actor().await?;
        let (reply, wait) = oneshot::channel();
        let command = Command::InvalidateLink {
            peer,
            generation,
            reply,
        };
        handle.control.send(command).await?;
        let _ = wait.await;
        Ok(())
    }

    /// Stop the actor of the hub and join it.
    ///
    /// The actor owns the forwarding state of the node, so it is stopped
    /// after the peer sessions of the node are gone. The actor never stops
    /// itself, so the task is joined here.
    pub(crate) async fn stop(&self) {
        let handle = { self.actor.lock().await.clone() };
        let Some(handle) = handle else {
            return;
        };
        let (reply, wait) = oneshot::channel();
        if handle.control.send(Command::Stop { reply }).await.is_err() {
            return;
        }
        let _ = wait.await;
        let task = { handle.task.lock().await.take() };
        if let Some(task) = task {
            let _ = task.await;
        }
    }
}

/// Run the actor of a hub.
///
/// The actor owns the forwarding state of the node. It reads the control
/// path first, so the cleanup of a link is never starved by the traffic, and
/// it never waits for a peer: every decision it applies is complete on its
/// own and the answer of the caller only reports the outcome.
async fn actor_loop(
    state: AppState,
    local: NodeId,
    mut control: mpsc::Receiver<Command>,
    mut data: mpsc::Receiver<Command>,
    control_tx: mpsc::Sender<Command>,
    data_tx: mpsc::Sender<Command>,
    data_budget: Arc<Semaphore>,
    ingress_total: usize,
) {
    let mut actor = Actor {
        state,
        inner: HubState::new(local),
        control_tx,
        data_tx,
        data_budget,
        ingress_total,
    };
    loop {
        tokio::select! {
            biased;
            command = control.recv() => {
                let Some(command) = command else { break };
                if actor.handle_control(command).await {
                    break;
                }
            }
            command = data.recv() => {
                let Some(command) = command else { break };
                actor.handle_data(command).await;
            }
        }
    }
    actor.shutdown().await;
}

/// The actor that owns the forwarding state of a node.
struct Actor {
    /// The relay state of this node.
    state: AppState,
    /// The forwarding state of the node.
    inner: HubState,
    /// The sender of the control commands, used by the producer of the
    /// virtual worker.
    control_tx: mpsc::Sender<Command>,
    /// The sender of the traffic commands, used by the producer of the
    /// virtual worker.
    data_tx: mpsc::Sender<Command>,
    /// The byte budget of the traffic commands.
    data_budget: Arc<Semaphore>,
    /// The byte budget of the ingress, as a total.
    ///
    /// The total belongs to the ingress of the node, so the diagnostics of a
    /// refusal report the budget the node holds and not only what is left of
    /// it.
    ingress_total: usize,
}

impl Actor {
    /// Handle a command of the control path.
    ///
    /// The answer of the actor reports whether it must stop.
    async fn handle_control(&mut self, command: Command) -> bool {
        match command {
            Command::RegisterLink {
                token,
                tx,
                shutdown,
                reply,
            } => {
                let accepted = self.register_link(token, tx, shutdown).await;
                let _ = reply.send(accepted);
            }
            Command::ReleaseLink {
                peer,
                generation,
                reply,
            } => {
                self.release_link(peer, generation).await;
                let _ = reply.send(());
            }
            Command::ApplyUpdate {
                token,
                bytes,
                permit,
                reply,
            } => {
                let result = self.apply_update(token, bytes).await;
                // The budget of the snapshot is held until the command
                // retires, so a caller that abandons its answer never frees
                // the budget of a snapshot that still waits.
                drop(permit);
                let _ = reply.send(result);
            }
            Command::ApplyWithdraw {
                token,
                generation,
                reply,
            } => {
                let result = self.apply_withdraw(token, generation).await;
                let _ = reply.send(result);
            }
            Command::FailLocal {
                worker_id,
                epoch,
                request_id,
                status,
                code,
                reply,
            } => {
                if self.worker_command_is_current(worker_id, epoch) {
                    self.fail_local(&request_id, status, &code).await;
                }
                let _ = reply.send(());
            }
            Command::InvalidateLink {
                peer,
                generation,
                reply,
            } => {
                self.stop_link(peer, generation).await;
                let _ = reply.send(());
            }
            Command::InvalidateWorker { worker_id, reply } => {
                self.invalidate_worker(worker_id).await;
                let _ = reply.send(());
            }
            Command::WorkerFailed {
                worker_id,
                epoch,
                reply,
            } => {
                self.fail_worker(worker_id, epoch).await;
                let _ = reply.send(());
            }
            Command::AbortPeer {
                token,
                request_id,
                frame,
                reply,
            } => {
                let outcome = self.abort_peer(&token, &request_id, frame).await;
                let _ = reply.send(outcome);
            }
            Command::AbortLocal {
                worker_id,
                epoch,
                kind,
                request_id,
                reply,
            } => {
                // The retirement belongs to the worker that produced it, so
                // the work of a worker that was replaced is left alone.
                if self.worker_command_is_current(worker_id, epoch) {
                    self.abort_local(&request_id, kind).await;
                }
                let _ = reply.send(());
            }
            Command::Stop { reply } => {
                let _ = reply.send(());
                return true;
            }
            #[cfg(test)]
            Command::Terminal { reply } => {
                let _ = reply.send(self.inner.route.terminal());
            }
            #[cfg(test)]
            Command::OwnerCount { reply } => {
                let _ = reply.send(self.inner.owners.len());
            }
            #[cfg(test)]
            Command::CounterCount { reply } => {
                let _ = reply.send(self.inner.counters.len());
            }
            #[cfg(test)]
            Command::Epoch { reply } => {
                let _ = reply.send(self.inner.route.epoch());
            }
            #[cfg(test)]
            Command::SelectedPath { reply } => {
                let _ = reply.send(
                    self.inner
                        .route
                        .selected()
                        .map(|selected| selected.path.nodes().to_vec()),
                );
            }
            #[cfg(test)]
            Command::FillOwners { count, reply } => {
                let epoch = self.inner.route.epoch();
                for index in 0..count {
                    self.inner.owners.insert(
                        format!("filler-{index}"),
                        Owner {
                            kind: RequestKind::Http,
                            epoch,
                            upstream_peer: self.inner.route.local(),
                            upstream_generation: 1,
                            downstream: None,
                        },
                    );
                }
                let _ = reply.send(());
            }
            Command::ForwardStart { .. }
            | Command::HandlePeerMessage { .. }
            | Command::ForwardLocal { .. } => {}
        }
        false
    }

    /// Whether work of the virtual worker still belongs to the current route.
    ///
    /// The command carries the worker and the epoch it was produced for, so
    /// queued work of a worker that was replaced is retired instead of
    /// running on the route of the current worker.
    fn worker_command_is_current(&self, worker_id: usize, epoch: u64) -> bool {
        self.inner
            .worker
            .as_ref()
            .is_some_and(|worker| worker.id == worker_id && worker.epoch == epoch)
            && self.inner.route.epoch() == epoch
    }

    /// Handle a command of the traffic path.
    async fn handle_data(&mut self, command: Command) {
        match command {
            Command::ForwardStart {
                token,
                path,
                message,
                permit,
                reply,
            } => {
                let result = self.forward_start(token, path, message).await;
                drop(permit);
                let _ = reply.send(result);
            }
            Command::HandlePeerMessage {
                token,
                message,
                permit,
                reply,
            } => {
                self.handle_peer_message(token, message).await;
                drop(permit);
                let _ = reply.send(());
            }
            Command::ForwardLocal {
                worker_id,
                epoch,
                message,
                fence,
                permit,
            } => {
                if self.worker_command_is_current(worker_id, epoch) {
                    self.forward_local(message, fence).await;
                } else {
                    // The work belongs to a worker or a route that is gone.
                    // The pending request of the relay state was already
                    // failed with its worker, so the command is retired
                    // instead of running on the route of the current worker.
                    debug!(
                        worker_id,
                        epoch, "the local work of a replaced worker was retired"
                    );
                }
                drop(permit);
            }
            _ => {}
        }
    }

    /// Register a live peer link.
    async fn register_link(
        &mut self,
        token: AdmissionToken,
        tx: EnvelopeSender,
        shutdown: watch::Sender<bool>,
    ) -> bool {
        let peer = token.peer();
        let generation = token.generation();
        if !token.is_live() {
            warn!(peer = %peer, generation, "a replaced peer session tried to register");
            return false;
        }
        if self
            .inner
            .links
            .get(&peer)
            .is_some_and(|link| link.generation > generation)
        {
            warn!(peer = %peer, generation, "a replaced peer session tried to register");
            return false;
        }
        self.inner.links.insert(
            peer,
            Link {
                peer,
                generation,
                tx,
                shutdown,
                token: token.clone(),
            },
        );
        let stale = self.inner.route.selected().is_some_and(|selected| {
            selected.next_hop == peer && selected.next_link_generation != generation
        });
        let outcome = if stale {
            let old = self
                .inner
                .route
                .selected()
                .map(|selected| selected.next_link_generation)
                .unwrap_or_default();
            self.inner.route.release_link(peer, old)
        } else {
            RouteOutcome::Unchanged
        };
        self.after_change(outcome).await;
        // A link that arrives after the route is known learns the route right
        // away, so a peer that connects late is never left without a route.
        if let Source::Token(token) = self.selected_source() {
            self.advertise(token);
        }
        true
    }

    /// Release a peer link.
    async fn release_link(&mut self, peer: NodeId, generation: u64) {
        let shutdown = if self
            .inner
            .links
            .get(&peer)
            .is_some_and(|link| link.generation == generation)
        {
            self.inner.links.remove(&peer).map(|link| link.shutdown)
        } else {
            None
        };
        // The advertisement counter belongs to the generation of the link, so
        // the counter of a link that ends is removed with it. The counter of
        // a newer generation is never removed, and the removal is idempotent.
        self.remove_counter(peer, generation);
        let outcome = self.inner.route.release_link(peer, generation);
        if let Some(shutdown) = shutdown {
            let _ = shutdown.send(true);
        }
        self.fail_link_owners(peer, generation).await;
        self.after_change(outcome).await;
    }

    /// Stop the link of a generation that failed.
    ///
    /// The link loses its authority before it is told to stop, and the
    /// cleanup of its generation runs, so a link that cannot carry its
    /// traffic never keeps the state of the node.
    ///
    /// The cleanup holds no history: every step of it is scoped to the
    /// generation, so a repeated cleanup of the same generation finds
    /// nothing left to do and a cleanup of an old generation never touches
    /// the link that replaced it.
    async fn stop_link(&mut self, peer: NodeId, generation: u64) {
        self.detach_link(peer, generation);
        let outcome = self.inner.route.release_link(peer, generation);
        self.fail_link_owners(peer, generation).await;
        self.after_change(outcome).await;
    }

    /// Detach a link from the node.
    ///
    /// The link loses its authority and its session is told to stop, so the
    /// cleanup of its generation runs when the session releases it. The
    /// detach never sends a frame, so it never waits for a peer.
    fn detach_link(&mut self, peer: NodeId, generation: u64) -> Option<Link> {
        let link = if self
            .inner
            .links
            .get(&peer)
            .is_some_and(|link| link.generation == generation)
        {
            self.inner.links.remove(&peer)
        } else {
            None
        };
        if let Some(link) = link {
            // The actor is the sole owner of the state of the node, so it
            // marks the link dead without waiting for the fence of a
            // publication it may hold itself.
            link.token.mark_dead();
            let _ = link.shutdown.send(true);
            // The advertisement counter belongs to the generation of the
            // link, so it ends with the link it belongs to.
            self.remove_counter(peer, generation);
            return Some(link);
        }
        None
    }

    /// Remove the advertisement counter of a link generation.
    ///
    /// The counter belongs to the generation of the link, so the counter of a
    /// newer generation is never removed. The removal is idempotent, so a
    /// repeated cleanup of a generation changes nothing. The counters
    /// therefore stay bounded by the peers that are live.
    fn remove_counter(&mut self, peer: NodeId, generation: u64) {
        if self
            .inner
            .counters
            .get(&peer)
            .is_some_and(|counter| counter.link_generation == generation)
        {
            self.inner.counters.remove(&peer);
        }
    }

    /// Invalidate the virtual worker that ended.
    async fn invalidate_worker(&mut self, worker_id: usize) {
        let Some(worker) = self.inner.worker.take() else {
            return;
        };
        if worker.id != worker_id {
            // The worker of the node is another one, so it stays.
            self.inner.worker = Some(worker);
            return;
        }
        worker.task.abort();
        let _ = worker.task.await;
        cleanup_worker(&self.state, worker.id).await;
        let owners = std::mem::take(&mut self.inner.owners);
        self.inner.downstream.clear();
        for (id, owner) in owners {
            self.fail_owner(&id, owner).await;
        }
    }

    /// Apply a route advertisement of a peer.
    ///
    /// The advertisement is decoded here, so the actor is the only authority
    /// of what a peer advertised. The candidate of the peer is mutated under
    /// the fence of the admission the advertisement arrived on, so an
    /// advertisement of a replaced link never touches the route state.
    async fn apply_update(
        &mut self,
        token: AdmissionToken,
        bytes: Vec<u8>,
    ) -> Result<RouteOutcome, RouteError> {
        let peer = token.peer();
        let link_generation = token.generation();
        let outcome = {
            // The guard is taken before the liveness is checked, so an
            // invalidation that is already on its way linearizes before
            // this advertisement touches the route state.
            let _guard = token.read_publication().await;
            if !token.is_live() {
                debug!(peer = %peer, link_generation, "a route advertisement of a replaced link was refused");
                return Err(RouteError::StaleGeneration);
            }
            // The frame must arrive on the link that is live: a frame of a
            // replaced link never touches the route state.
            if !self
                .inner
                .links
                .get(&peer)
                .is_some_and(|link| link.generation == link_generation)
            {
                return Err(RouteError::StaleGeneration);
            }
            let envelope = PeerEnvelope::decode(&bytes).map_err(|_| RouteError::StaleGeneration)?;
            if envelope.kind != PeerEnvelopeKind::RouteUpdate {
                return Err(RouteError::StaleGeneration);
            }
            let Some(advertised) = envelope.path.clone() else {
                return Err(RouteError::StaleGeneration);
            };
            let Some(snapshot) = envelope.snapshot().cloned() else {
                return Err(RouteError::StaleGeneration);
            };
            if let Some(previous) = self.inner.route.candidate(peer) {
                debug!(
                    peer = %peer,
                    generation = previous.generation,
                    link_generation = previous.link_generation,
                    "a peer advertised a new route"
                );
            }
            self.inner.route.apply_update(
                peer,
                link_generation,
                envelope.generation,
                &advertised,
                snapshot,
            )?
        };
        self.after_change(outcome).await;
        Ok(outcome)
    }

    /// Apply the withdrawal of the route of a peer.
    async fn apply_withdraw(
        &mut self,
        token: AdmissionToken,
        generation: u64,
    ) -> Result<RouteOutcome, RouteError> {
        let peer = token.peer();
        let link_generation = token.generation();
        let outcome = {
            // The guard is taken before the liveness is checked, so a
            // withdrawal of a replaced link never touches the route state.
            let _guard = token.read_publication().await;
            if !token.is_live() {
                debug!(peer = %peer, link_generation, "a route withdrawal of a replaced link was refused");
                return Err(RouteError::StaleGeneration);
            }
            if !self
                .inner
                .links
                .get(&peer)
                .is_some_and(|link| link.generation == link_generation)
            {
                return Err(RouteError::StaleGeneration);
            }
            self.inner
                .route
                .apply_withdraw(peer, link_generation, generation)?
        };
        self.after_change(outcome).await;
        Ok(outcome)
    }

    /// Forward the start of a request that arrived from a peer.
    async fn forward_start(
        &mut self,
        token: AdmissionToken,
        path: PeerPath,
        message: BridgeMessage,
    ) -> anyhow::Result<()> {
        let peer = token.peer();
        let link_generation = token.generation();
        if !token.is_live() {
            return Err(anyhow!("the link of the peer is no longer admitted"));
        }
        let kind = match classify(&message) {
            MessageRole::Start(kind) => kind,
            _ => return Err(anyhow!("a request start must start a request")),
        };
        let downstream_id = request_id_of(&message)
            .ok_or_else(|| anyhow!("a request start must carry a request id"))?
            .to_string();
        if downstream_id.is_empty() || downstream_id.len() > MAX_ID_BYTES {
            return Err(anyhow!("the request id of a peer request is invalid"));
        }
        let (link, envelope) = {
            if !self
                .inner
                .links
                .get(&peer)
                .is_some_and(|link| link.generation == link_generation)
            {
                return Err(anyhow!("the link of the peer is not live"));
            }
            let key = DownstreamKey {
                peer,
                link_generation,
                request_id: downstream_id.clone(),
            };
            if self.inner.downstream.contains_key(&key) {
                // The duplicate is dropped with its own id: answering it would
                // cross-cancel the request that already owns that id.
                warn!(request_id = %downstream_id, "a peer repeated a live request start");
                return Ok(());
            }
            self.inner.route.accept_start_path(&path, peer)?;
            let selected = self
                .inner
                .route
                .selected()
                .ok_or_else(|| anyhow!("the node has no route"))?
                .clone();
            let link = self
                .inner
                .links
                .get(&selected.next_hop)
                .cloned()
                .ok_or_else(|| anyhow!("the next hop of the route is not connected"))?;
            if link.generation != selected.next_link_generation {
                return Err(anyhow!("the next hop of the route was replaced"));
            }
            if self.inner.owners.len() >= MAX_FORWARDED_REQUESTS {
                return Err(anyhow!("the node forwards too many requests"));
            }
            let upstream_id = self.fresh_upstream_id()?;
            let mut message = message;
            set_request_id(&mut message, &upstream_id);
            self.inner.owners.insert(
                upstream_id.clone(),
                Owner {
                    kind,
                    epoch: selected.epoch,
                    upstream_peer: selected.next_hop,
                    upstream_generation: selected.next_link_generation,
                    downstream: Some(Downstream {
                        peer,
                        link_generation,
                        request_id: downstream_id.clone(),
                    }),
                },
            );
            self.inner.downstream.insert(key, upstream_id);
            (link, PeerEnvelope::request_start(path, message))
        };
        if self
            .send_to_peer(link.peer, link.generation, envelope)
            .is_err()
        {
            let removed = self
                .remove_owner_by_downstream(peer, link_generation, &downstream_id)
                .await;
            drop(removed);
            return Err(anyhow!(
                "the next hop of the route did not accept the start"
            ));
        }
        Ok(())
    }

    /// Handle a bridge message that arrived from a peer.
    async fn handle_peer_message(&mut self, token: AdmissionToken, message: BridgeMessage) {
        let peer = token.peer();
        let link_generation = token.generation();
        let Some(id) = request_id_of(&message).map(|id| id.to_string()) else {
            return;
        };
        if !token.is_live() {
            debug!(request_id = %id, "a frame of a replaced link was dropped");
            return;
        }
        let direction = {
            if !self
                .inner
                .links
                .get(&peer)
                .is_some_and(|link| link.generation == link_generation)
            {
                debug!(request_id = %id, "a frame of a link that is not live was dropped");
                return;
            }
            self.frame_direction(peer, link_generation, &id)
        };
        match direction {
            Some(Direction::Upstream) => {
                self.handle_upstream_message(peer, link_generation, id, message)
                    .await;
            }
            Some(Direction::Downstream(upstream_id)) => {
                self.forward_downstream_frame(peer, link_generation, id, upstream_id, message)
                    .await;
            }
            None => {
                debug!(request_id = %id, "a frame of an unknown request was dropped");
            }
        }
    }

    /// The direction of a frame of a peer, if the node holds a request for
    /// it.
    ///
    /// The direction belongs to the owner the node holds, never to the type
    /// of the frame: a frame of the executor of the route travels on the
    /// upstream of its owner, a frame of the peer that forwarded the request
    /// travels on its downstream.
    fn frame_direction(&self, peer: NodeId, link_generation: u64, id: &str) -> Option<Direction> {
        let hub = &self.inner;
        if hub.owners.get(id).is_some_and(|owner| {
            owner.upstream_peer == peer && owner.upstream_generation == link_generation
        }) {
            return Some(Direction::Upstream);
        }
        hub.downstream
            .get(&DownstreamKey {
                peer,
                link_generation,
                request_id: id.to_string(),
            })
            .and_then(|upstream| hub.owners.contains_key(upstream).then(|| upstream.clone()))
            .map(Direction::Downstream)
    }

    /// Handle a frame of the executor of the route.
    async fn handle_upstream_message(
        &mut self,
        peer: NodeId,
        link_generation: u64,
        id: String,
        message: BridgeMessage,
    ) {
        let kind = response_kind(&message);
        let error = matches!(message, BridgeMessage::ResponseError(_));
        let terminal = error || matches!(classify(&message), MessageRole::Terminal(_));
        if kind.is_none() && !error {
            warn!(request_id = %id, "the executor sent a frame that is not a response");
            return;
        }
        let action = {
            let hub = &self.inner;
            let Some(owner) = hub.owners.get(&id) else {
                debug!(request_id = %id, "a response of an unknown request was dropped");
                return;
            };
            if kind.is_some_and(|kind| owner.kind != kind) {
                debug!(request_id = %id, "a response of another kind was dropped");
                return;
            }
            // The response must arrive on the link the request travelled on
            // and belong to the epoch of the current route.
            if owner.upstream_peer != peer
                || owner.upstream_generation != link_generation
                || owner.epoch != hub.route.epoch()
            {
                debug!(request_id = %id, "a response of another route was dropped");
                return;
            }
            match owner.downstream.clone() {
                None => Action::Local(message),
                Some(downstream) => {
                    let mut message = message;
                    set_request_id(&mut message, &downstream.request_id);
                    Action::Downstream(downstream, message)
                }
            }
        };
        match action {
            Action::Local(message) => {
                // The shared dispatcher routes a `ResponseError` to the HTTP,
                // MCP and realtime pending request alike. The relay may wait
                // for a slow client, so the message leaves the actor.
                self.deliver(message).await;
            }
            Action::Downstream(downstream, message) => {
                let envelope = PeerEnvelope::bridge(message);
                let _ = self.send_to_peer(downstream.peer, downstream.link_generation, envelope);
            }
        }
        if terminal {
            self.finish_owner(&id).await;
        }
    }

    /// Forward a frame of a client to the executor of the route.
    async fn forward_downstream_frame(
        &mut self,
        peer: NodeId,
        link_generation: u64,
        downstream_id: String,
        upstream_id: String,
        message: BridgeMessage,
    ) {
        if matches!(classify(&message), MessageRole::Start(_)) {
            warn!(request_id = %downstream_id, "a peer sent a start outside a request start envelope");
            return;
        }
        let (link, terminal) = {
            let hub = &self.inner;
            let Some(owner) = hub.owners.get(&upstream_id) else {
                return;
            };
            let correlated = owner.downstream.as_ref().is_some_and(|downstream| {
                downstream.peer == peer
                    && downstream.link_generation == link_generation
                    && downstream.request_id == downstream_id
            });
            if !correlated || owner.epoch != hub.route.epoch() {
                debug!(request_id = %downstream_id, "a frame of another route was dropped");
                return;
            }
            let Some(link) = hub.links.get(&owner.upstream_peer).cloned() else {
                return;
            };
            if link.generation != owner.upstream_generation {
                debug!(request_id = %downstream_id, "a frame of a replaced link was dropped");
                return;
            }
            (link, is_client_terminal(&message))
        };
        let mut message = message;
        set_request_id(&mut message, &upstream_id);
        if self
            .send_to_peer(link.peer, link.generation, PeerEnvelope::bridge(message))
            .is_err()
        {
            // The cancellation or frame cannot be enqueued safely: end the
            // upstream generation so its database side cancels the work it
            // owns. The owner is kept until its healthy downstream caller is
            // failed, so the caller never waits for a terminal that cannot
            // come.
            let owner = self
                .remove_owner_by_downstream(peer, link_generation, &downstream_id)
                .await;
            if let Some(owner) = owner {
                self.fail_owner(&upstream_id, owner).await;
            }
            return;
        }
        if terminal {
            self.finish_owner(&upstream_id).await;
        }
    }

    /// Forward a message of the relay state of this node.
    ///
    /// The relay state owns the request ids of the local clients, so the id
    /// of a local request stays the same on the wire and the response of the
    /// database node reaches the pending request of the relay state.
    async fn forward_local(&mut self, message: BridgeMessage, fence: Option<Arc<StartFence>>) {
        let Some(id) = request_id_of(&message).map(|id| id.to_string()) else {
            return;
        };
        let role = match &message {
            BridgeMessage::RealtimeSessionClose(_) => MessageRole::Cancel(RequestKind::Realtime),
            _ => classify(&message),
        };
        match role {
            MessageRole::Start(kind) => {
                if fence.as_ref().is_some_and(|fence| fence.is_cancelled()) {
                    // The start was aborted while it waited in the ingress, so
                    // it never creates an owner and never reaches the
                    // executor. The abort owns the terminal delivery of the
                    // request.
                    debug!(request_id = %id, "an aborted local start was retired");
                    return;
                }
                if id.is_empty() || id.len() > MAX_ID_BYTES {
                    warn!("a local request carries an invalid request id");
                    self.fail_local(&id, 400, "peer_request_invalid").await;
                    return;
                }
                // The relay state owns the request id of the local client, so
                // the request travels upstream with that id and the answer
                // reaches the pending request of the relay state.
                let prepared = {
                    let hub = &mut self.inner;
                    let selected = hub.route.selected().cloned();
                    let link = selected
                        .as_ref()
                        .and_then(|selected| hub.links.get(&selected.next_hop).cloned());
                    let usable = selected.as_ref().is_some_and(|selected| {
                        link.as_ref()
                            .is_some_and(|link| link.generation == selected.next_link_generation)
                    });
                    if !usable {
                        Err(Some((503, "peer_no_route")))
                    } else if hub.owners.contains_key(&id) {
                        // A duplicate local id must not replace or cross-cancel
                        // the request that already runs.
                        Err(None)
                    } else if hub.owners.len() >= MAX_FORWARDED_REQUESTS {
                        Err(Some((503, "peer_overloaded")))
                    } else {
                        let selected = selected.expect("the route is usable");
                        let link = link.expect("the link is usable");
                        hub.owners.insert(
                            id.clone(),
                            Owner {
                                kind,
                                epoch: selected.epoch,
                                upstream_peer: selected.next_hop,
                                upstream_generation: selected.next_link_generation,
                                downstream: None,
                            },
                        );
                        Ok((
                            link,
                            PeerEnvelope::request_start(selected.path.clone(), message),
                        ))
                    }
                };
                let (link, envelope) = match prepared {
                    Ok(prepared) => prepared,
                    Err(Some((status, code))) => {
                        self.fail_local(&id, status, code).await;
                        return;
                    }
                    Err(None) => return,
                };
                if self
                    .send_to_peer(link.peer, link.generation, envelope)
                    .is_err()
                {
                    self.remove_owner(&id).await;
                    self.fail_local(&id, 503, "peer_link_lost").await;
                }
            }
            MessageRole::Stream(kind) | MessageRole::End(kind) | MessageRole::Cancel(kind) => {
                let prepared = {
                    let hub = &self.inner;
                    let owner = hub.owners.get(&id);
                    match owner {
                        Some(owner) if owner.downstream.is_none() && owner.kind == kind => {
                            let link = hub
                                .owners
                                .get(&id)
                                .and_then(|owner| hub.links.get(&owner.upstream_peer).cloned());
                            match link {
                                Some(link) if link.generation == owner.upstream_generation => {
                                    Some((link, is_client_terminal(&message)))
                                }
                                _ => None,
                            }
                        }
                        _ => None,
                    }
                };
                let Some((link, terminal)) = prepared else {
                    return;
                };
                if self
                    .send_to_peer(link.peer, link.generation, PeerEnvelope::bridge(message))
                    .is_err()
                {
                    self.remove_owner(&id).await;
                    self.fail_local(&id, 503, "peer_link_lost").await;
                    return;
                }
                if terminal {
                    self.finish_owner(&id).await;
                }
            }
            MessageRole::Response(_)
            | MessageRole::Terminal(_)
            | MessageRole::Error
            | MessageRole::Control => {}
        }
    }

    /// Fail a local request of the relay state.
    async fn fail_local(&mut self, id: &str, status: u16, code: &str) {
        if id.is_empty() {
            return;
        }
        let error = ResponseError {
            request_id: id.to_string(),
            status,
            code: code.to_string(),
            message: "the peer route could not serve the request".to_string(),
        };
        self.deliver(BridgeMessage::ResponseError(error)).await;
    }

    /// Fail a local request of the relay state with an overload error.
    ///
    /// The overload of the ingress is the reason of the failure, so the
    /// pending request of the relay state learns it instead of waiting for a
    /// terminal that never comes.
    async fn fail_local_overloaded(&mut self, id: &str) {
        if id.is_empty() {
            return;
        }
        let error = ResponseError {
            request_id: id.to_string(),
            status: 503,
            code: "peer_overloaded".to_string(),
            message: "the ingress of the node is overloaded".to_string(),
        };
        self.deliver(BridgeMessage::ResponseError(error)).await;
    }

    /// Hand a message to the relay state.
    ///
    /// The dispatch of a local action only touches the in-memory state of
    /// the relay: it never waits for a peer or for the capacity of a client,
    /// so the actor may await it and no local action is ever lost.
    async fn deliver(&self, message: BridgeMessage) {
        let state = self.state.clone();
        dispatch_worker_bridge_message(&state, message).await;
    }

    /// Finish a request that ended.
    async fn finish_owner(&mut self, id: &str) {
        remove_owner_entry(&mut self.inner, id);
    }

    /// Remove an owner and both of its indexes.
    async fn remove_owner(&mut self, id: &str) {
        self.finish_owner(id).await;
    }

    /// Remove the owner of a forwarded request by its downstream key.
    async fn remove_owner_by_downstream(
        &mut self,
        peer: NodeId,
        link_generation: u64,
        request_id: &str,
    ) -> Option<Owner> {
        let key = DownstreamKey {
            peer,
            link_generation,
            request_id: request_id.to_string(),
        };
        let upstream = self.inner.downstream.remove(&key)?;
        self.inner.owners.remove(&upstream)
    }

    /// Fail the requests that travelled over a link that ended.
    ///
    /// A lost upstream link fails its requests: the downstream peer or the
    /// local client learns the failure. A lost downstream link cancels only
    /// its own work on a healthy upstream and removes both of its indexes.
    async fn fail_link_owners(&mut self, peer: NodeId, generation: u64) {
        let failed = {
            let hub = &mut self.inner;
            let ids = hub
                .owners
                .iter()
                .filter(|(_, owner)| {
                    (owner.upstream_peer == peer && owner.upstream_generation == generation)
                        || owner.downstream.as_ref().is_some_and(|downstream| {
                            downstream.peer == peer && downstream.link_generation == generation
                        })
                })
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>();
            let mut failed = Vec::with_capacity(ids.len());
            for id in ids {
                if let Some(owner) = hub.owners.remove(&id) {
                    if let Some(downstream) = &owner.downstream {
                        hub.downstream.remove(&DownstreamKey {
                            peer: downstream.peer,
                            link_generation: downstream.link_generation,
                            request_id: downstream.request_id.clone(),
                        });
                    }
                    failed.push((id, owner));
                }
            }
            failed
        };
        for (id, owner) in failed {
            if owner.upstream_peer == peer && owner.upstream_generation == generation {
                let mut error = ResponseError {
                    request_id: id.clone(),
                    status: 503,
                    code: "peer_link_lost".to_string(),
                    message: "the peer link of the request ended".to_string(),
                };
                match owner.downstream {
                    Some(downstream) => {
                        error.request_id = downstream.request_id.clone();
                        let envelope = PeerEnvelope::bridge(BridgeMessage::ResponseError(error));
                        let _ = self.send_to_peer(
                            downstream.peer,
                            downstream.link_generation,
                            envelope,
                        );
                    }
                    None => {
                        self.deliver(BridgeMessage::ResponseError(error)).await;
                    }
                }
            } else {
                // The client side is gone: cancel its work on the healthy
                // upstream. If the cancellation cannot be enqueued, end that
                // generation so its database side tears the work down.
                self.cancel_upstream(&owner, &id).await;
            }
        }
    }

    /// Fail one owner of the node.
    async fn fail_owner(&mut self, id: &str, owner: Owner) {
        self.fail_owner_with(
            id,
            owner,
            503,
            "peer_link_lost",
            "the peer link of the request ended",
        )
        .await;
    }

    /// Fail one owner of the node with a terminal error.
    ///
    /// The wrapper keeps the semantics of the default failure, so only the
    /// error that reaches the caller changes.
    async fn fail_owner_with(
        &mut self,
        id: &str,
        owner: Owner,
        status: u16,
        code: &str,
        message: &str,
    ) {
        let mut error = ResponseError {
            request_id: id.to_string(),
            status,
            code: code.to_string(),
            message: message.to_string(),
        };
        match owner.downstream {
            Some(downstream) => {
                error.request_id = downstream.request_id.clone();
                let envelope = PeerEnvelope::bridge(BridgeMessage::ResponseError(error));
                if self
                    .send_to_peer(downstream.peer, downstream.link_generation, envelope)
                    .is_err()
                {
                    // The failure cannot be delivered, so the downstream
                    // generation is closed: the caller never waits for a
                    // terminal that cannot arrive.
                    self.detach_link(downstream.peer, downstream.link_generation);
                }
            }
            None => {
                self.deliver(BridgeMessage::ResponseError(error)).await;
            }
        }
    }

    /// Retire a local request whose continuation the ingress of the node
    /// refused.
    ///
    /// Only the local request of that kind on the current route is retired:
    /// a request of a peer and a request of another kind keep their owner on
    /// an id collision. The pending request of the relay state is failed even
    /// when no owner existed, so an abort that overtook its start never
    /// leaves the request hanging.
    async fn abort_local(&mut self, id: &str, kind: RequestKind) {
        let epoch = self.inner.route.epoch();
        let owner = self
            .inner
            .owners
            .get(id)
            .filter(|owner| {
                owner.downstream.is_none() && owner.kind == kind && owner.epoch == epoch
            })
            .cloned();
        if let Some(owner) = owner {
            remove_owner_entry(&mut self.inner, id);
            // The upstream work of the retired request is cancelled once, so
            // the executor stops it.
            self.cancel_upstream(&owner, id).await;
        }
        self.fail_local_overloaded(id).await;
    }

    /// Retire the request of a peer whose frame the ingress of the node
    /// refused.
    ///
    /// The direction of the request is derived again from the state of the
    /// node, exactly as the normal dispatch does: a frame never retires a
    /// request of another link, of another route or of another kind. A frame
    /// that names no request of the link retires nothing.
    async fn abort_peer(
        &mut self,
        token: &AdmissionToken,
        id: &str,
        frame: FrameVariant,
    ) -> Retirement {
        let peer = token.peer();
        let link_generation = token.generation();
        if !token.is_live() {
            return Retirement::NoOwner;
        }
        if !self
            .inner
            .links
            .get(&peer)
            .is_some_and(|link| link.generation == link_generation)
        {
            return Retirement::NoOwner;
        }
        let Some(direction) = self.frame_direction(peer, link_generation, id) else {
            return Retirement::NoOwner;
        };
        // A frame that the normal dispatch would drop retires nothing, so an
        // ingress that is full never ends a request of a valid direction.
        if !frame_accepted(&direction, frame) {
            return Retirement::NoOwner;
        }
        let owner_id = match &direction {
            Direction::Upstream => id.to_string(),
            Direction::Downstream(upstream_id) => upstream_id.clone(),
        };
        let epoch = self.inner.route.epoch();
        let surface = frame.surface();
        let owner = self
            .inner
            .owners
            .get(&owner_id)
            .filter(|owner| owner.epoch == epoch && surface.map_or(true, |kind| owner.kind == kind))
            .cloned();
        let Some(owner) = owner else {
            return Retirement::NoOwner;
        };
        remove_owner_entry(&mut self.inner, &owner_id);
        // The upstream work of the retired request is cancelled once, so the
        // executor stops it.
        self.cancel_upstream(&owner, &owner_id).await;
        // The caller of the request learns the failure.
        self.fail_owner_with(
            &owner_id,
            owner,
            503,
            "peer_overloaded",
            "the ingress of the node is overloaded",
        )
        .await;
        Retirement::Retired
    }

    /// The frame that cancels the upstream work of an owner whose client
    /// side is gone.
    fn cancel_upstream_frame(&self, owner: &Owner, upstream_id: &str) -> Option<PeerFrame> {
        let link = self.inner.links.get(&owner.upstream_peer)?;
        if link.generation != owner.upstream_generation {
            return None;
        }
        Some(PeerFrame {
            peer: link.peer,
            link_generation: link.generation,
            envelope: PeerEnvelope::bridge(cancellation(owner.kind, upstream_id)),
            source: None,
        })
    }

    /// Cancel the upstream work of an owner whose client side is gone.
    async fn cancel_upstream(&mut self, owner: &Owner, upstream_id: &str) {
        let link = {
            let hub = &self.inner;
            hub.links.get(&owner.upstream_peer).cloned()
        };
        let Some(link) = link.filter(|link| link.generation == owner.upstream_generation) else {
            return;
        };
        let envelope = PeerEnvelope::bridge(cancellation(owner.kind, upstream_id));
        if self
            .send_to_peer(link.peer, link.generation, envelope)
            .is_err()
        {
            // The link cannot carry the cancellation, so its session is told
            // to stop and its generation cleanup runs.
            self.detach_link(link.peer, link.generation);
        }
    }

    /// Tear the captured epoch down after the virtual worker failed.
    ///
    /// The worker of the relay state could not take the traffic of the node,
    /// so the epoch it served ends: the worker is released, the requests it
    /// owned are cancelled upstream and failed for their callers, and the
    /// link the traffic travelled on is stopped, so the database node cancels
    /// the work it still owns for that link. A failure of a worker that was
    /// already replaced changes nothing.
    async fn fail_worker(&mut self, worker_id: usize, epoch: u64) {
        let Some(worker) = self.inner.worker.take() else {
            return;
        };
        if worker.id != worker_id || worker.epoch != epoch {
            // The signal belongs to a worker that was replaced: the worker
            // of the current route keeps its traffic.
            self.inner.worker = Some(worker);
            return;
        }
        // The producer of the failed worker is stopped before the worker is
        // released, so it never waits for an actor that is gone. The actor
        // tears the epoch down, so it never joins its own task.
        worker.task.abort();
        let _ = worker.task.await;
        cleanup_worker(&self.state, worker.id).await;
        let owners = self.inner.owners.drain().collect::<Vec<_>>();
        self.inner.downstream.clear();
        for (id, owner) in owners {
            self.cancel_upstream(&owner, &id).await;
            if let Some(downstream) = owner.downstream {
                let error = ResponseError {
                    request_id: downstream.request_id,
                    status: 503,
                    code: "peer_worker_failed".to_string(),
                    message: "the worker of the route failed".to_string(),
                };
                let envelope = PeerEnvelope::bridge(BridgeMessage::ResponseError(error));
                let _ = self.send_to_peer(downstream.peer, downstream.link_generation, envelope);
            }
        }
        // The link the traffic travelled on is stopped, so the database node
        // cancels the work of that link. A link that was replaced in the
        // meantime keeps its own generation and is never stopped here.
        self.stop_link(worker.upstream_peer, worker.upstream_generation)
            .await;
    }

    /// Apply what a change of the route state did.
    ///
    /// The local publication of the change runs under the fence of the
    /// admission that carries the traffic of the selected route: a replaced
    /// upstream never publishes a snapshot, a worker or a readiness after
    /// its admission died. The frames to the peers leave that fence, so the
    /// actor never holds a publication while it talks to a peer.
    async fn after_change(&mut self, outcome: RouteOutcome) {
        {
            let hub = &self.inner;
            info!(
                node = %hub.route.local(),
                epoch = hub.route.epoch(),
                terminal = ?hub.route.terminal(),
                candidates = hub.route.candidate_count(),
                selected = hub.route.selected().is_some(),
                "the route of the node is known"
            );
        }
        if outcome == RouteOutcome::Unchanged {
            return;
        }
        let source = self.selected_source();
        let mut frames = Vec::new();
        match &source {
            Source::None => {
                // The node has no route any more: the work of the old epoch
                // is cancelled, so no request of the old route survives.
                if outcome == RouteOutcome::Changed {
                    frames.extend(self.cancel_epoch().await);
                }
            }
            Source::Missing => {
                // The link of the selected route is gone, so the route state
                // of the node is stale: nothing is published for it. The
                // cleanup of that link runs when its command reaches the
                // actor.
                debug!("the link of the selected route is gone; the change was not published");
                return;
            }
            Source::Token(token) => {
                // The fence of the source is taken before its liveness is
                // checked, so the publication that is already admitted
                // finishes before the invalidation of the source
                // linearizes.
                let _guard = token.read_publication().await;
                if !token.is_live() {
                    // The upstream of the selected route is dead: nothing is
                    // published for a route it can no longer carry.
                    debug!(
                        "the upstream of the selected route is dead; the change was not published"
                    );
                    return;
                }
                match outcome {
                    RouteOutcome::Changed => {
                        frames.extend(self.cancel_epoch().await);
                        self.sync_worker().await;
                    }
                    RouteOutcome::SnapshotRefreshed => {
                        self.install_snapshot().await;
                    }
                    RouteOutcome::Unchanged => {}
                }
            }
        }
        // The frames to the peers leave the fence of the publication.
        for frame in frames {
            self.send_frame(frame);
        }
        match outcome {
            RouteOutcome::Changed => {
                if self.inner.route.selected().is_some() {
                    if let Source::Token(token) = &source {
                        self.advertise(token.clone());
                    }
                } else {
                    self.withdraw();
                }
            }
            RouteOutcome::SnapshotRefreshed => {
                if let Source::Token(token) = &source {
                    self.advertise(token.clone());
                }
            }
            RouteOutcome::Unchanged => {}
        }
    }

    /// The admission that carries the traffic of the selected route.
    fn selected_source(&self) -> Source {
        let Some(selected) = self.inner.route.selected() else {
            return Source::None;
        };
        let Some(link) = self.inner.links.get(&selected.next_hop) else {
            return Source::Missing;
        };
        if link.generation != selected.next_link_generation {
            return Source::Missing;
        }
        Source::Token(link.token.clone())
    }

    /// Withdraw the route of the node.
    ///
    /// The node cannot serve the route it advertised, so the candidate of
    /// every peer is withdrawn: the selection of the node is gone and the
    /// peers of the node learn that it has no route. A peer that advertises
    /// its route again restores the route of the node.
    async fn withdraw_route(&mut self) {
        let hub = &mut self.inner;
        let candidates = hub
            .links
            .keys()
            .copied()
            .filter_map(|peer| {
                let candidate = hub.route.candidate(peer)?;
                Some((peer, candidate.link_generation, candidate.generation + 1))
            })
            .collect::<Vec<_>>();
        for (peer, link_generation, generation) in candidates {
            let _ = hub.route.apply_withdraw(peer, link_generation, generation);
        }
    }

    /// Cancel the work of the epoch that ended.
    ///
    /// The virtual worker of the old epoch is closed and every request that
    /// was admitted on it is cancelled through the upstream link it was
    /// admitted on, so an old request is never replayed on the new route.
    ///
    /// The frames to the peers are collected: they leave the fence of the
    /// publication of the new route.
    async fn cancel_epoch(&mut self) -> Vec<PeerFrame> {
        let (worker, owners) = {
            let worker = self.inner.worker.take();
            let owners = self.inner.owners.drain().collect::<Vec<_>>();
            self.inner.downstream.clear();
            (worker, owners)
        };
        let mut frames = Vec::new();
        if let Some(worker) = worker {
            // The producer of the old epoch is stopped before the worker is
            // released, so it never waits for an actor that is gone.
            worker.task.abort();
            let _ = worker.task.await;
            cleanup_worker(&self.state, worker.id).await;
        }
        for (id, owner) in owners {
            if let Some(frame) = self.cancel_upstream_frame(&owner, &id) {
                frames.push(frame);
            }
            if let Some(downstream) = owner.downstream {
                let error = ResponseError {
                    request_id: downstream.request_id,
                    status: 503,
                    code: "peer_route_changed".to_string(),
                    message: "the route of the request changed".to_string(),
                };
                frames.push(PeerFrame {
                    peer: downstream.peer,
                    link_generation: downstream.link_generation,
                    envelope: PeerEnvelope::bridge(BridgeMessage::ResponseError(error)),
                    source: None,
                });
            }
        }
        frames
    }

    /// Start the virtual worker of the selected route.
    ///
    /// The worker is the only worker of the relay state of a database-less
    /// node: it represents the selected route, not one neighbour. The
    /// selected snapshot is applied before the worker is published, so a
    /// request can never select a worker that still runs with the previous
    /// route.
    async fn sync_worker(&mut self) {
        let snapshot = {
            if self.inner.worker.is_some() {
                return;
            }
            self.inner
                .route
                .selected()
                .map(|selected| selected.snapshot.clone())
        };
        let Some(snapshot) = snapshot else {
            return;
        };
        dispatch_worker_bridge_message(&self.state, BridgeMessage::ConfigSnapshot(snapshot)).await;
        // The worker serves the route of this epoch, so the epoch and the
        // link the traffic travels on are captured with it: a failure of the
        // worker tears exactly this route down.
        let (epoch, upstream_peer, upstream_generation) = {
            let Some(selected) = self.inner.route.selected() else {
                return;
            };
            (
                selected.epoch,
                selected.next_hop,
                selected.next_link_generation,
            )
        };
        let worker_id = self.state.allocate_worker_id();
        // The failure watch is installed before the worker is published, so
        // no producer can observe the worker without the failure path.
        let mut failure_rx = self.state.inner.install_worker_failure(worker_id);
        let (tx, mut rx): (WorkerSender, mpsc::Receiver<BridgeMessage>) =
            mpsc::channel(BRIDGE_LINK_BUFFER);
        if !register_worker(&self.state, worker_id, tx).await {
            self.state.inner.remove_worker_failure(worker_id);
            warn!("the relay state refused the virtual worker of the route");
            // The node cannot serve the route it advertised, so it ends the
            // route and tells its peers that it has none. The route is gone
            // now, so the worker is never published again for it.
            self.withdraw_route().await;
            return;
        }
        let data_tx = self.data_tx.clone();
        let control_tx = self.control_tx.clone();
        let data_budget = self.data_budget.clone();
        let ingress_total = self.ingress_total;
        let relay_state = self.state.clone();
        let task = tokio::spawn(async move {
            let mut failed = false;
            // The fences of the starts that wait in the ingress of the actor.
            // The pump keeps only weak references, so a fence dies with the
            // command that carries it and is never a tombstone.
            let mut fences: HashMap<(RequestKind, String), Vec<Weak<StartFence>>> = HashMap::new();
            // The flows that were already failed. The set is bounded, and it
            // is retired with the terminal message of its flow.
            let mut aborted: HashSet<(RequestKind, String)> = HashSet::new();
            loop {
                prune_fences(&mut fences);
                let message = tokio::select! {
                    biased;
                    // The failure of the worker is read first, so a producer
                    // that lost its traffic never queues another request.
                    changed = failure_rx.changed() => {
                        if changed.is_ok() {
                            failed = true;
                        }
                        break;
                    }
                    message = rx.recv() => match message {
                        Some(message) => message,
                        None => break,
                    },
                };
                // The close of a realtime session ends its flow on the way
                // up, so it is the terminal of that flow.
                let role = match &message {
                    BridgeMessage::RealtimeSessionClose(_) => {
                        MessageRole::Cancel(RequestKind::Realtime)
                    }
                    _ => classify(&message),
                };
                let request_id = request_id_of(&message).map(|id| id.to_string());
                let frame = frame_variant(&message);
                let kind = match role {
                    MessageRole::Start(kind)
                    | MessageRole::Stream(kind)
                    | MessageRole::End(kind)
                    | MessageRole::Cancel(kind) => Some(kind),
                    _ => None,
                };
                let terminal = matches!(role, MessageRole::End(_) | MessageRole::Cancel(_));
                let flow = kind.zip(request_id.clone()).map(|(kind, id)| (kind, id));
                // A message of a flow that was already failed is discarded:
                // the flow was explicitly failed, so it is not silently
                // continued and it needs no second retirement.
                if let Some(flow) = flow.as_ref()
                    && aborted.contains(flow)
                {
                    if terminal {
                        aborted.remove(flow);
                    }
                    continue;
                }
                // The producer of the route hands the traffic of the relay
                // state to the actor, with the byte budget of the ingress. A
                // closed ingress is the ordinary end of the actor, so it is
                // never a pressure of the ingress.
                let closed = data_tx.is_closed();
                let size = ingress_bytes(&message) as u32;
                let permit = if closed {
                    None
                } else {
                    data_budget.clone().try_acquire_many_owned(size).ok()
                };
                let fence = match role {
                    // A start of the relay state is fenced, so an abort that
                    // overtakes it keeps it from creating an owner.
                    MessageRole::Start(_) => {
                        let trackable = request_id
                            .as_ref()
                            .is_some_and(|id| !id.is_empty() && id.len() <= MAX_ID_BYTES);
                        if trackable && fences.len() >= MAX_FORWARDED_REQUESTS {
                            // The bounded tracking of the queued starts is
                            // exhausted, so the start is refused instead of
                            // being queued without a fence.
                            report_ingress_refusal(IngressRefusalContext {
                                source: "local",
                                reason: IngressRefusal::TrackingExhausted,
                                frame,
                                peer: None,
                                generation: None,
                                worker: Some(worker_id),
                                epoch: Some(epoch),
                                state: ingress_counters(&data_tx, &data_budget, ingress_total),
                                outcome: "request_failed",
                            });
                            fail_local_message(&control_tx, worker_id, epoch, &message).await;
                            continue;
                        }
                        trackable.then(StartFence::new)
                    }
                    _ => None,
                };
                let refused = match permit {
                    Some(permit) => {
                        let command = Command::ForwardLocal {
                            worker_id,
                            epoch,
                            message: message.clone(),
                            fence: fence.clone(),
                            permit,
                        };
                        match admit_into_ingress(
                            command,
                            |command| match data_tx.try_send(command) {
                                Ok(()) => Attempt::Queued,
                                Err(mpsc::error::TrySendError::Full(unsent)) => {
                                    Attempt::Full(unsent)
                                }
                                Err(mpsc::error::TrySendError::Closed(unsent)) => {
                                    Attempt::Closed(unsent)
                                }
                            },
                            &AdmissionGuard::Local(&failure_rx),
                        )
                        .await
                        {
                            Admission::Queued => None,
                            Admission::Full(command) => {
                                drop(command);
                                Some(IngressRefusal::ChannelFull)
                            }
                            Admission::Closed(command) => {
                                drop(command);
                                Some(IngressRefusal::ChannelClosed)
                            }
                            Admission::Stopped(command, stop) => {
                                // The worker of the route failed while the
                                // command waited, so the failure handling of
                                // the worker takes over. The end of its
                                // failure watch is a normal invalidation, so
                                // it never stops the link of the route.
                                drop(command);
                                if matches!(stop, Stop::Failed) {
                                    failed = true;
                                }
                                break;
                            }
                        }
                    }
                    None => {
                        if data_tx.is_closed() {
                            Some(IngressRefusal::ChannelClosed)
                        } else {
                            Some(IngressRefusal::ByteBudgetExhausted)
                        }
                    }
                };
                if let Some(reason) = refused {
                    let state = ingress_counters(&data_tx, &data_budget, ingress_total);
                    let tracking_has_room = aborted.len() < MAX_FORWARDED_REQUESTS;
                    let action = refusal_action(role, tracking_has_room);
                    // A tracking that is exhausted is its own reason, so the
                    // diagnostic names the bound that was reached.
                    let reason = if action == RefusalAction::FailWorker
                        && matches!(
                            role,
                            MessageRole::Stream(_) | MessageRole::End(_) | MessageRole::Cancel(_)
                        ) {
                        IngressRefusal::TrackingExhausted
                    } else {
                        reason
                    };
                    match action {
                        RefusalAction::FailRequest => {
                            // A new start never reached the actor, so it is
                            // failed promptly to the pending request of the
                            // relay state.
                            report_ingress_refusal(IngressRefusalContext {
                                source: "local",
                                reason,
                                frame,
                                peer: None,
                                generation: None,
                                worker: Some(worker_id),
                                epoch: Some(epoch),
                                state,
                                outcome: "request_failed",
                            });
                            fail_local_message(&control_tx, worker_id, epoch, &message).await;
                            continue;
                        }
                        RefusalAction::RetireRequest => {
                            let kind = kind.expect("a continuation carries its kind");
                            // A continuation of a running request has no
                            // pending request of the relay state any more,
                            // so the request itself is retired: the ingress
                            // of the node refuses one request, not the worker
                            // and not the route.
                            let Some(request_id) = request_id.clone() else {
                                report_ingress_refusal(IngressRefusalContext {
                                    source: "local",
                                    reason,
                                    frame,
                                    peer: None,
                                    generation: None,
                                    worker: Some(worker_id),
                                    epoch: Some(epoch),
                                    state,
                                    outcome: "worker_fallback",
                                });
                                relay_state.inner.signal_worker_failure(worker_id);
                                failed = true;
                                break;
                            };
                            let flow = (kind, request_id.clone());
                            aborted.insert(flow.clone());
                            // The start of the flow is fenced, so it never
                            // creates an owner when the actor sees it.
                            cancel_fences(&mut fences, &flow);
                            let (reply, _wait) = oneshot::channel();
                            let command = Command::AbortLocal {
                                worker_id,
                                epoch,
                                kind,
                                request_id,
                                reply,
                            };
                            if control_tx.try_send(command).is_err() {
                                // The priority path cannot carry the
                                // retirement, so the worker of the route
                                // fails as it did before.
                                report_ingress_refusal(IngressRefusalContext {
                                    source: "local",
                                    reason: IngressRefusal::ControlUnavailable,
                                    frame,
                                    peer: None,
                                    generation: None,
                                    worker: Some(worker_id),
                                    epoch: Some(epoch),
                                    state,
                                    outcome: "worker_fallback",
                                });
                                relay_state.inner.signal_worker_failure(worker_id);
                                failed = true;
                                break;
                            }
                            report_ingress_refusal(IngressRefusalContext {
                                source: "local",
                                reason,
                                frame,
                                peer: None,
                                generation: None,
                                worker: Some(worker_id),
                                epoch: Some(epoch),
                                state,
                                outcome: "scoped_retirement",
                            });
                            if terminal {
                                aborted.remove(&flow);
                            }
                            continue;
                        }
                        RefusalAction::FailWorker => {
                            // A message that cannot be retired in its scope
                            // fails the worker of the route as it did before.
                            report_ingress_refusal(IngressRefusalContext {
                                source: "local",
                                reason,
                                frame,
                                peer: None,
                                generation: None,
                                worker: Some(worker_id),
                                epoch: Some(epoch),
                                state,
                                outcome: "worker_fallback",
                            });
                            relay_state.inner.signal_worker_failure(worker_id);
                            failed = true;
                            break;
                        }
                    }
                }
                // The start reached the ingress of the actor, so its fence is
                // tracked until the command retires.
                if let (Some(fence), Some(flow)) = (fence, flow.as_ref())
                    && !aborted.contains(flow)
                {
                    fences
                        .entry(flow.clone())
                        .or_default()
                        .push(Arc::downgrade(&fence));
                }
            }
            if failed {
                // The worker cannot carry the traffic of the relay state, so
                // the actor tears the captured epoch down on the priority
                // path.
                let (reply, wait) = oneshot::channel();
                let command = Command::WorkerFailed {
                    worker_id,
                    epoch,
                    reply,
                };
                if control_tx.send(command).await.is_ok() {
                    let _ = wait.await;
                }
                return;
            }
            // The relay dropped the worker: the actor invalidates it.
            let (reply, wait) = oneshot::channel();
            let command = Command::InvalidateWorker { worker_id, reply };
            if control_tx.send(command).await.is_ok() {
                let _ = wait.await;
            }
        });
        self.inner.worker = Some(VirtualWorker {
            id: worker_id,
            epoch,
            upstream_peer,
            upstream_generation,
            task,
        });
    }

    /// Install the snapshot of the selected route in the relay state.
    async fn install_snapshot(&mut self) {
        let snapshot = self
            .inner
            .route
            .selected()
            .map(|selected| selected.snapshot.clone());
        let Some(snapshot) = snapshot else {
            return;
        };
        dispatch_worker_bridge_message(&self.state, BridgeMessage::ConfigSnapshot(snapshot)).await;
    }

    /// Advertise the selected route to the peers that may use it.
    ///
    /// A route is never advertised back to a peer that is part of the path.
    /// A peer that learned the route and can no longer extend it within the
    /// bound of the path learns that the node withdraws the route: a peer
    /// that keeps a route the node can no longer serve would refuse its
    /// requests instead of learning that the node has no route for them.
    fn advertise(&mut self, source: AdmissionToken) {
        let targets = {
            let hub = &mut self.inner;
            let Some(selected) = hub.route.selected().cloned() else {
                return;
            };
            // A peer extends the route of the node with its own node in
            // front of it, so the path may not grow beyond the bound.
            let extendable = selected.path.len() + 1 <= MAX_PEER_ROUTE_NODES;
            let peers = hub.links.keys().copied().collect::<Vec<_>>();
            peers
                .into_iter()
                .filter_map(|peer| {
                    let link_generation = hub.links.get(&peer)?.generation;
                    let generation = hub.next_counter(peer, link_generation);
                    let usable = extendable && !selected.path.contains(peer);
                    if usable {
                        hub.set_learned_route(peer, true);
                        Some((
                            peer,
                            link_generation,
                            generation,
                            Some((selected.path.clone(), selected.snapshot.clone())),
                        ))
                    } else if hub.learned_route(peer) {
                        // The peer learned the route and cannot use it any
                        // more, so it learns that the route is withdrawn.
                        hub.set_learned_route(peer, false);
                        Some((peer, link_generation, generation, None))
                    } else {
                        // The peer never learned the route of the node, so
                        // it has nothing to withdraw.
                        None
                    }
                })
                .collect::<Vec<_>>()
        };
        for (peer, link_generation, generation, advertised) in targets {
            match advertised {
                Some((path, snapshot)) => {
                    let envelope = PeerEnvelope::route_update(path, generation, &snapshot);
                    self.send_frame(PeerFrame {
                        peer,
                        link_generation,
                        envelope,
                        source: Some(source.clone()),
                    });
                }
                None => {
                    // The peer cannot use the route of the node, so it learns
                    // that the node withdraws it.
                    let envelope = PeerEnvelope::route_withdraw(generation);
                    self.send_frame(PeerFrame {
                        peer,
                        link_generation,
                        envelope,
                        source: None,
                    });
                }
            }
        }
    }

    /// Withdraw the route from the peers of this node.
    fn withdraw(&mut self) {
        let targets = {
            let hub = &mut self.inner;
            let peers = hub.links.keys().copied().collect::<Vec<_>>();
            peers
                .into_iter()
                .filter_map(|peer| {
                    let link_generation = hub.links.get(&peer)?.generation;
                    let generation = hub.next_counter(peer, link_generation);
                    hub.set_learned_route(peer, false);
                    Some((peer, link_generation, generation))
                })
                .collect::<Vec<_>>()
        };
        for (peer, link_generation, generation) in targets {
            let envelope = PeerEnvelope::route_withdraw(generation);
            self.send_frame(PeerFrame {
                peer,
                link_generation,
                envelope,
                source: None,
            });
        }
    }

    /// Send a frame to a peer.
    ///
    /// The egress is fenced by the admission of the destination and, for an
    /// advertisement, by the admission of the source of the route. The
    /// fences are taken without waiting and are released before the frame
    /// leaves the actor, so a peer is never waited for.
    fn send_frame(&mut self, frame: PeerFrame) {
        let PeerFrame {
            peer,
            link_generation,
            envelope,
            source,
        } = frame;
        let result = match &source {
            Some(source) => self.send_route_update(peer, link_generation, envelope, source),
            None => self.send_to_peer(peer, link_generation, envelope),
        };
        if let Err(err) = result {
            debug!(
                peer = %peer,
                link_generation,
                reason = err.reason(),
                "the frame did not reach the peer"
            );
        }
    }

    /// Send an envelope to a peer without waiting.
    ///
    /// The actor never waits for a peer, so a link that does not accept the
    /// frame now is stopped: its generation cleanup runs and the other links
    /// keep working.
    fn send_to_peer(
        &mut self,
        peer: NodeId,
        link_generation: u64,
        envelope: PeerEnvelope,
    ) -> Result<(), EnqueueError> {
        let link = admitted_link(&self.inner, &peer, link_generation)?;
        // The egress is fenced by the admission of the destination: a frame
        // is only queued while the link is still admitted.
        let guard = link.try_admit()?;
        let frame_kind = envelope.kind;
        let result = link.tx.try_send(envelope);
        drop(guard);
        if let Err(err) = result {
            report_enqueue_refusal(
                &peer,
                link_generation,
                frame_kind,
                err,
                link.tx.queue_state(),
            );
            // The link cannot carry the frame, so it loses its authority and
            // its session is told to stop. The cleanup of its generation
            // runs when the session releases the link.
            self.detach_link(peer, link_generation);
            return Err(err);
        }
        Ok(())
    }

    /// Send the advertisement of the selected route to a peer.
    ///
    /// The advertisement is only sent while the source of the route is still
    /// admitted: a dead upstream never refreshes the route of a downstream
    /// peer.
    fn send_route_update(
        &mut self,
        peer: NodeId,
        link_generation: u64,
        envelope: PeerEnvelope,
        source: &AdmissionToken,
    ) -> Result<(), EnqueueError> {
        let link = admitted_link(&self.inner, &peer, link_generation)?;
        let destination = link.try_admit()?;
        // The source of the route must still be admitted at the boundary of
        // the send, so a replaced upstream never refreshes the route of a
        // downstream peer.
        let Some(origin) = source.try_read_publication() else {
            drop(destination);
            return Err(EnqueueError::LinkUnavailable);
        };
        if !source.is_live() {
            drop(origin);
            drop(destination);
            return Err(EnqueueError::LinkUnavailable);
        }
        let frame_kind = envelope.kind;
        let result = link.tx.try_send(envelope);
        drop(origin);
        drop(destination);
        if let Err(err) = result {
            report_enqueue_refusal(
                &peer,
                link_generation,
                frame_kind,
                err,
                link.tx.queue_state(),
            );
            self.detach_link(peer, link_generation);
            return Err(err);
        }
        Ok(())
    }

    /// Stop the actor: the virtual worker of the node is released and the
    /// owners of the node fail.
    async fn shutdown(&mut self) {
        if let Some(worker) = self.inner.worker.take() {
            worker.task.abort();
            let _ = worker.task.await;
            cleanup_worker(&self.state, worker.id).await;
        }
        let owners = std::mem::take(&mut self.inner.owners);
        self.inner.downstream.clear();
        for (id, owner) in owners {
            self.fail_owner(&id, owner).await;
        }
    }

    /// A request id that no owner of the node uses.
    fn fresh_upstream_id(&self) -> anyhow::Result<String> {
        for _ in 0..8 {
            let candidate = uuid::Uuid::new_v4().to_string();
            if !self.inner.owners.contains_key(&candidate) {
                return Ok(candidate);
            }
        }
        Err(anyhow!("the node could not allocate a request id"))
    }
}

/// Refuse a local message that could not enter the ingress of the actor.
async fn fail_local_message(
    control_tx: &mpsc::Sender<Command>,
    worker_id: usize,
    epoch: u64,
    message: &BridgeMessage,
) {
    let Some(id) = request_id_of(message) else {
        return;
    };
    let (reply, wait) = oneshot::channel();
    let command = Command::FailLocal {
        worker_id,
        epoch,
        request_id: id.to_string(),
        status: 503,
        code: "peer_overloaded".to_string(),
        reply,
    };
    if control_tx.send(command).await.is_ok() {
        let _ = wait.await;
    }
}

/// What a response of a peer does.
enum Action {
    /// The response belongs to a request of the local relay state.
    Local(BridgeMessage),
    /// The response belongs to a request that came from a peer.
    Downstream(Downstream, BridgeMessage),
}

/// The direction a peer frame belongs to.
enum Direction {
    /// The frame comes from the executor of the route.
    Upstream,
    /// The frame comes from a client that uses this node as its next hop.
    Downstream(String),
}

/// Remove an owner and both of its indexes.
fn remove_owner_entry(hub: &mut HubState, id: &str) {
    if let Some(owner) = hub.owners.remove(id)
        && let Some(downstream) = owner.downstream
    {
        hub.downstream.remove(&DownstreamKey {
            peer: downstream.peer,
            link_generation: downstream.link_generation,
            request_id: downstream.request_id,
        });
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use tokio::sync::oneshot;
    use uuid::Uuid;

    use super::{
        FrameVariant, MessageRole, RefusalAction, RequestKind, RouteHub, RouteState, classify,
        frame_variant, refusal_action, response_kind,
    };
    use crate::protocol::{
        BridgeMessage, BridgeRequestStart, ClientRoute, ConfigSnapshot, ResponseEnd, ResponseStart,
    };
    use crate::relay::AppState;
    use crate::worker::runtime::node::identity::NodeId;
    use crate::worker::runtime::node::peer::session::run_node_peer_session;
    use crate::worker::runtime::node::peer::transport::{
        MemoryPeer, PeerFrame, PeerReader, PeerWriter,
    };
    use crate::worker::runtime::node::peer::wire::{PeerEnvelope, PeerEnvelopeKind, PeerPath};

    const LOCAL: u128 = 1;
    const MAC: u128 = 2;
    const NAS: u128 = 3;

    /// The managed client key of the local client of the node.
    const CLIENT_KEY: &str = "pfy_local_client_key";

    fn id(value: u128) -> NodeId {
        NodeId::from_uuid(Uuid::from_u128(value))
    }

    fn snapshot(version: i64) -> ConfigSnapshot {
        ConfigSnapshot {
            version,
            keys: vec![ClientRoute {
                key_hash: "hash".to_string(),
                key_prefix: "pfy_".to_string(),
                user_id: 1,
                route_id: "route".to_string(),
            }],
            relay_ip_policy: Default::default(),
        }
    }

    fn http_start(request_id: &str) -> BridgeMessage {
        BridgeMessage::RequestStart(BridgeRequestStart {
            request_id: request_id.to_string(),
            method: "POST".to_string(),
            path: "/v1/responses".to_string(),
            headers: vec![],
            request_deadline_unix_ms: 0,
            user_id: Some(1),
            route_id: Some("route".to_string()),
            client_key_hash: Some("hash".to_string()),
            request_user_agent: None,
            http_request_content_encoding: None,
            http_request_compressed: false,
            http_request_compressed_bytes: None,
        })
    }

    /// A node of the test: its relay state and its forwarding hub.
    fn node(value: u128) -> (Arc<RouteHub>, AppState) {
        let state = crate::relay::test_state();
        let hub = RouteHub::new(id(value), state.clone());
        (hub, state)
    }

    /// Run the session of a node on one half of an in-memory link.
    ///
    /// The other half of the link is returned, so the test may play the peer
    /// of the node.
    fn spawn_link(
        hub: Arc<RouteHub>,
        peer: NodeId,
        generation: u64,
    ) -> (
        MemoryPeer,
        oneshot::Sender<()>,
        tokio::task::JoinHandle<anyhow::Result<()>>,
    ) {
        let (near, far) = MemoryPeer::pair(64);
        let (reader, writer) = near.into_halves();
        let (stop_tx, stop_rx) = oneshot::channel();
        let handle = tokio::spawn(run_node_peer_session(
            reader,
            writer,
            hub,
            super::super::admission::mint_for_test(peer, generation),
            Duration::from_secs(30),
            stop_rx,
        ));
        (far, stop_tx, handle)
    }

    /// The starts the fake database node saw.
    type Seen = Arc<Mutex<Vec<(String, Vec<NodeId>)>>>;

    /// The answer of the fake database node for a request.
    fn answer_of(request: &BridgeMessage, request_id: &str) -> Vec<BridgeMessage> {
        match request {
            BridgeMessage::McpRequestStart(_) => vec![
                BridgeMessage::McpResponseStart(crate::protocol::McpResponseStart {
                    request_id: request_id.to_string(),
                    status: 200,
                    content_type: Some("application/json".to_string()),
                    headers: vec![],
                }),
                BridgeMessage::McpResponseEnd(crate::protocol::McpResponseEnd {
                    request_id: request_id.to_string(),
                }),
            ],
            BridgeMessage::RealtimeSessionStart(_) => vec![
                BridgeMessage::RealtimeServerEvent(crate::protocol::RealtimeServerEventMessage {
                    request_id: request_id.to_string(),
                    event_json: "{}".to_string(),
                }),
                BridgeMessage::RealtimeSessionClose(crate::protocol::RealtimeSessionClose {
                    request_id: request_id.to_string(),
                    code: None,
                    reason: None,
                    response_started: true,
                }),
            ],
            _ => vec![
                BridgeMessage::ResponseStart(ResponseStart {
                    request_id: request_id.to_string(),
                    status: 200,
                    content_type: Some("text/plain".to_string()),
                    headers: vec![],
                }),
                BridgeMessage::ResponseEnd(ResponseEnd {
                    request_id: request_id.to_string(),
                }),
            ],
        }
    }

    /// A fake database node that answers the requests of its peer.
    ///
    /// The fake node speaks the peer envelope of the production codec and
    /// never starts an executor of its own.
    async fn run_fake_nas(
        mut reader: impl PeerReader,
        mut writer: impl PeerWriter,
        nas: NodeId,
        seen: Seen,
        gate: Arc<tokio::sync::Notify>,
    ) {
        let path = PeerPath::new(vec![nas]).expect("a bounded path");
        let advertisement = PeerEnvelope::route_update(path, 1, &snapshot(1));
        if writer
            .send_binary(advertisement.encode().expect("the envelope encodes"))
            .await
            .is_err()
        {
            return;
        }
        while let Ok(Some(frame)) = reader.recv().await {
            let PeerFrame::Binary(bytes) = frame else {
                continue;
            };
            let Ok(envelope) = PeerEnvelope::decode(&bytes) else {
                continue;
            };
            let Some(message) = envelope.message else {
                continue;
            };
            match message {
                BridgeMessage::Ping => {
                    let _ = writer
                        .send_binary(
                            PeerEnvelope::bridge(BridgeMessage::Pong)
                                .encode()
                                .expect("the envelope encodes"),
                        )
                        .await;
                }
                message => {
                    let request_id = match &message {
                        BridgeMessage::RequestStart(start) => Some(start.request_id.clone()),
                        BridgeMessage::McpRequestStart(start) => Some(start.request_id.clone()),
                        BridgeMessage::RealtimeSessionStart(start) => {
                            Some(start.request_id.clone())
                        }
                        _ => None,
                    };
                    let Some(request_id) = request_id else {
                        continue;
                    };
                    let path = envelope
                        .path
                        .map(|path| path.nodes().to_vec())
                        .unwrap_or_default();
                    seen.lock()
                        .expect("seen lock")
                        .push((request_id.clone(), path));
                    // The answer waits for the test, so the test may observe
                    // the request while it is still running.
                    gate.notified().await;
                    for message in answer_of(&message, &request_id) {
                        let _ = writer
                            .send_binary(
                                PeerEnvelope::bridge(message)
                                    .encode()
                                    .expect("the envelope encodes"),
                            )
                            .await;
                    }
                }
            }
        }
    }

    /// Wait until the condition holds.
    async fn wait_until<F, Fut>(mut condition: F)
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = bool>,
    {
        tokio::time::timeout(Duration::from_secs(5), async {
            while !condition().await {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the condition holds");
    }

    /// The envelopes a link receives until it stays quiet.
    ///
    /// The observation is bounded: the drain ends when the link receives
    /// nothing for a moment.
    async fn drain(receiver: &mut super::EnvelopeReceiver) -> Vec<PeerEnvelope> {
        let mut envelopes = Vec::new();
        while let Ok(Some(queued)) =
            tokio::time::timeout(Duration::from_millis(200), receiver.recv()).await
        {
            let (bytes, _permit) = queued.into_parts();
            envelopes.push(PeerEnvelope::decode(&bytes).expect("the envelope decodes"));
        }
        envelopes
    }

    /// The commands that wait in the data channel of the actor.
    ///
    /// The actor is blocked, so the commands pile up in the data channel
    /// until it holds every slot.
    fn fill_data_channel(handle: &super::ActorHandle, peer: NodeId, limit: usize) -> usize {
        let mut queued = 0usize;
        while queued < limit {
            let message = BridgeMessage::ResponseEnd(crate::protocol::ResponseEnd {
                request_id: format!("filler-{queued}"),
            });
            let Ok(permit) = handle
                .data_budget
                .clone()
                .try_acquire_many_owned(super::ingress_bytes(&message) as u32)
            else {
                break;
            };
            let (reply, _wait) = oneshot::channel();
            if handle
                .data
                .try_send(super::Command::HandlePeerMessage {
                    token: super::super::admission::mint_for_test(peer, 1),
                    message,
                    permit,
                    reply,
                })
                .is_err()
            {
                break;
            }
            queued += 1;
        }
        queued
    }

    #[tokio::test]
    async fn a_request_of_a_peer_travels_over_the_route_to_the_database_node() {
        use super::super::{auth, handshake::Hello, wire::PEER_PROTOCOL_VERSION};
        let manager = crate::relay_secrets::RelaySecretManager::from_key_bytes([7; 32]);
        let server = Hello::new(id(NAS).as_uuid(), true, "2.0.0", PEER_PROTOCOL_VERSION);
        let dialer = Hello::new(id(MAC).as_uuid(), false, "1.0.0", PEER_PROTOCOL_VERSION);
        let epoch = auth::startup_epoch();
        let challenge =
            auth::issue_challenge(&manager, server.clone(), &epoch, auth::now_ms()).unwrap();
        let (proof, ticket) =
            auth::build_proof(&manager, &dialer, &challenge, auth::now_ms()).unwrap();
        let verified = auth::verify_peer(
            &manager,
            &server,
            &epoch,
            &challenge.ticket,
            &proof,
            auth::now_ms(),
            &auth::ReplayCache::new(),
        )
        .unwrap();
        let confirmation =
            auth::seal_confirmation(&manager, &verified.hello, &verified.ticket).unwrap();
        auth::verify_confirmation(&manager, &dialer, &ticket, &confirmation).unwrap();
        let (mac_hub, mac_state) = node(MAC);
        let (internal_hub, internal_state) = node(LOCAL);
        let seen: Seen = Arc::new(Mutex::new(Vec::new()));

        // The database node answers the requests of the Mac.
        let (nas_side, mac_side) = MemoryPeer::pair(64);
        let (nas_reader, nas_writer) = nas_side.into_halves();
        let gate = Arc::new(tokio::sync::Notify::new());
        let nas_task = tokio::spawn(run_fake_nas(
            nas_reader,
            nas_writer,
            id(NAS),
            seen.clone(),
            gate.clone(),
        ));
        let (mac_reader, mac_writer) = mac_side.into_halves();
        let (mac_stop_tx, mac_stop_rx) = oneshot::channel();
        let mac_task = tokio::spawn(run_node_peer_session(
            mac_reader,
            mac_writer,
            mac_hub.clone(),
            super::super::admission::mint_for_test(id(NAS), 1),
            Duration::from_secs(30),
            mac_stop_rx,
        ));

        // The Mac is the next hop of the internal node.
        let (mac_side2, internal_side) = MemoryPeer::pair(64);
        let (mac_reader2, mac_writer2) = mac_side2.into_halves();
        let (mac_stop_tx2, mac_stop_rx2) = oneshot::channel();
        let mac_task2 = tokio::spawn(run_node_peer_session(
            mac_reader2,
            mac_writer2,
            mac_hub.clone(),
            super::super::admission::mint_for_test(id(LOCAL), 2),
            Duration::from_secs(30),
            mac_stop_rx2,
        ));
        let (internal_reader, internal_writer) = internal_side.into_halves();
        let (internal_stop_tx, internal_stop_rx) = oneshot::channel();
        let internal_task = tokio::spawn(run_node_peer_session(
            internal_reader,
            internal_writer,
            internal_hub.clone(),
            super::super::admission::mint_for_test(id(MAC), 1),
            Duration::from_secs(30),
            internal_stop_rx,
        ));

        // Both database-less nodes know the route and installed the snapshot
        // of the database node.
        wait_until(|| async { !mac_state.inner.workers.lock().await.is_empty() }).await;
        wait_until(|| async { !internal_state.inner.workers.lock().await.is_empty() }).await;
        assert_eq!(mac_hub.selected_path().await, Some(vec![id(MAC), id(NAS)]));
        assert_eq!(
            internal_hub.selected_path().await,
            Some(vec![id(LOCAL), id(MAC), id(NAS)])
        );
        assert_eq!(mac_state.inner.routes.lock().await.len(), 1);
        assert_eq!(internal_state.inner.routes.lock().await.len(), 1);

        // The local client of the internal node sends a request over the
        // virtual worker of the selected route.
        let worker = {
            let workers = internal_state.inner.workers.lock().await;
            workers
                .values()
                .next()
                .cloned()
                .expect("the virtual worker")
        };
        worker
            .send(http_start("local-1"))
            .await
            .expect("the request reaches the worker");
        wait_until(|| async { internal_hub.owner_count().await == 1 }).await;

        // The database node saw the request with the full path of the route.
        wait_until(|| async { !seen.lock().expect("seen lock").is_empty() }).await;
        let seen = seen.lock().expect("seen lock").clone();
        assert_eq!(seen.len(), 1, "the request is never broadcast");
        assert_ne!(
            seen[0].0, "local-1",
            "the forwarding node gives the request a fresh id of its own"
        );
        assert_eq!(
            seen[0].1,
            vec![id(LOCAL), id(MAC), id(NAS)],
            "the request carries the full path of the route"
        );

        // The request is owned by the node while it runs.
        assert_eq!(internal_hub.owner_count().await, 1);

        // The answer of the database node reaches the node that owns the
        // request and releases it.
        gate.notify_one();
        wait_until(|| async { internal_hub.owner_count().await == 0 }).await;

        mac_stop_tx.send(()).ok();
        mac_stop_tx2.send(()).ok();
        internal_stop_tx.send(()).ok();
        let _ = mac_task.await;
        let _ = mac_task2.await;
        let _ = internal_task.await;
        nas_task.abort();
    }

    #[tokio::test]
    async fn a_start_with_a_foreign_path_never_reaches_the_database_node() {
        let (mac_hub, _state) = node(MAC);
        let seen: Seen = Arc::new(Mutex::new(Vec::new()));
        let (nas_side, mac_side) = MemoryPeer::pair(64);
        let (nas_reader, nas_writer) = nas_side.into_halves();
        let nas_task = tokio::spawn(run_fake_nas(
            nas_reader,
            nas_writer,
            id(NAS),
            seen.clone(),
            Arc::new(tokio::sync::Notify::new()),
        ));
        let (mac_reader, mac_writer) = mac_side.into_halves();
        let (stop_tx, stop_rx) = oneshot::channel();
        let mac_task = tokio::spawn(run_node_peer_session(
            mac_reader,
            mac_writer,
            mac_hub.clone(),
            super::super::admission::mint_for_test(id(NAS), 1),
            Duration::from_secs(30),
            stop_rx,
        ));
        wait_until(|| async { mac_hub.selected_path().await.is_some() }).await;

        // A peer of the Mac sends a start whose path is not the path of the
        // selected route.
        let (peer_side, peer_stop_tx, peer_task) = spawn_link(mac_hub.clone(), id(LOCAL), 2);
        let (_peer_reader, mut peer_writer) = peer_side.into_halves();
        let foreign = PeerPath::new(vec![id(LOCAL), id(99)]).expect("a bounded path");
        peer_writer
            .send_binary(
                PeerEnvelope::request_start(foreign, http_start("loop-1"))
                    .encode()
                    .expect("the envelope encodes"),
            )
            .await
            .expect("the start is written");

        // The database node never saw the request.
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(
            seen.lock().expect("seen lock").is_empty(),
            "a start with a foreign path never reaches the database node"
        );
        assert_eq!(mac_hub.owner_count().await, 0);

        peer_stop_tx.send(()).ok();
        stop_tx.send(()).ok();
        let _ = peer_task.await;
        let _ = mac_task.await;
        nas_task.abort();
    }

    /// The database side of a link, driven by the test.
    struct FakeDatabase {
        writer: crate::worker::runtime::node::peer::transport::MemoryPeerWriter,
        reader: crate::worker::runtime::node::peer::transport::MemoryPeerReader,
    }

    impl FakeDatabase {
        /// The database side of an in-memory link.
        fn new(side: MemoryPeer) -> Self {
            let (reader, writer) = side.into_halves();
            Self { writer, reader }
        }

        /// Advertise the route of the database node.
        async fn advertise(&mut self, generation: u64, version: i64) {
            let path = PeerPath::new(vec![id(NAS)]).expect("a bounded path");
            let envelope = PeerEnvelope::route_update(path, generation, &snapshot(version));
            self.writer
                .send_binary(envelope.encode().expect("the envelope encodes"))
                .await
                .expect("the advertisement is written");
        }

        /// Wait for the next request that reaches the database node.
        async fn next_start(&mut self) -> (String, Vec<NodeId>) {
            loop {
                let frame = tokio::time::timeout(Duration::from_secs(5), self.reader.recv())
                    .await
                    .expect("the request arrives")
                    .expect("the link is open")
                    .expect("the link carries a frame");
                let PeerFrame::Binary(bytes) = frame else {
                    continue;
                };
                let envelope = PeerEnvelope::decode(&bytes).expect("the envelope decodes");
                let Some(BridgeMessage::RequestStart(start)) = envelope.message else {
                    continue;
                };
                let path = envelope
                    .path
                    .map(|path| path.nodes().to_vec())
                    .unwrap_or_default();
                return (start.request_id, path);
            }
        }

        /// Answer a request.
        async fn answer(&mut self, request_id: &str) {
            for message in [
                BridgeMessage::ResponseStart(ResponseStart {
                    request_id: request_id.to_string(),
                    status: 200,
                    content_type: Some("text/plain".to_string()),
                    headers: vec![],
                }),
                BridgeMessage::ResponseEnd(ResponseEnd {
                    request_id: request_id.to_string(),
                }),
            ] {
                self.writer
                    .send_binary(
                        PeerEnvelope::bridge(message)
                            .encode()
                            .expect("the envelope encodes"),
                    )
                    .await
                    .expect("the answer is written");
            }
        }
    }

    #[tokio::test]
    async fn a_snapshot_that_only_refreshes_keeps_the_running_request_alive() {
        let (mac_hub, mac_state) = node(MAC);
        let (nas_side, mac_side) = MemoryPeer::pair(64);
        let mut database = FakeDatabase::new(nas_side);
        let (mac_reader, mac_writer) = mac_side.into_halves();
        let (stop_tx, stop_rx) = oneshot::channel();
        let mac_task = tokio::spawn(run_node_peer_session(
            mac_reader,
            mac_writer,
            mac_hub.clone(),
            super::super::admission::mint_for_test(id(NAS), 1),
            Duration::from_secs(30),
            stop_rx,
        ));
        database.advertise(1, 1).await;
        wait_until(|| async { mac_hub.selected_path().await.is_some() }).await;
        let epoch = mac_hub.epoch().await;

        // The local client of the Mac sends a request.
        let worker = {
            let workers = mac_state.inner.workers.lock().await;
            workers
                .values()
                .next()
                .cloned()
                .expect("the virtual worker")
        };
        worker
            .send(http_start("local-1"))
            .await
            .expect("the request reaches the worker");
        let (request_id, path) = database.next_start().await;
        assert_eq!(path, vec![id(MAC), id(NAS)]);
        assert_eq!(mac_hub.owner_count().await, 1);

        // The database node publishes a new snapshot without a new route.
        database.advertise(2, 2).await;
        wait_until(|| async { mac_state.inner.routes.lock().await.len() == 1 }).await;
        assert_eq!(
            mac_hub.epoch().await,
            epoch,
            "a snapshot that only refreshes does not move the epoch"
        );
        assert_eq!(
            mac_hub.owner_count().await,
            1,
            "the running request survives the refresh"
        );

        // The answer of the database node releases the request.
        database.answer(&request_id).await;
        wait_until(|| async { mac_hub.owner_count().await == 0 }).await;

        stop_tx.send(()).ok();
        let _ = mac_task.await;
    }

    #[tokio::test]
    async fn a_local_request_and_a_request_of_a_peer_keep_their_own_ids() {
        let (mac_hub, mac_state) = node(MAC);
        let (nas_side, mac_side) = MemoryPeer::pair(64);
        let mut database = FakeDatabase::new(nas_side);
        let (mac_reader, mac_writer) = mac_side.into_halves();
        let (stop_tx, stop_rx) = oneshot::channel();
        let mac_task = tokio::spawn(run_node_peer_session(
            mac_reader,
            mac_writer,
            mac_hub.clone(),
            super::super::admission::mint_for_test(id(NAS), 1),
            Duration::from_secs(30),
            stop_rx,
        ));
        database.advertise(1, 1).await;
        wait_until(|| async { mac_hub.selected_path().await.is_some() }).await;

        // A peer of the Mac sends a request that carries the same id as the
        // local request of the Mac.
        let (peer_side, peer_stop_tx, peer_task) = spawn_link(mac_hub.clone(), id(LOCAL), 2);
        let (mut peer_reader, mut peer_writer) = peer_side.into_halves();
        let path = PeerPath::new(vec![id(LOCAL), id(MAC), id(NAS)]).expect("a bounded path");
        peer_writer
            .send_binary(
                PeerEnvelope::request_start(path, http_start("same"))
                    .encode()
                    .expect("the envelope encodes"),
            )
            .await
            .expect("the start is written");

        // The local client of the Mac sends a request with the same id.
        let worker = {
            let workers = mac_state.inner.workers.lock().await;
            workers
                .values()
                .next()
                .cloned()
                .expect("the virtual worker")
        };
        worker
            .send(http_start("same"))
            .await
            .expect("the request reaches the worker");

        // The database node sees two different requests.
        let first = database.next_start().await;
        let second = database.next_start().await;
        assert_ne!(
            first.0, second.0,
            "the requests of the node and of its peer never share an id"
        );

        // The answers of the database node reach their owners.
        database.answer(&first.0).await;
        database.answer(&second.0).await;
        wait_until(|| async { mac_hub.owner_count().await == 0 }).await;
        let answer = loop {
            let frame = tokio::time::timeout(Duration::from_secs(5), peer_reader.recv())
                .await
                .expect("the peer receives its answer")
                .expect("the link is open")
                .expect("the link carries a frame");
            let PeerFrame::Binary(bytes) = frame else {
                continue;
            };
            let envelope = PeerEnvelope::decode(&bytes).expect("the envelope decodes");
            if let Some(BridgeMessage::ResponseStart(start)) = envelope.message {
                break start;
            }
        };
        assert_eq!(answer.request_id, "same");

        peer_stop_tx.send(()).ok();
        stop_tx.send(()).ok();
        let _ = peer_task.await;
        let _ = mac_task.await;
    }

    #[tokio::test]
    async fn the_route_is_withdrawn_when_the_link_of_the_next_hop_ends() {
        let (mac_hub, mac_state) = node(MAC);
        let (nas_side, mac_side) = MemoryPeer::pair(64);
        let (nas_reader, nas_writer) = nas_side.into_halves();
        let nas_task = tokio::spawn(run_fake_nas(
            nas_reader,
            nas_writer,
            id(NAS),
            Arc::new(Mutex::new(Vec::new())),
            Arc::new(tokio::sync::Notify::new()),
        ));
        let (mac_reader, mac_writer) = mac_side.into_halves();
        let (stop_tx, stop_rx) = oneshot::channel();
        let mac_task = tokio::spawn(run_node_peer_session(
            mac_reader,
            mac_writer,
            mac_hub.clone(),
            super::super::admission::mint_for_test(id(NAS), 1),
            Duration::from_secs(30),
            stop_rx,
        ));
        wait_until(|| async { mac_hub.selected_path().await.is_some() }).await;

        // A peer of the Mac watches the route controls.
        let (peer_side, peer_stop_tx, peer_task) = spawn_link(mac_hub.clone(), id(LOCAL), 2);
        let (mut peer_reader, _peer_writer) = peer_side.into_halves();

        // The link of the database node ends: the route of the Mac is gone
        // and the peer learns that the route was withdrawn.
        nas_task.abort();
        let withdrawal = loop {
            let frame = tokio::time::timeout(Duration::from_secs(5), peer_reader.recv())
                .await
                .expect("the peer receives the withdrawal")
                .expect("the link is open")
                .expect("the link carries a frame");
            let PeerFrame::Binary(bytes) = frame else {
                continue;
            };
            let envelope = PeerEnvelope::decode(&bytes).expect("the envelope decodes");
            if envelope.kind
                == crate::worker::runtime::node::peer::wire::PeerEnvelopeKind::RouteWithdraw
            {
                break envelope;
            }
        };
        assert!(withdrawal.path.is_none());
        assert_eq!(mac_hub.selected_path().await, None);
        wait_until(|| async { mac_state.inner.workers.lock().await.is_empty() }).await;

        peer_stop_tx.send(()).ok();
        stop_tx.send(()).ok();
        let _ = peer_task.await;
        let _ = mac_task.await;
    }

    #[tokio::test]
    async fn an_mcp_and_a_realtime_request_travel_over_the_route_like_an_http_request() {
        let (mac_hub, _state) = node(MAC);
        let seen: Seen = Arc::new(Mutex::new(Vec::new()));
        let (nas_side, mac_side) = MemoryPeer::pair(64);
        let (nas_reader, nas_writer) = nas_side.into_halves();
        let gate = Arc::new(tokio::sync::Notify::new());
        let nas_task = tokio::spawn(run_fake_nas(
            nas_reader,
            nas_writer,
            id(NAS),
            seen.clone(),
            gate.clone(),
        ));
        let (mac_reader, mac_writer) = mac_side.into_halves();
        let (stop_tx, stop_rx) = oneshot::channel();
        let mac_task = tokio::spawn(run_node_peer_session(
            mac_reader,
            mac_writer,
            mac_hub.clone(),
            super::super::admission::mint_for_test(id(NAS), 1),
            Duration::from_secs(30),
            stop_rx,
        ));
        wait_until(|| async { mac_hub.selected_path().await.is_some() }).await;

        // A peer of the Mac sends an MCP request and a realtime request.
        let (peer_side, peer_stop_tx, peer_task) = spawn_link(mac_hub.clone(), id(LOCAL), 2);
        let (_peer_reader, mut peer_writer) = peer_side.into_halves();
        let path = PeerPath::new(vec![id(LOCAL), id(MAC), id(NAS)]).expect("a bounded path");
        for message in [
            BridgeMessage::McpRequestStart(crate::protocol::McpRequestStart {
                request_id: "mcp-1".to_string(),
                server_name: None,
                method: "POST".to_string(),
                path: "/mcp".to_string(),
                headers: vec![],
                user_id: Some(1),
                client_key_hash: Some("hash".to_string()),
                http_request_content_encoding: None,
                http_request_compressed: false,
                http_request_compressed_bytes: None,
            }),
            BridgeMessage::RealtimeSessionStart(crate::protocol::RealtimeSessionStart {
                request_id: "rt-1".to_string(),
                model: "gpt".to_string(),
                path: "/v1/realtime".to_string(),
                user_id: Some(1),
                route_id: None,
                client_key_hash: None,
                request_user_agent: None,
            }),
        ] {
            peer_writer
                .send_binary(
                    PeerEnvelope::request_start(path.clone(), message)
                        .encode()
                        .expect("the envelope encodes"),
                )
                .await
                .expect("the start is written");
        }

        // The database node answers both requests.
        gate.notify_one();
        gate.notify_one();

        // The database node sees both requests with the path of the route.
        wait_until(|| async { seen.lock().expect("seen lock").len() == 2 }).await;
        let seen = seen.lock().expect("seen lock").clone();
        assert_ne!(seen[0].0, seen[1].0, "the requests never share an id");
        for (_request_id, path) in &seen {
            assert_eq!(
                path,
                &vec![id(LOCAL), id(MAC), id(NAS)],
                "the requests carry the full path of the route"
            );
        }

        peer_stop_tx.send(()).ok();
        stop_tx.send(()).ok();
        let _ = peer_task.await;
        let _ = mac_task.await;
        nas_task.abort();
    }

    #[tokio::test]
    async fn a_duplicate_local_request_id_never_reaches_the_database_node() {
        let (mac_hub, mac_state) = node(MAC);
        let seen: Seen = Arc::new(Mutex::new(Vec::new()));
        let (nas_side, mac_side) = MemoryPeer::pair(64);
        let (nas_reader, nas_writer) = nas_side.into_halves();
        let nas_task = tokio::spawn(run_fake_nas(
            nas_reader,
            nas_writer,
            id(NAS),
            seen.clone(),
            Arc::new(tokio::sync::Notify::new()),
        ));
        let (mac_reader, mac_writer) = mac_side.into_halves();
        let (stop_tx, stop_rx) = oneshot::channel();
        let mac_task = tokio::spawn(run_node_peer_session(
            mac_reader,
            mac_writer,
            mac_hub.clone(),
            super::super::admission::mint_for_test(id(NAS), 1),
            Duration::from_secs(30),
            stop_rx,
        ));
        wait_until(|| async { mac_hub.selected_path().await.is_some() }).await;

        // The local client of the Mac sends the same request id twice.
        let worker = {
            let workers = mac_state.inner.workers.lock().await;
            workers
                .values()
                .next()
                .cloned()
                .expect("the virtual worker")
        };
        worker
            .send(http_start("same"))
            .await
            .expect("the request reaches the worker");
        worker
            .send(http_start("same"))
            .await
            .expect("the request reaches the worker");
        wait_until(|| async { !seen.lock().expect("seen lock").is_empty() }).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(
            seen.lock().expect("seen lock").len(),
            1,
            "a request id that is already in use never travels twice"
        );

        stop_tx.send(()).ok();
        let _ = mac_task.await;
        nas_task.abort();
    }

    #[tokio::test]
    async fn a_route_that_cannot_be_extended_is_withdrawn_from_the_peer_that_learned_it() {
        let (hub, _state) = node(LOCAL);
        let nas = super::super::admission::mint_for_test(id(NAS), 1);
        let (nas_tx, _nas_rx) = super::envelope_channel();
        let (nas_shutdown, _nas_shutdown_rx) = tokio::sync::watch::channel(false);
        assert!(hub.register_link(&nas, nas_tx, nas_shutdown).await);
        let other = super::super::admission::mint_for_test(id(4), 1);
        let (other_tx, _other_rx) = super::envelope_channel();
        let (other_shutdown, _other_shutdown_rx) = tokio::sync::watch::channel(false);
        assert!(hub.register_link(&other, other_tx, other_shutdown).await);
        // A peer of the node watches the route controls.
        let peer = super::super::admission::mint_for_test(id(MAC), 1);
        let (peer_tx, mut peer_rx) = super::envelope_channel();
        let (peer_shutdown, _peer_shutdown_rx) = tokio::sync::watch::channel(false);
        assert!(hub.register_link(&peer, peer_tx, peer_shutdown).await);

        // The database node advertises the direct route, the other node the
        // two hop route to the same database node.
        hub.apply_update(
            &nas,
            1,
            &PeerPath::new(vec![id(NAS)]).expect("a bounded path"),
            snapshot(1),
        )
        .await
        .expect("the direct route is applied");
        hub.apply_update(
            &other,
            1,
            &PeerPath::new(vec![id(4), id(NAS)]).expect("a bounded path"),
            snapshot(1),
        )
        .await
        .expect("the two hop route is applied");
        assert_eq!(
            hub.selected_path().await,
            Some(vec![id(LOCAL), id(NAS)]),
            "the shortest route wins"
        );

        // The peer of the node learns the direct route.
        let learned = next_envelope(&mut peer_rx).await;
        assert_eq!(
            learned.kind,
            crate::worker::runtime::node::peer::wire::PeerEnvelopeKind::RouteUpdate
        );

        // The direct link of the database node ends: the node selects the
        // two hop route, which the peer cannot extend within the bound of
        // the path.
        hub.invalidate_link(id(NAS), 1)
            .await
            .expect("the link of the database node is invalidated");
        assert_eq!(
            hub.selected_path().await,
            Some(vec![id(LOCAL), id(4), id(NAS)]),
            "the two hop route takes over"
        );
        let withdrawal = loop {
            let envelope = next_envelope(&mut peer_rx).await;
            if envelope.kind
                == crate::worker::runtime::node::peer::wire::PeerEnvelopeKind::RouteWithdraw
            {
                break envelope;
            }
        };
        assert!(
            withdrawal.path.is_none(),
            "the peer that learned the route learns that it is withdrawn"
        );
    }

    #[tokio::test]
    async fn a_stale_alternate_never_restores_the_configuration_of_the_node() {
        let (hub, state) = node(LOCAL);

        // The database node advertises the direct route with the current
        // configuration.
        let nas = super::super::admission::mint_for_test(id(NAS), 1);
        let (nas_tx, _nas_rx) = super::envelope_channel();
        let (nas_shutdown, _nas_shutdown_rx) = tokio::sync::watch::channel(false);
        assert!(hub.register_link(&nas, nas_tx, nas_shutdown).await);
        let direct = PeerPath::new(vec![id(NAS)]).expect("a bounded path");
        hub.apply_update(&nas, 1, &direct, snapshot(2))
            .await
            .expect("the route is applied");
        wait_until(|| async { !state.inner.workers.lock().await.is_empty() }).await;

        // A peer of the node learns the route of the node.
        let peer = super::super::admission::mint_for_test(id(MAC), 1);
        let (peer_tx, mut peer_rx) = super::envelope_channel();
        let (peer_shutdown, _peer_shutdown_rx) = tokio::sync::watch::channel(false);
        assert!(hub.register_link(&peer, peer_tx, peer_shutdown).await);
        let learned = next_envelope(&mut peer_rx).await;
        assert_eq!(learned.kind, PeerEnvelopeKind::RouteUpdate);

        // An alternate peer of the node still carries the older
        // configuration.
        let other = super::super::admission::mint_for_test(id(4), 1);
        let (other_tx, _other_rx) = super::envelope_channel();
        let (other_shutdown, _other_shutdown_rx) = tokio::sync::watch::channel(false);
        assert!(hub.register_link(&other, other_tx, other_shutdown).await);
        let alternate = PeerPath::new(vec![id(4), id(NAS)]).expect("a bounded path");
        hub.apply_update(&other, 1, &alternate, snapshot(1))
            .await
            .expect("the alternate route is applied");
        assert_eq!(
            hub.selected_path().await,
            Some(vec![id(LOCAL), id(NAS)]),
            "the direct route stays selected"
        );

        // The direct link of the database node ends: the stale alternate
        // never takes over, so the node has no route, no worker and no
        // configuration any more.
        hub.invalidate_link(id(NAS), 1)
            .await
            .expect("the link of the database node ends");
        assert!(hub.selected_path().await.is_none());
        wait_until(|| async { state.inner.workers.lock().await.is_empty() }).await;
        assert_eq!(
            *state.inner.config_version.lock().await,
            None,
            "the older configuration never reaches the relay state"
        );

        // The peer of the node learns that the node has no route.
        let withdrawal = loop {
            let envelope = next_envelope(&mut peer_rx).await;
            if envelope.kind == PeerEnvelopeKind::RouteWithdraw {
                break envelope;
            }
        };
        assert!(
            withdrawal.path.is_none(),
            "the peer never learns a route with an older configuration"
        );

        // The alternate with the current configuration restores the route.
        hub.apply_update(&other, 2, &alternate, snapshot(2))
            .await
            .expect("the route is restored");
        wait_until(|| async { !state.inner.workers.lock().await.is_empty() }).await;
        assert_eq!(
            hub.selected_path().await,
            Some(vec![id(LOCAL), id(4), id(NAS)]),
            "the current configuration restores the route of the node"
        );
    }

    #[tokio::test]
    async fn a_publication_that_is_admitted_finishes_before_the_exclusive_invalidation_returns() {
        let (hub, state) = node(LOCAL);
        let nas = super::super::admission::mint_for_test(id(NAS), 1);
        let (nas_tx, _nas_rx) = super::envelope_channel();
        let (nas_shutdown, _nas_shutdown_rx) = tokio::sync::watch::channel(false);
        assert!(hub.register_link(&nas, nas_tx, nas_shutdown).await);
        let path = PeerPath::new(vec![id(NAS)]).expect("a bounded path");

        // The relay state holds its workers, so the actor blocks when it
        // publishes the worker of the route.
        let workers = state.inner.workers.lock().await;
        let advertisement = {
            let hub = hub.clone();
            let nas = nas.clone();
            let path = path.clone();
            tokio::spawn(async move { hub.apply_update(&nas, 1, &path, snapshot(1)).await })
        };
        // The snapshot of the route is applied before the worker is
        // published, so the actor is inside the publication of the route.
        wait_until(|| async { *state.inner.config_version.lock().await == Some(1) }).await;

        // The admission of the link dies exclusively: it waits for the
        // publication that is already admitted.
        let dying = nas.clone();
        let mut invalidation = tokio::spawn(async move { dying.invalidate_exclusive().await });
        assert!(
            tokio::time::timeout(Duration::from_millis(200), &mut invalidation)
                .await
                .is_err(),
            "the invalidation waits for the publication that is already admitted"
        );

        // The publication finishes, so the worker of the route is published
        // before the invalidation returns.
        drop(workers);
        invalidation.await.expect("the invalidation returns");
        assert_eq!(
            state.inner.workers.lock().await.len(),
            1,
            "the admitted publication finished before the invalidation returned"
        );

        // A stale advertisement of the dead admission never installs a
        // snapshot or a worker.
        assert!(
            hub.apply_update(&nas, 2, &path, snapshot(2)).await.is_err(),
            "the advertisement of a dead admission is refused"
        );
        assert_eq!(*state.inner.config_version.lock().await, Some(1));
        assert_eq!(state.inner.workers.lock().await.len(), 1);
        drop(advertisement);
    }

    #[tokio::test]
    async fn the_egress_of_a_peer_whose_invalidation_is_pending_is_refused_without_waiting() {
        let (hub, _state) = node(LOCAL);
        let nas = super::super::admission::mint_for_test(id(NAS), 1);
        let (nas_tx, _nas_rx) = super::envelope_channel();
        let (nas_shutdown, _nas_shutdown_rx) = tokio::sync::watch::channel(false);
        assert!(hub.register_link(&nas, nas_tx, nas_shutdown).await);
        let peer = super::super::admission::mint_for_test(id(MAC), 1);
        let (peer_tx, mut peer_rx) = super::envelope_channel();
        let (peer_shutdown, _peer_shutdown_rx) = tokio::sync::watch::channel(false);
        assert!(hub.register_link(&peer, peer_tx, peer_shutdown).await);

        // The admission of the peer dies: the invalidation waits behind the
        // fence the test holds.
        let fence = peer.read_publication().await;
        let dying = peer.clone();
        let invalidation = tokio::spawn(async move { dying.invalidate_exclusive().await });
        tokio::time::sleep(Duration::from_millis(200)).await;

        // The advertisement of the route is refused for that peer, and the
        // actor never waits for it.
        let path = PeerPath::new(vec![id(NAS)]).expect("a bounded path");
        tokio::time::timeout(
            Duration::from_secs(5),
            hub.apply_update(&nas, 1, &path, snapshot(1)),
        )
        .await
        .expect("the actor never waits for the peer")
        .expect("the route is applied");
        assert!(
            tokio::time::timeout(Duration::from_millis(200), peer_rx.recv())
                .await
                .is_err(),
            "the frame of a peer whose admission is dying is not queued"
        );
        drop(fence);
        invalidation.await.expect("the invalidation returns");
    }

    #[tokio::test]
    async fn the_route_state_pins_the_first_database_node_and_refuses_another_one() {
        let mut state = RouteState::new(id(LOCAL));
        let path = PeerPath::new(vec![id(MAC), id(NAS)]).expect("a bounded path");
        state
            .apply_update(id(MAC), 1, 1, &path, snapshot(1))
            .expect("the route is applied");
        assert_eq!(state.terminal(), Some(id(NAS)));
        let other = PeerPath::new(vec![id(99), id(98)]).expect("a bounded path");
        assert!(
            state
                .apply_update(id(99), 1, 1, &other, snapshot(2))
                .is_err(),
            "the node never binds to another database node"
        );
    }

    /// A node with a database-less peer and its next hop.
    ///
    /// The downstream peer registers before the route exists, so the test also
    /// covers the later of the two startup orders.
    async fn routed_node() -> (
        Arc<RouteHub>,
        AppState,
        super::EnvelopeReceiver,
        super::EnvelopeReceiver,
    ) {
        let state = crate::relay::test_state();
        let hub = RouteHub::new(id(LOCAL), state.clone());
        let (mac_tx, mac_rx) = super::envelope_channel();
        let (shutdown, _rx) = tokio::sync::watch::channel(false);
        assert!(
            hub.register_link(
                &super::super::admission::mint_for_test(id(MAC), 2),
                mac_tx,
                shutdown,
            )
            .await
        );
        let (nas_tx, nas_rx) = super::envelope_channel();
        let (shutdown, _rx) = tokio::sync::watch::channel(false);
        assert!(
            hub.register_link(
                &super::super::admission::mint_for_test(id(NAS), 1),
                nas_tx,
                shutdown,
            )
            .await
        );
        let path = PeerPath::new(vec![id(NAS)]).expect("a bounded path");
        hub.apply_update(
            &super::super::admission::mint_for_test(id(NAS), 1),
            1,
            &path,
            snapshot(1),
        )
        .await
        .expect("the route is applied");
        (hub, state, nas_rx, mac_rx)
    }

    fn forwarded_path() -> PeerPath {
        PeerPath::new(vec![id(MAC), id(LOCAL), id(NAS)]).expect("a bounded path")
    }

    async fn next_envelope(receiver: &mut super::EnvelopeReceiver) -> PeerEnvelope {
        let queued = tokio::time::timeout(Duration::from_secs(2), receiver.recv())
            .await
            .expect("the link receives an envelope")
            .expect("the link is open");
        let (bytes, _permit) = queued.into_parts();
        PeerEnvelope::decode(&bytes).expect("the envelope decodes")
    }

    async fn next_bridge(receiver: &mut super::EnvelopeReceiver) -> BridgeMessage {
        loop {
            let envelope = next_envelope(receiver).await;
            if let Some(message) = envelope.message {
                if !matches!(message, BridgeMessage::ConfigSnapshot(_)) {
                    return message;
                }
            }
        }
    }

    #[tokio::test]
    async fn a_snapshot_refresh_keeps_the_epoch_and_the_running_worker() {
        let (hub, state, _nas, _mac) = routed_node().await;
        let epoch = hub.epoch().await;
        assert_eq!(state.inner.workers.lock().await.len(), 1);
        let path = PeerPath::new(vec![id(NAS)]).expect("a bounded path");
        let outcome = hub
            .apply_update(
                &super::super::admission::mint_for_test(id(NAS), 1),
                2,
                &path,
                snapshot(2),
            )
            .await
            .expect("the snapshot is applied");
        assert_eq!(outcome, super::RouteOutcome::SnapshotRefreshed);
        assert_eq!(hub.epoch().await, epoch, "a refresh keeps the epoch");
        assert_eq!(state.inner.workers.lock().await.len(), 1);
        assert_eq!(
            *state.inner.config_version.lock().await,
            Some(2),
            "the newest snapshot is applied"
        );
    }

    #[tokio::test]
    async fn a_client_cancel_finishes_its_owner_and_cancels_only_that_upstream_request() {
        let (hub, _state, mut nas, _mac) = routed_node().await;
        let path = forwarded_path();
        for request in ["a", "b"] {
            hub.forward_start(
                &super::super::admission::mint_for_test(id(MAC), 2),
                path.clone(),
                http_start(request),
            )
            .await
            .expect("the start is forwarded");
        }
        assert_eq!(hub.owner_count().await, 2);
        let mut upstream = Vec::new();
        for _ in 0..2 {
            match next_bridge(&mut nas).await {
                BridgeMessage::RequestStart(start) => upstream.push(start.request_id),
                other => panic!("unexpected frame: {other:?}"),
            }
        }

        let cancel: BridgeMessage = serde_json::from_value(serde_json::json!({
            "type":"request_cancel", "request_id":"a", "reason":"client_cancelled"
        }))
        .unwrap();
        hub.handle_peer_message(&super::super::admission::mint_for_test(id(MAC), 2), cancel)
            .await;
        assert_eq!(
            hub.owner_count().await,
            1,
            "only the cancelled owner is gone"
        );
        let forwarded = next_bridge(&mut nas).await;
        let BridgeMessage::RequestCancel(cancel) = forwarded else {
            panic!("the upstream request is cancelled");
        };
        assert!(
            upstream.contains(&cancel.request_id),
            "the cancellation carries the upstream id of the request"
        );

        // The other request is untouched and still forwards its body.
        let chunk: BridgeMessage = serde_json::from_value(serde_json::json!({
            "type":"request_chunk", "request_id":"b", "data":[1,2,3]
        }))
        .unwrap();
        hub.handle_peer_message(&super::super::admission::mint_for_test(id(MAC), 2), chunk)
            .await;
        assert_eq!(hub.owner_count().await, 1);
        let BridgeMessage::RequestChunk(chunk) = next_bridge(&mut nas).await else {
            panic!("the body of the other request is forwarded");
        };
        assert!(upstream.contains(&chunk.request_id));
    }

    #[tokio::test]
    async fn a_duplicate_forwarded_start_never_replaces_or_cancels_the_owner() {
        let (hub, _state, mut nas, mut mac) = routed_node().await;
        let path = forwarded_path();
        hub.forward_start(
            &super::super::admission::mint_for_test(id(MAC), 2),
            path.clone(),
            http_start("dup"),
        )
        .await
        .expect("the first start is forwarded");
        hub.forward_start(
            &super::super::admission::mint_for_test(id(MAC), 2),
            path,
            http_start("dup"),
        )
        .await
        .expect("the duplicate is dropped");
        assert_eq!(hub.owner_count().await, 1);
        let first = next_bridge(&mut nas).await;
        assert!(matches!(first, BridgeMessage::RequestStart(_)));
        // The duplicate never reached the executor and no error crossed the
        // existing owner.
        match tokio::time::timeout(Duration::from_millis(100), nas.recv()).await {
            Err(_) => {}
            Ok(Some(queued)) => {
                let (bytes, _permit) = queued.into_parts();
                panic!(
                    "unexpected frame: {:?}",
                    PeerEnvelope::decode(&bytes).expect("decodes")
                );
            }
            Ok(None) => panic!("the queue closed"),
        }
        loop {
            match tokio::time::timeout(Duration::from_millis(100), mac.recv()).await {
                Err(_) => break,
                Ok(None) => break,
                Ok(Some(queued)) => {
                    let (bytes, _permit) = queued.into_parts();
                    let envelope = PeerEnvelope::decode(&bytes).expect("decodes");
                    match envelope.message {
                        None | Some(BridgeMessage::ConfigSnapshot(_)) => continue,
                        other => panic!("the existing request was cross-cancelled: {other:?}"),
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn an_executor_error_finishes_an_mcp_and_a_realtime_owner() {
        let (hub, _state, mut nas, mut mac) = routed_node().await;
        let path = forwarded_path();
        for message in [
            BridgeMessage::McpRequestStart(crate::protocol::McpRequestStart {
                request_id: "mcp-error".to_string(),
                server_name: None,
                method: "POST".to_string(),
                path: "/mcp".to_string(),
                headers: vec![],
                user_id: Some(1),
                client_key_hash: Some("hash".to_string()),
                http_request_content_encoding: None,
                http_request_compressed: false,
                http_request_compressed_bytes: None,
            }),
            BridgeMessage::RealtimeSessionStart(crate::protocol::RealtimeSessionStart {
                request_id: "realtime-error".to_string(),
                model: "gpt".to_string(),
                path: "/v1/realtime".to_string(),
                user_id: Some(1),
                route_id: None,
                client_key_hash: None,
                request_user_agent: None,
            }),
        ] {
            hub.forward_start(
                &super::super::admission::mint_for_test(id(MAC), 2),
                path.clone(),
                message,
            )
            .await
            .expect("the start is forwarded");
        }
        let mut upstream = Vec::new();
        for _ in 0..2 {
            match next_bridge(&mut nas).await {
                BridgeMessage::McpRequestStart(start) => upstream.push(start.request_id),
                BridgeMessage::RealtimeSessionStart(start) => upstream.push(start.request_id),
                other => panic!("unexpected frame: {other:?}"),
            }
        }
        assert_eq!(hub.owner_count().await, 2);

        for (upstream_id, downstream_id) in upstream.iter().zip(["mcp-error", "realtime-error"]) {
            let error: BridgeMessage = serde_json::from_value(serde_json::json!({
                "type":"response_error", "request_id":upstream_id, "status":502, "code":"upstream_failed", "message":"failed"
            }))
            .unwrap();
            hub.handle_peer_message(&super::super::admission::mint_for_test(id(NAS), 1), error)
                .await;
            let forwarded = next_bridge(&mut mac).await;
            let BridgeMessage::ResponseError(error) = forwarded else {
                panic!("the shared error reaches the downstream peer");
            };
            assert_eq!(error.request_id, downstream_id);
        }
        assert_eq!(hub.owner_count().await, 0);
    }

    #[tokio::test]
    async fn a_realtime_close_from_a_client_cancels_upstream_instead_of_answering() {
        let (hub, _state, mut nas, _mac) = routed_node().await;
        let start = BridgeMessage::RealtimeSessionStart(crate::protocol::RealtimeSessionStart {
            request_id: "rt".to_string(),
            model: "gpt".to_string(),
            path: "/v1/realtime".to_string(),
            user_id: Some(1),
            route_id: None,
            client_key_hash: None,
            request_user_agent: None,
        });
        hub.forward_start(
            &super::super::admission::mint_for_test(id(MAC), 2),
            forwarded_path(),
            start,
        )
        .await
        .expect("the realtime start is forwarded");
        let BridgeMessage::RealtimeSessionStart(start) = next_bridge(&mut nas).await else {
            panic!("the realtime start reaches the executor");
        };
        let close: BridgeMessage = serde_json::from_value(serde_json::json!({
            "type":"realtime_session_close", "request_id":"rt"
        }))
        .unwrap();
        hub.handle_peer_message(&super::super::admission::mint_for_test(id(MAC), 2), close)
            .await;
        assert_eq!(hub.owner_count().await, 0);
        let BridgeMessage::RealtimeSessionClose(close) = next_bridge(&mut nas).await else {
            panic!("the close is a cancellation upstream, not a response");
        };
        assert_eq!(close.request_id, start.request_id);
    }

    #[tokio::test]
    async fn a_local_realtime_close_through_the_virtual_worker_finishes_the_owner() {
        let (hub, state, mut nas, _mac) = routed_node().await;
        let worker = state
            .inner
            .workers
            .lock()
            .await
            .values()
            .next()
            .unwrap()
            .clone();
        let start = BridgeMessage::RealtimeSessionStart(crate::protocol::RealtimeSessionStart {
            request_id: "local-rt".to_string(),
            model: "gpt".to_string(),
            path: "/v1/realtime".to_string(),
            user_id: Some(1),
            route_id: None,
            client_key_hash: None,
            request_user_agent: None,
        });
        worker.try_send(start).unwrap();
        let BridgeMessage::RealtimeSessionStart(start) = next_bridge(&mut nas).await else {
            panic!("the local start reaches the executor");
        };
        let close = super::cancellation(super::RequestKind::Realtime, "local-rt");
        worker.try_send(close).unwrap();
        let BridgeMessage::RealtimeSessionClose(close) = next_bridge(&mut nas).await else {
            panic!("the local close reaches the executor");
        };
        assert_eq!(close.request_id, start.request_id);
        wait_until(|| async { hub.owner_count().await == 0 }).await;
        hub.release_link(id(NAS), 1).await;
        hub.release_link(id(MAC), 2).await;
    }

    #[tokio::test]
    async fn a_lost_downstream_link_cancels_its_work_without_answering_the_gone_peer() {
        let (hub, _state, mut nas, _mac) = routed_node().await;
        hub.forward_start(
            &super::super::admission::mint_for_test(id(MAC), 2),
            forwarded_path(),
            http_start("gone"),
        )
        .await
        .expect("the start is forwarded");
        let BridgeMessage::RequestStart(start) = next_bridge(&mut nas).await else {
            panic!("the start reaches the executor");
        };
        hub.release_link(id(MAC), 2).await;
        assert_eq!(hub.owner_count().await, 0, "both indexes are removed");
        let BridgeMessage::RequestCancel(cancel) = next_bridge(&mut nas).await else {
            panic!("the executor is told to stop the orphaned work");
        };
        assert_eq!(cancel.request_id, start.request_id);
    }

    #[tokio::test]
    async fn a_lost_upstream_link_fails_only_its_own_downstream_requests() {
        let (hub, _state, _nas, mut mac) = routed_node().await;
        hub.forward_start(
            &super::super::admission::mint_for_test(id(MAC), 2),
            forwarded_path(),
            http_start("lost"),
        )
        .await
        .expect("the start is forwarded");
        hub.release_link(id(NAS), 1).await;
        assert_eq!(hub.owner_count().await, 0);
        let BridgeMessage::ResponseError(error) = next_bridge(&mut mac).await else {
            panic!("the downstream peer learns that its request failed");
        };
        assert_eq!(error.request_id, "lost");
        assert_eq!(error.code, "peer_link_lost");
    }

    #[tokio::test]
    async fn a_route_withdrawal_cancels_the_in_flight_work_of_the_old_route() {
        let (hub, _state, mut nas, mut mac) = routed_node().await;
        hub.forward_start(
            &super::super::admission::mint_for_test(id(MAC), 2),
            forwarded_path(),
            http_start("old"),
        )
        .await
        .expect("the start is forwarded");
        let BridgeMessage::RequestStart(start) = next_bridge(&mut nas).await else {
            panic!("the start reaches the executor");
        };
        hub.apply_withdraw(&super::super::admission::mint_for_test(id(NAS), 1), 2)
            .await
            .expect("the route is withdrawn");
        assert_eq!(hub.owner_count().await, 0, "old-epoch work is not replayed");
        let BridgeMessage::RequestCancel(cancel) = next_bridge(&mut nas).await else {
            panic!("the old upstream request is cancelled");
        };
        assert_eq!(cancel.request_id, start.request_id);
        let BridgeMessage::ResponseError(error) = next_bridge(&mut mac).await else {
            panic!("the downstream peer learns that its request failed");
        };
        assert_eq!(error.code, "peer_route_changed");
    }

    /// Fill the owners of the node through the actor.
    ///
    /// The actor owns the owners of the node, so the test reaches them
    /// through the actor itself.
    async fn fill_owners(hub: &Arc<RouteHub>, count: usize) {
        let handle = hub.ensure_actor().await.expect("the actor of the hub");
        let (reply, wait) = oneshot::channel();
        handle
            .control
            .send(super::Command::FillOwners { count, reply })
            .await
            .expect("the command is queued");
        wait.await.expect("the actor answers");
    }

    #[tokio::test]
    async fn the_owner_bound_refuses_a_new_forwarded_start() {
        let (hub, state, _nas, _mac) = routed_node().await;
        fill_owners(&hub, super::MAX_FORWARDED_REQUESTS).await;
        assert!(
            hub.forward_start(
                &super::super::admission::mint_for_test(id(MAC), 2),
                forwarded_path(),
                http_start("over"),
            )
            .await
            .is_err(),
            "the node refuses a request over the bound"
        );
        assert_eq!(
            hub.owner_count().await,
            super::MAX_FORWARDED_REQUESTS,
            "no owner is admitted over the bound"
        );
        // The relay state keeps its virtual worker; only the admission was
        // refused.
        assert_eq!(state.inner.workers.lock().await.len(), 1);
    }

    #[tokio::test]
    async fn a_delayed_registration_of_a_replaced_link_is_refused_after_its_replacement_ended() {
        let (hub, _state) = node(LOCAL);
        let peer = id(NAS);

        // The first link of the peer is admitted.
        let old = super::super::admission::mint_for_test(peer, 1);
        let (old_tx, _old_rx) = super::envelope_channel();
        let (old_shutdown, _old_shutdown_rx) = tokio::sync::watch::channel(false);
        assert!(hub.register_link(&old, old_tx, old_shutdown).await);

        // The link is replaced: the replaced link loses its authority before
        // the replacement is registered.
        old.invalidate_exclusive().await;
        let new = super::super::admission::mint_for_test(peer, 2);
        let (new_tx, _new_rx) = super::envelope_channel();
        let (new_shutdown, _new_shutdown_rx) = tokio::sync::watch::channel(false);
        assert!(hub.register_link(&new, new_tx, new_shutdown).await);

        // The replacement ends too, so the hub knows no link of that peer.
        hub.release_link(peer, 2).await;
        let (late_tx, _late_rx) = super::envelope_channel();
        let (late_shutdown, _late_shutdown_rx) = tokio::sync::watch::channel(false);
        assert!(
            !hub.register_link(&old, late_tx, late_shutdown).await,
            "a replaced link never takes the place of its replacement"
        );
    }

    #[tokio::test]
    async fn a_stale_callback_of_a_replaced_link_publishes_no_route_and_cancels_nothing() {
        let (hub, state) = node(LOCAL);

        // The database node advertises its route.
        let nas = super::super::admission::mint_for_test(id(NAS), 1);
        let (nas_tx, mut nas_rx) = super::envelope_channel();
        let (nas_shutdown, _nas_shutdown_rx) = tokio::sync::watch::channel(false);
        assert!(hub.register_link(&nas, nas_tx, nas_shutdown).await);
        let path = PeerPath::new(vec![id(NAS)]).expect("a bounded path");
        hub.apply_update(&nas, 1, &path, snapshot(1))
            .await
            .expect("the route is applied");
        assert_eq!(hub.selected_path().await, Some(vec![id(LOCAL), id(NAS)]));

        // A peer of the node forwards a request over that route.
        let mac_old = super::super::admission::mint_for_test(id(MAC), 1);
        let (mac_tx, _mac_rx) = super::envelope_channel();
        let (mac_shutdown, _mac_shutdown_rx) = tokio::sync::watch::channel(false);
        assert!(hub.register_link(&mac_old, mac_tx, mac_shutdown).await);
        let forwarded = PeerPath::new(vec![id(MAC), id(LOCAL), id(NAS)]).expect("a bounded path");
        hub.forward_start(&mac_old, forwarded, http_start("live"))
            .await
            .expect("the start is forwarded");
        assert_eq!(hub.owner_count().await, 1);
        let BridgeMessage::RequestStart(start) = next_bridge(&mut nas_rx).await else {
            panic!("the request reaches the executor");
        };
        assert_ne!(start.request_id, "live");

        // The link of the peer is replaced: the replaced link loses its
        // authority.
        mac_old.invalidate_exclusive().await;
        let mac_new = super::super::admission::mint_for_test(id(MAC), 2);
        let (mac_tx, _mac_rx) = super::envelope_channel();
        let (mac_shutdown, _mac_shutdown_rx) = tokio::sync::watch::channel(false);
        assert!(hub.register_link(&mac_new, mac_tx, mac_shutdown).await);

        // A delayed advertisement of the replaced link arrives: it never
        // publishes a route and never installs a snapshot.
        let err = hub
            .apply_update(&mac_old, 2, &path, snapshot(2))
            .await
            .expect_err("the advertisement of a replaced link is refused");
        assert_eq!(err, super::RouteError::StaleGeneration);
        assert!(
            hub.apply_withdraw(&mac_old, 3).await.is_err(),
            "the withdrawal of a replaced link is refused"
        );
        assert_eq!(
            hub.selected_path().await,
            Some(vec![id(LOCAL), id(NAS)]),
            "the route of the node stays"
        );
        assert_ne!(
            *state.inner.config_version.lock().await,
            Some(2),
            "the snapshot of a replaced link never reaches the relay state"
        );

        // A delayed cancellation of the replaced link arrives: it never
        // cancels the request that the node still owns.
        let cancel: BridgeMessage = serde_json::from_value(serde_json::json!({
            "type":"request_cancel", "request_id":"live", "reason":"client_cancelled"
        }))
        .unwrap();
        hub.handle_peer_message(&mac_old, cancel).await;
        assert_eq!(
            hub.owner_count().await,
            1,
            "a replaced link never cancels the request of the node"
        );
        match tokio::time::timeout(Duration::from_millis(100), nas_rx.recv()).await {
            Err(_) => {}
            Ok(Some(queued)) => {
                let (bytes, _permit) = queued.into_parts();
                panic!(
                    "unexpected frame: {:?}",
                    PeerEnvelope::decode(&bytes).expect("decodes")
                );
            }
            Ok(None) => panic!("the queue closed"),
        }
    }

    #[tokio::test]
    async fn the_actor_starts_and_stops_without_leaving_a_task_behind() {
        let (hub, state) = node(LOCAL);
        let nas = super::super::admission::mint_for_test(id(NAS), 1);
        let (nas_tx, _nas_rx) = super::envelope_channel();
        let (nas_shutdown, _nas_shutdown_rx) = tokio::sync::watch::channel(false);
        assert!(hub.register_link(&nas, nas_tx, nas_shutdown).await);
        let path = PeerPath::new(vec![id(NAS)]).expect("a bounded path");
        hub.apply_update(&nas, 1, &path, snapshot(1))
            .await
            .expect("the route is applied");
        wait_until(|| async { !state.inner.workers.lock().await.is_empty() }).await;

        hub.stop().await;

        // The virtual worker of the node is released with the actor.
        wait_until(|| async { state.inner.workers.lock().await.is_empty() }).await;
        assert!(
            hub.selected_path().await.is_none(),
            "the actor of a stopped hub answers nothing"
        );
    }

    #[tokio::test]
    async fn the_traffic_of_the_ingress_is_bounded_by_its_byte_budget() {
        let state = crate::relay::test_state();
        // The budget of the ingress of the test is small, so the byte
        // permits of the queue are observable without a huge frame.
        let hub = RouteHub::with_budgets(id(LOCAL), state.clone(), 1024, 1024);
        let nas = super::super::admission::mint_for_test(id(NAS), 1);
        let (nas_tx, _nas_rx) = super::envelope_channel();
        let (nas_shutdown, _nas_shutdown_rx) = tokio::sync::watch::channel(false);
        assert!(hub.register_link(&nas, nas_tx, nas_shutdown).await);

        // A small frame fits the budget of the ingress.
        let small = BridgeMessage::ResponseEnd(crate::protocol::ResponseEnd {
            request_id: "small".to_string(),
        });
        assert!(
            hub.handle_peer_message(&nas, small).await,
            "a small frame enters the ingress of the actor"
        );

        // A frame that does not fit the byte budget of the ingress never
        // enters the queue of the actor.
        let handle = hub.ensure_actor().await.expect("the actor of the hub");
        let available_before = handle.data_budget.available_permits();
        let pending_before = handle.data.capacity();
        let huge = BridgeMessage::ResponseChunk(crate::protocol::ResponseChunk {
            request_id: "huge".to_string(),
            data: vec![7; 4096],
        });
        assert!(
            hub.handle_peer_message(&nas, huge).await,
            "the link of the peer survives the refusal of a frame over its budget"
        );
        assert!(
            handle.data_budget.available_permits() >= available_before,
            "the frame over the budget never entered the ingress of the node"
        );
        assert!(
            handle.data.capacity() >= pending_before,
            "the frame over the budget was never admitted, so it was never retried"
        );
    }

    #[tokio::test]
    async fn a_saturated_sink_of_one_peer_never_stops_the_healthy_peer() {
        let (hub, state) = node(LOCAL);

        // The next hop of the route is healthy.
        let nas = super::super::admission::mint_for_test(id(NAS), 1);
        let (nas_tx, mut nas_rx) = super::envelope_channel();
        let (nas_shutdown, _nas_shutdown_rx) = tokio::sync::watch::channel(false);
        assert!(hub.register_link(&nas, nas_tx, nas_shutdown).await);

        // A peer whose sink never drains.
        let mac = super::super::admission::mint_for_test(id(MAC), 2);
        let (mac_tx, _mac_rx) = super::envelope_channel();
        let (mac_shutdown, mac_shutdown_rx) = tokio::sync::watch::channel(false);
        let slow_sink = mac_tx.clone();
        assert!(hub.register_link(&mac, mac_tx, mac_shutdown).await);
        let filler = PeerEnvelope::bridge(BridgeMessage::Pong);
        for _ in 0..super::PEER_ENVELOPE_BUFFER {
            slow_sink
                .try_send(filler.clone())
                .expect("the slow sink accepts its frames");
        }

        // The database node advertises its route.
        let path = PeerPath::new(vec![id(NAS)]).expect("a bounded path");
        hub.apply_update(&nas, 1, &path, snapshot(1))
            .await
            .expect("the route is applied");
        assert_eq!(hub.selected_path().await, Some(vec![id(LOCAL), id(NAS)]));

        // The saturated peer is told to stop, because the node never waits
        // for a sink.
        wait_until(|| async { *mac_shutdown_rx.borrow() }).await;

        // The healthy peer keeps the route and the traffic of the node.
        let worker = {
            let workers = state.inner.workers.lock().await;
            workers
                .values()
                .next()
                .cloned()
                .expect("the virtual worker")
        };
        worker
            .send(http_start("local-1"))
            .await
            .expect("the request reaches the worker");
        let BridgeMessage::RequestStart(start) = next_bridge(&mut nas_rx).await else {
            panic!("the healthy peer receives the request");
        };
        assert_eq!(
            start.request_id, "local-1",
            "the request keeps the id of the relay state"
        );
        assert_eq!(
            hub.selected_path().await,
            Some(vec![id(LOCAL), id(NAS)]),
            "the route of the node survives the saturated peer"
        );
    }

    #[tokio::test]
    async fn the_cleanup_of_a_link_is_never_starved_by_a_full_data_ingress() {
        let (hub, state) = node(LOCAL);
        let nas = super::super::admission::mint_for_test(id(NAS), 1);
        let (nas_tx, _nas_rx) = super::envelope_channel();
        let (nas_shutdown, _nas_shutdown_rx) = tokio::sync::watch::channel(false);
        assert!(hub.register_link(&nas, nas_tx, nas_shutdown).await);

        // The actor blocks in the snapshot of the relay state, so the traffic
        // of the data path piles up.
        let version = state.inner.config_version.lock().await;
        let advertisement = {
            let hub = hub.clone();
            let nas = nas.clone();
            tokio::spawn(async move {
                let path = PeerPath::new(vec![id(NAS)]).expect("a bounded path");
                hub.apply_update(&nas, 1, &path, snapshot(1)).await
            })
        };
        tokio::time::sleep(Duration::from_millis(100)).await;

        // The data ingress fills up with traffic that waits for the actor.
        let handle = hub.ensure_actor().await.expect("the actor of the hub");
        let mut queued = 0usize;
        loop {
            let message = BridgeMessage::ResponseEnd(crate::protocol::ResponseEnd {
                request_id: format!("filler-{queued}"),
            });
            let Ok(permit) = handle
                .data_budget
                .clone()
                .try_acquire_many_owned(super::ingress_bytes(&message) as u32)
            else {
                break;
            };
            let (reply, _wait) = oneshot::channel();
            if handle
                .data
                .try_send(super::Command::HandlePeerMessage {
                    token: nas.clone(),
                    message,
                    permit,
                    reply,
                })
                .is_err()
            {
                break;
            }
            queued += 1;
            if queued > super::PEER_INGRESS_BUFFER {
                break;
            }
        }
        assert!(queued > 0, "the data ingress of the node accepts traffic");
        assert!(
            queued <= super::PEER_INGRESS_BUFFER,
            "the data ingress of the node is bounded"
        );
        assert!(
            handle.data.capacity() == 0,
            "the data ingress of the node is full"
        );

        // The cleanup of the link travels on the priority path, so it is
        // accepted while the data path is full.
        let (reply, wait) = oneshot::channel();
        handle
            .control
            .try_send(super::Command::ReleaseLink {
                peer: id(NAS),
                generation: 1,
                reply,
            })
            .expect("the priority path accepts the cleanup of the link");
        assert!(
            handle.control.capacity() < super::PEER_CONTROL_BUFFER,
            "the priority path holds the cleanup of the link"
        );

        // The actor runs the cleanup before the traffic that waits.
        drop(version);
        tokio::time::timeout(Duration::from_secs(5), wait)
            .await
            .expect("the cleanup of the link completes")
            .expect("the cleanup of the link runs");
        assert!(
            hub.selected_path().await.is_none(),
            "the cleanup of the link ran before the traffic that waited"
        );
        let _ = advertisement.await;
    }

    #[tokio::test]
    async fn a_caller_that_abandons_its_answer_never_cancels_the_transition() {
        let (hub, state) = node(LOCAL);
        let nas = super::super::admission::mint_for_test(id(NAS), 1);
        let (nas_tx, _nas_rx) = super::envelope_channel();
        let (nas_shutdown, _nas_shutdown_rx) = tokio::sync::watch::channel(false);
        assert!(hub.register_link(&nas, nas_tx, nas_shutdown).await);

        // The actor blocks in the snapshot of the relay state.
        let version = state.inner.config_version.lock().await;
        let path = PeerPath::new(vec![id(NAS)]).expect("a bounded path");
        let abandoned = tokio::time::timeout(
            Duration::from_millis(100),
            hub.apply_update(&nas, 1, &path, snapshot(1)),
        )
        .await;
        assert!(
            abandoned.is_err(),
            "the caller abandons the answer of its command"
        );

        // The transition of the actor completes, even though nobody waits
        // for the answer anymore.
        drop(version);
        wait_until(|| async { hub.selected_path().await.is_some() }).await;
        wait_until(|| async { *state.inner.config_version.lock().await == Some(1) }).await;
    }

    /// A writer that keeps the frames it is given.
    struct CollectingWriter {
        /// The frames the writer received.
        frames: Arc<Mutex<Vec<Vec<u8>>>>,
    }

    impl PeerWriter for CollectingWriter {
        async fn send_binary(&mut self, bytes: Vec<u8>) -> anyhow::Result<()> {
            self.frames.lock().expect("the frames lock").push(bytes);
            Ok(())
        }

        async fn flush(&mut self) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn a_burst_of_small_frames_fits_the_queue_of_a_peer_link() {
        let (tx, rx) = super::envelope_channel();
        let frames = Arc::new(Mutex::new(Vec::new()));
        let writer = CollectingWriter {
            frames: frames.clone(),
        };

        // The producer queues a burst of small frames that stays far below
        // the byte budget of the link.
        let mut expected = Vec::new();
        for index in 0..160 {
            let request_id = format!("burst-{index}");
            let envelope =
                PeerEnvelope::bridge(BridgeMessage::ResponseEnd(crate::protocol::ResponseEnd {
                    request_id: request_id.clone(),
                }));
            tx.try_send(envelope)
                .unwrap_or_else(|err| panic!("the queue accepts the burst: {err:?}"));
            expected.push(request_id);
        }

        // The production egress of the link delivers the burst in order.
        drop(tx);
        super::write_envelopes_to_peer(writer, rx)
            .await
            .expect("the egress writes the burst");
        let delivered = frames.lock().expect("the frames lock").clone();
        assert_eq!(
            delivered.len(),
            expected.len(),
            "the egress writes every frame of the burst"
        );
        let ids = delivered
            .iter()
            .map(|bytes| {
                let envelope = PeerEnvelope::decode(bytes).expect("the envelope decodes");
                match envelope.message {
                    Some(BridgeMessage::ResponseEnd(end)) => end.request_id,
                    _ => panic!("the envelope carries its response end"),
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(ids, expected, "the egress delivers the burst in order");
    }

    #[tokio::test]
    async fn the_queue_of_a_peer_link_refuses_a_frame_over_its_capacity() {
        let (tx, mut rx) = super::envelope_channel();
        let filler = PeerEnvelope::bridge(BridgeMessage::Pong);
        for _ in 0..super::PEER_ENVELOPE_BUFFER {
            tx.try_send(filler.clone())
                .expect("the queue accepts its frames");
        }
        assert_eq!(
            tx.try_send(filler.clone()),
            Err(super::EnqueueError::QueueFull),
            "the queue refuses a frame over its capacity"
        );

        // A released slot accepts the next frame again.
        let released = rx.recv().await.expect("the queue holds a frame");
        drop(released);
        assert!(
            tx.try_send(filler).is_ok(),
            "a released slot accepts the next frame"
        );
    }

    #[tokio::test]
    async fn a_closed_queue_of_a_peer_link_is_not_reported_as_full() {
        let (tx, rx) = super::envelope_channel();
        drop(rx);
        assert_eq!(
            tx.try_send(PeerEnvelope::bridge(BridgeMessage::Pong)),
            Err(super::EnqueueError::QueueClosed),
            "a closed queue is not a full queue"
        );
    }

    #[tokio::test]
    async fn a_link_that_is_no_longer_admitted_is_never_reported_as_a_full_queue() {
        let peer = id(NAS);
        let token = super::super::admission::mint_for_test(peer, 1);
        let (tx, _rx) = super::envelope_channel();
        let (shutdown, _shutdown_rx) = tokio::sync::watch::channel(false);
        let link = super::Link {
            peer,
            generation: 1,
            tx,
            shutdown,
            token: token.clone(),
        };
        let mut hub = super::HubState::new(id(LOCAL));
        hub.links.insert(peer, link.clone());

        // A frame of a replaced generation finds no link to travel on.
        assert_eq!(
            super::admitted_link(&hub, &peer, 2).err(),
            Some(super::EnqueueError::LinkUnavailable),
            "a replaced link carries no frame"
        );
        assert!(
            super::admitted_link(&hub, &peer, 1).is_ok(),
            "the link of its own generation carries the frame"
        );

        // A dead link is refused before its queue is touched.
        token.mark_dead();
        assert_eq!(
            link.try_admit().err(),
            Some(super::EnqueueError::LinkUnavailable),
            "a dead link is not a full queue"
        );
        assert_ne!(
            super::EnqueueError::LinkUnavailable,
            super::EnqueueError::QueueFull,
            "a refused admission keeps its own reason"
        );
    }

    #[tokio::test]
    async fn a_burst_of_local_requests_survives_the_held_egress_of_a_peer_link() {
        let (hub, state) = node(LOCAL);
        let nas = super::super::admission::mint_for_test(id(NAS), 1);
        let (nas_tx, mut nas_rx) = super::envelope_channel();
        let (nas_shutdown, _nas_shutdown_rx) = tokio::sync::watch::channel(false);
        assert!(hub.register_link(&nas, nas_tx.clone(), nas_shutdown).await);
        let path = PeerPath::new(vec![id(NAS)]).expect("a bounded path");
        hub.apply_update(&nas, 1, &path, snapshot(1))
            .await
            .expect("the route is applied");

        // The egress of the link is held: the peer never reads its queue, so
        // the burst piles up in the queue of the link.
        let worker = {
            let workers = state.inner.workers.lock().await;
            workers
                .values()
                .next()
                .cloned()
                .expect("the virtual worker")
        };
        for index in 0..160 {
            worker
                .send(http_start(&format!("local-{index}")))
                .await
                .expect("the request reaches the worker");
            let expected = index + 1;
            wait_until(|| async { nas_tx.queue_state().pending_frames == expected }).await;
        }

        // The route and the owners of the burst survive the held egress.
        assert_eq!(
            hub.selected_path().await,
            Some(vec![id(LOCAL), id(NAS)]),
            "the route of the node survives the burst"
        );
        assert_eq!(
            hub.owner_count().await,
            160,
            "the owners of the burst survive the held egress"
        );

        // The egress of the link delivers the burst in order.
        for index in 0..160 {
            let BridgeMessage::RequestStart(start) = next_bridge(&mut nas_rx).await else {
                panic!("the peer receives the request of the burst");
            };
            assert_eq!(start.request_id, format!("local-{index}"));
        }
    }

    #[tokio::test]
    async fn a_snapshot_the_relay_state_refuses_withdraws_the_route() {
        let (hub, state) = node(LOCAL);
        let nas = super::super::admission::mint_for_test(id(NAS), 1);
        let (nas_tx, mut nas_rx) = super::envelope_channel();
        let (nas_shutdown, _nas_shutdown_rx) = tokio::sync::watch::channel(false);
        assert!(hub.register_link(&nas, nas_tx, nas_shutdown).await);

        // The relay state holds the maximum number of workers, so it refuses
        // the virtual worker of the route.
        {
            let (tx, _rx) = tokio::sync::mpsc::channel(1);
            let mut worker_id = 1000;
            while super::register_worker(&state, worker_id, tx.clone()).await {
                worker_id += 1;
            }
        }

        let path = PeerPath::new(vec![id(NAS)]).expect("a bounded path");
        hub.apply_update(&nas, 1, &path, snapshot(1))
            .await
            .expect("the route is applied");

        // The node cannot serve the route it advertised, so it ends the route
        // and its peers learn that it has none.
        wait_until(|| async { hub.selected_path().await.is_none() }).await;
        let withdrawal = loop {
            let envelope = next_envelope(&mut nas_rx).await;
            if envelope.kind
                == crate::worker::runtime::node::peer::wire::PeerEnvelopeKind::RouteWithdraw
            {
                break envelope;
            }
        };
        assert!(withdrawal.path.is_none());
    }

    #[tokio::test]
    async fn the_envelope_queue_accounts_bytes_and_releases_them_on_acceptance() {
        let probe =
            PeerEnvelope::bridge(BridgeMessage::ResponseEnd(crate::protocol::ResponseEnd {
                request_id: "x".to_string(),
            }));
        let size = probe.encode().expect("the probe encodes").len();
        let (tx, mut rx) = super::envelope_channel_with_budget(size);
        tx.send(probe.clone())
            .await
            .expect("the first envelope fits the budget");
        assert!(
            tokio::time::timeout(Duration::from_millis(100), tx.send(probe.clone()))
                .await
                .is_err(),
            "a full byte budget makes the producer wait"
        );
        let accepted = rx.recv().await.expect("the first envelope is queued");
        let (bytes, permit) = accepted.into_parts();
        assert_eq!(bytes.len(), size);
        drop(permit);
        tokio::time::timeout(Duration::from_secs(1), tx.send(probe))
            .await
            .expect("the acceptance releases the byte budget")
            .expect("the second envelope fits again");
    }

    /// A snapshot that carries the route of the local client.
    ///
    /// The relay of the node authenticates a managed client key with the
    /// routes of the snapshot, so the snapshot carries the route of the key
    /// of the local client.
    fn client_snapshot(token: &str) -> ConfigSnapshot {
        ConfigSnapshot {
            version: 1,
            keys: vec![ClientRoute {
                key_hash: crate::keys::hash_client_key(token),
                key_prefix: "pfy_".to_string(),
                user_id: 1,
                route_id: "route".to_string(),
            }],
            relay_ip_policy: Default::default(),
        }
    }

    /// The public router of a node without a database.
    ///
    /// The router serves the AI API of the local client, so the local
    /// delivery of the node is observed at the end of the local client.
    async fn local_client_router() -> (axum::Router, AppState) {
        let (router, state) = crate::relay::node_edge_apps(crate::config::RelayConfig::default())
            .await
            .expect("the public router of the node");
        (router, state)
    }

    /// A request of the local client of the node.
    fn client_request(token: &str) -> axum::http::Request<axum::body::Body> {
        let mut request = axum::http::Request::builder()
            .method("POST")
            .uri("/v1/responses")
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", "application/json")
            .body(axum::body::Body::from("{}"))
            .expect("the request of the local client is built");
        request
            .extensions_mut()
            .insert(axum::extract::ConnectInfo(crate::relay::RemoteAddr(
                std::net::SocketAddr::from(([127, 0, 0, 1], 4000)),
            )));
        request
    }

    /// A database node that answers the requests of the node.
    ///
    /// The database node advertises its route and answers every request with
    /// a body, or with a shared error when the answer fails.
    async fn run_answering_nas(
        mut reader: impl PeerReader,
        mut writer: impl PeerWriter,
        nas: NodeId,
        fails: bool,
    ) {
        let path = PeerPath::new(vec![nas]).expect("a bounded path");
        let advertisement = PeerEnvelope::route_update(path, 1, &client_snapshot(CLIENT_KEY));
        if writer
            .send_binary(advertisement.encode().expect("the envelope encodes"))
            .await
            .is_err()
        {
            return;
        }
        while let Ok(Some(frame)) = reader.recv().await {
            let PeerFrame::Binary(bytes) = frame else {
                continue;
            };
            let Ok(envelope) = PeerEnvelope::decode(&bytes) else {
                continue;
            };
            let Some(BridgeMessage::RequestStart(start)) = envelope.message else {
                continue;
            };
            let request_id = start.request_id;
            let answer = if fails {
                vec![BridgeMessage::ResponseError(
                    crate::protocol::ResponseError {
                        request_id: request_id.clone(),
                        status: 502,
                        code: "upstream_failed".to_string(),
                        message: "the database node failed".to_string(),
                    },
                )]
            } else {
                vec![
                    BridgeMessage::ResponseStart(ResponseStart {
                        request_id: request_id.clone(),
                        status: 200,
                        content_type: Some("text/plain".to_string()),
                        headers: vec![],
                    }),
                    BridgeMessage::ResponseChunk(crate::protocol::ResponseChunk {
                        request_id: request_id.clone(),
                        data: b"the answer of the route".to_vec(),
                    }),
                    BridgeMessage::ResponseEnd(ResponseEnd {
                        request_id: request_id.clone(),
                    }),
                ]
            };
            for message in answer {
                let _ = writer
                    .send_binary(
                        PeerEnvelope::bridge(message)
                            .encode()
                            .expect("the envelope encodes"),
                    )
                    .await;
            }
        }
    }

    /// A node with a database node as its next hop.
    ///
    /// The node serves the local client of the relay state and forwards every
    /// request to the database node.
    async fn node_with_a_database_peer(
        fails: bool,
    ) -> (
        axum::Router,
        Arc<RouteHub>,
        AppState,
        oneshot::Sender<()>,
        tokio::task::JoinHandle<anyhow::Result<()>>,
        tokio::task::JoinHandle<()>,
    ) {
        let (router, state) = local_client_router().await;
        let hub = RouteHub::new(id(LOCAL), state.clone());
        let (nas_side, node_side) = MemoryPeer::pair(64);
        let (nas_reader, nas_writer) = nas_side.into_halves();
        let nas_task = tokio::spawn(run_answering_nas(nas_reader, nas_writer, id(NAS), fails));
        let (node_reader, node_writer) = node_side.into_halves();
        let (stop_tx, stop_rx) = oneshot::channel();
        let node_task = tokio::spawn(run_node_peer_session(
            node_reader,
            node_writer,
            hub.clone(),
            super::super::admission::mint_for_test(id(NAS), 1),
            Duration::from_secs(30),
            stop_rx,
        ));
        // The node knows the route and published the virtual worker of the
        // route, so the local client reaches the node.
        wait_until(|| async { hub.selected_path().await.is_some() }).await;
        wait_until(|| async { !state.inner.workers.lock().await.is_empty() }).await;
        (router, hub, state, stop_tx, node_task, nas_task)
    }

    #[tokio::test]
    async fn the_answer_of_the_route_reaches_the_local_client_of_the_node() {
        let (router, hub, _state, stop_tx, node_task, nas_task) =
            node_with_a_database_peer(false).await;

        // The local client asks the node, and the answer of the database node
        // reaches the local client.
        let request = client_request(CLIENT_KEY);
        let response = tokio::time::timeout(
            Duration::from_secs(10),
            tower::ServiceExt::oneshot(router, request),
        )
        .await
        .expect("the local client is answered")
        .expect("the router answers the local client");
        assert_eq!(response.status(), 200);
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("the body of the answer");
        assert_eq!(
            body.as_ref(),
            b"the answer of the route",
            "the answer of the database node reaches the local client"
        );
        assert_eq!(hub.owner_count().await, 0, "the request is retired");

        stop_tx.send(()).ok();
        let _ = node_task.await;
        nas_task.abort();
    }

    #[tokio::test]
    async fn the_error_of_the_route_reaches_the_local_client_of_the_node() {
        let (router, _hub, _state, stop_tx, node_task, nas_task) =
            node_with_a_database_peer(true).await;

        // The local client asks the node, and the shared error of the
        // database node reaches the local client.
        let request = client_request(CLIENT_KEY);
        let response = tokio::time::timeout(
            Duration::from_secs(10),
            tower::ServiceExt::oneshot(router, request),
        )
        .await
        .expect("the local client is answered")
        .expect("the router answers the local client");
        assert!(
            response.status().is_server_error(),
            "the error of the database node reaches the local client: {}",
            response.status()
        );

        stop_tx.send(()).ok();
        let _ = node_task.await;
        nas_task.abort();
    }

    #[tokio::test]
    async fn a_compressed_snapshot_that_fits_the_control_budget_applies_even_when_its_json_is_bigger()
     {
        // The control path of the test is small, so the difference between
        // the JSON of a snapshot and the frame that really travels over the
        // link is observable.
        let state = crate::relay::test_state();
        let budget = 64 * 1024;
        let hub = RouteHub::with_budgets(id(LOCAL), state.clone(), 1024 * 1024, budget);
        let nas = super::super::admission::mint_for_test(id(NAS), 1);
        let (nas_tx, _nas_rx) = super::envelope_channel();
        let (nas_shutdown, _nas_shutdown_rx) = tokio::sync::watch::channel(false);
        assert!(hub.register_link(&nas, nas_tx, nas_shutdown).await);
        let path = PeerPath::new(vec![id(NAS)]).expect("a bounded path");

        // A snapshot of many compressible routes: its JSON is bigger than the
        // budget of the control path, its encoded frame is not.
        let mut big = snapshot(1);
        big.keys = (0..512)
            .map(|index| ClientRoute {
                key_hash: format!("{index:04}{}", "pfy_".repeat(64)),
                key_prefix: "pfy_".to_string(),
                user_id: index as i64,
                route_id: "route".to_string(),
            })
            .collect();
        let json = serde_json::to_vec(&big).expect("the snapshot encodes");
        let encoded = PeerEnvelope::route_update(path.clone(), 1, &big)
            .encode()
            .expect("the advertisement encodes");
        assert!(
            json.len() > budget,
            "the JSON of the snapshot is bigger than the control budget"
        );
        assert!(
            encoded.len() <= budget,
            "the encoded frame of the snapshot fits the control budget: {} bytes",
            encoded.len()
        );

        // The advertisement applies, so a valid compressed snapshot is never
        // refused because of the size of its JSON.
        hub.apply_update(&nas, 1, &path, big)
            .await
            .expect("the route is applied");
        assert_eq!(hub.selected_path().await, Some(vec![id(LOCAL), id(NAS)]));
    }

    #[tokio::test]
    async fn an_encoded_advertisement_that_exceeds_the_control_budget_is_refused_at_once() {
        let state = crate::relay::test_state();
        let budget = 4096;
        let hub = RouteHub::with_budgets(id(LOCAL), state.clone(), 1024 * 1024, budget);
        let nas = super::super::admission::mint_for_test(id(NAS), 1);
        let (nas_tx, _nas_rx) = super::envelope_channel();
        let (nas_shutdown, _nas_shutdown_rx) = tokio::sync::watch::channel(false);
        assert!(hub.register_link(&nas, nas_tx, nas_shutdown).await);

        // A frame that is bigger than the whole budget of the control path
        // can never be admitted, so it is refused instead of waiting for a
        // budget that never frees.
        let refused = tokio::time::timeout(
            Duration::from_secs(5),
            hub.apply_update_bytes(&nas, vec![0_u8; budget + 1]),
        )
        .await
        .expect("the oversized advertisement is refused at once");
        assert!(refused.is_err());
        assert!(hub.selected_path().await.is_none());
    }

    #[tokio::test]
    async fn the_cleanup_of_a_link_is_idempotent_and_never_touches_its_replacement() {
        let (hub, _state) = node(LOCAL);
        let nas = id(NAS);
        let path = PeerPath::new(vec![nas]).expect("a bounded path");

        // The first generation of the link of the database node.
        let (tx, _rx) = super::envelope_channel();
        let (shutdown, shutdown_rx) = tokio::sync::watch::channel(false);
        assert!(
            hub.register_link(
                &super::super::admission::mint_for_test(nas, 1),
                tx,
                shutdown
            )
            .await
        );
        hub.apply_update(
            &super::super::admission::mint_for_test(nas, 1),
            1,
            &path,
            snapshot(1),
        )
        .await
        .expect("the route is applied");
        assert_eq!(hub.selected_path().await, Some(vec![id(LOCAL), nas]));

        // The cleanup of the generation withdraws the route and tells the
        // link to stop.
        hub.release_link(nas, 1).await;
        assert_eq!(hub.selected_path().await, None);
        wait_until(|| async { *shutdown_rx.borrow() }).await;

        // The link is replaced by a newer generation.
        let (tx, _rx) = super::envelope_channel();
        let (shutdown, shutdown_rx) = tokio::sync::watch::channel(false);
        assert!(
            hub.register_link(
                &super::super::admission::mint_for_test(nas, 2),
                tx,
                shutdown
            )
            .await
        );
        hub.apply_update(
            &super::super::admission::mint_for_test(nas, 2),
            2,
            &path,
            snapshot(2),
        )
        .await
        .expect("the route is applied");
        assert_eq!(hub.selected_path().await, Some(vec![id(LOCAL), nas]));

        // The cleanup of the dead generation never touches the link that
        // replaced it.
        hub.release_link(nas, 1).await;
        hub.invalidate_link(nas, 1)
            .await
            .expect("the dead link is invalidated");
        assert_eq!(
            hub.selected_path().await,
            Some(vec![id(LOCAL), nas]),
            "the route of the replacement survives the cleanup of the dead link"
        );
        assert!(
            !*shutdown_rx.borrow(),
            "the link of the replacement stays alive"
        );

        // The cleanup of the live generation is idempotent.
        hub.release_link(nas, 2).await;
        hub.release_link(nas, 2).await;
        assert_eq!(hub.selected_path().await, None);
        wait_until(|| async { *shutdown_rx.borrow() }).await;
        assert_eq!(
            hub.owner_count().await,
            0,
            "the cleanup leaves no owner behind"
        );
    }

    #[tokio::test]
    async fn the_failure_of_the_virtual_worker_tears_its_epoch_down() {
        let (hub, state, _nas, _mac) = routed_node().await;
        // The node published its virtual worker for the route.
        wait_until(|| async { !state.inner.workers.lock().await.is_empty() }).await;
        let worker_id = {
            *state
                .inner
                .workers
                .lock()
                .await
                .keys()
                .next()
                .expect("the virtual worker of the route")
        };
        // A local request of the relay state is owned by the node.
        let worker = {
            state
                .inner
                .workers
                .lock()
                .await
                .values()
                .next()
                .cloned()
                .expect("the virtual worker of the route")
        };
        worker
            .send(http_start("local-1"))
            .await
            .expect("the request reaches the worker");
        wait_until(|| async { hub.owner_count().await == 1 }).await;

        // The worker cannot take the traffic of the relay state.
        state.inner.signal_worker_failure(worker_id);

        // The worker is released and the requests it owned are failed.
        wait_until(|| async { state.inner.workers.lock().await.is_empty() }).await;
        wait_until(|| async { hub.owner_count().await == 0 }).await;
        // The link the traffic travelled on is stopped, so the node serves no
        // route any more and the database node cancels the work of that link.
        wait_until(|| async { hub.selected_path().await.is_none() }).await;
        // The failed request is never replayed on another route.
        assert_eq!(hub.owner_count().await, 0);
        // The watch of the failed worker is gone with the worker.
        assert_eq!(
            state
                .inner
                .worker_failures
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .len(),
            0,
            "the watch of the failed worker is removed"
        );
    }

    #[tokio::test]
    async fn a_stale_failure_of_a_replaced_worker_changes_nothing() {
        let (hub, state, _nas, _mac) = routed_node().await;
        wait_until(|| async { !state.inner.workers.lock().await.is_empty() }).await;
        let first_worker = {
            *state
                .inner
                .workers
                .lock()
                .await
                .keys()
                .next()
                .expect("the virtual worker of the route")
        };

        // The route of the old epoch ends and comes back, so the node
        // publishes a new worker for it.
        let path = PeerPath::new(vec![id(NAS)]).expect("a bounded path");
        hub.apply_withdraw(&super::super::admission::mint_for_test(id(NAS), 1), 2)
            .await
            .expect("the route is withdrawn");
        wait_until(|| async { hub.selected_path().await.is_none() }).await;
        hub.apply_update(
            &super::super::admission::mint_for_test(id(NAS), 1),
            3,
            &path,
            snapshot(2),
        )
        .await
        .expect("the route is applied");
        wait_until(|| async { hub.selected_path().await.is_some() }).await;
        let second_worker = {
            *state
                .inner
                .workers
                .lock()
                .await
                .keys()
                .next()
                .expect("the virtual worker of the new route")
        };
        assert_ne!(
            first_worker, second_worker,
            "the route publishes a new worker"
        );

        // The watch of the replaced worker is gone with the worker.
        assert_eq!(
            state
                .inner
                .worker_failures
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .len(),
            1,
            "only the watch of the current worker is live"
        );

        // The failure of the worker that was replaced changes nothing.
        state.inner.signal_worker_failure(first_worker);
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(
            hub.selected_path().await,
            Some(vec![id(LOCAL), id(NAS)]),
            "the route of the replacement survives the failure of the old worker"
        );
        assert_eq!(
            state.inner.workers.lock().await.len(),
            1,
            "the worker of the replacement survives the failure of the old worker"
        );
    }

    #[tokio::test]
    async fn a_queued_local_request_of_a_replaced_worker_never_runs_on_the_new_route() {
        let (hub, state, mut nas, _mac) = routed_node().await;
        wait_until(|| async { !state.inner.workers.lock().await.is_empty() }).await;
        let handle = hub.ensure_actor().await.expect("the actor of the hub");

        // The actor blocks in the snapshot of the relay state, so the local
        // request of the relay state and the change of the route wait in the
        // ingress of the actor.
        let version = state.inner.config_version.lock().await;
        let path = PeerPath::new(vec![id(NAS)]).expect("a bounded path");
        let refresh = {
            let hub = hub.clone();
            let nas = super::super::admission::mint_for_test(id(NAS), 1);
            let path = path.clone();
            tokio::spawn(async move { hub.apply_update(&nas, 2, &path, snapshot(2)).await })
        };
        // The local request of the relay state waits in the traffic path.
        let worker = {
            state
                .inner
                .workers
                .lock()
                .await
                .values()
                .next()
                .cloned()
                .expect("the virtual worker of the route")
        };
        worker
            .send(http_start("local-1"))
            .await
            .expect("the request reaches the worker");
        // The route of the worker ends and comes back, so the node publishes
        // a new worker for it.
        let withdraw = {
            let hub = hub.clone();
            let nas = super::super::admission::mint_for_test(id(NAS), 1);
            tokio::spawn(async move { hub.apply_withdraw(&nas, 3).await })
        };
        let reapply = {
            let hub = hub.clone();
            let nas = super::super::admission::mint_for_test(id(NAS), 1);
            tokio::spawn(async move { hub.apply_update(&nas, 4, &path, snapshot(3)).await })
        };
        // The change of the route waits in the control path, which the actor
        // reads before the traffic path.
        wait_until(|| async { handle.control.capacity() <= super::PEER_CONTROL_BUFFER - 2 }).await;
        drop(version);
        let _ = refresh.await;
        let _ = withdraw.await;
        let _ = reapply.await;
        wait_until(|| async { hub.selected_path().await.is_some() }).await;

        // The queued request of the replaced worker never runs on the route
        // of the new worker.
        assert_eq!(
            hub.owner_count().await,
            0,
            "the retired request owns nothing"
        );
        // The database node only receives advertisements of the node, never
        // the retired request itself.
        let deadline = tokio::time::Instant::now() + Duration::from_millis(300);
        while tokio::time::Instant::now() < deadline {
            let Ok(Some(queued)) = tokio::time::timeout_at(deadline, nas.recv()).await else {
                break;
            };
            let (bytes, _permit) = queued.into_parts();
            let envelope = PeerEnvelope::decode(&bytes).expect("the envelope decodes");
            assert!(
                !matches!(envelope.message, Some(BridgeMessage::RequestStart(_))),
                "the retired request never reaches the database node"
            );
        }
    }

    #[tokio::test]
    async fn a_refused_local_continuation_retires_only_its_own_request() {
        let (hub, state, mut nas_rx, _mac_rx) = routed_node().await;
        wait_until(|| async { !state.inner.workers.lock().await.is_empty() }).await;
        let handle = hub.ensure_actor().await.expect("the actor of the hub");
        let worker = {
            state
                .inner
                .workers
                .lock()
                .await
                .values()
                .next()
                .cloned()
                .expect("the virtual worker of the route")
        };
        worker
            .send(http_start("local-1"))
            .await
            .expect("the request reaches the worker");
        wait_until(|| async { hub.owner_count().await == 1 }).await;

        // The byte budget of the ingress of the node is exhausted, so the
        // traffic of the relay state cannot enter the actor any more.
        let _budget = handle
            .data_budget
            .clone()
            .acquire_many_owned(super::PEER_INGRESS_BUDGET_BYTES as u32)
            .await
            .expect("the budget of the ingress");
        assert_eq!(
            handle.data_budget.available_permits(),
            0,
            "the ingress of the node is exhausted"
        );

        // A continuation of the running request cannot enter the ingress.
        worker
            .send(BridgeMessage::RequestEnd(
                crate::protocol::BridgeRequestEnd {
                    request_id: "local-1".to_string(),
                    http_request_compressed_bytes: None,
                    http_request_decompressed_bytes: None,
                    http_request_compression_ratio: None,
                },
            ))
            .await
            .expect("the frame reaches the worker");

        // The request is retired, so it owns nothing any more.
        wait_until(|| async { hub.owner_count().await == 0 }).await;

        // The upstream work of the retired request is cancelled once.
        let frames = drain(&mut nas_rx).await;
        let cancels = frames
            .iter()
            .filter(|envelope| matches!(envelope.message, Some(BridgeMessage::RequestCancel(_))))
            .count();
        assert_eq!(
            cancels, 1,
            "the upstream work of the retired request is cancelled once"
        );

        // The worker of the route, the route and its snapshot survive the
        // refusal: only the request of the refused frame is retired.
        assert!(
            !state.inner.workers.lock().await.is_empty(),
            "the worker of the route survives the refusal"
        );
        assert_eq!(
            hub.selected_path().await,
            Some(vec![id(LOCAL), id(NAS)]),
            "the route of the node survives the refusal"
        );
        assert_eq!(
            *state.inner.config_version.lock().await,
            Some(1),
            "the snapshot of the route survives the refusal"
        );
    }

    #[tokio::test]
    async fn a_refused_local_continuation_retires_its_request_when_the_data_channel_is_full() {
        let (hub, state, mut nas_rx, _mac_rx) = routed_node().await;
        wait_until(|| async { !state.inner.workers.lock().await.is_empty() }).await;
        let handle = hub.ensure_actor().await.expect("the actor of the hub");
        let worker = {
            state
                .inner
                .workers
                .lock()
                .await
                .values()
                .next()
                .cloned()
                .expect("the virtual worker of the route")
        };
        // The affected request and an unrelated request run on the route.
        worker
            .send(http_start("affected"))
            .await
            .expect("the request reaches the worker");
        worker
            .send(http_start("unrelated"))
            .await
            .expect("the request reaches the worker");
        wait_until(|| async { hub.owner_count().await == 2 }).await;

        // The actor blocks in the snapshot of the relay state, so the traffic
        // of the data path piles up.
        let version = state.inner.config_version.lock().await;
        let path = PeerPath::new(vec![id(NAS)]).expect("a bounded path");
        let advertisement = tokio::spawn({
            let hub = hub.clone();
            let nas = super::super::admission::mint_for_test(id(NAS), 1);
            async move { hub.apply_update(&nas, 2, &path, snapshot(2)).await }
        });
        // The command waits on the priority path and the actor takes it, so
        // the actor cannot consume the data path any more.
        wait_until(|| async {
            handle.control_budget.available_permits() < super::PEER_CONTROL_BUDGET_BYTES
        })
        .await;
        wait_until(|| async { handle.control.capacity() == super::PEER_CONTROL_BUFFER }).await;

        // The data channel of the actor holds every slot, and the byte budget
        // of the ingress is untouched.
        let queued = fill_data_channel(&handle, id(NAS), super::PEER_INGRESS_BUFFER);
        assert_eq!(
            queued,
            super::PEER_INGRESS_BUFFER,
            "the data channel of the node is full"
        );
        assert_eq!(
            handle.data.capacity(),
            0,
            "the data channel of the node is full"
        );
        assert!(
            handle.data_budget.available_permits() > 0,
            "the byte budget of the ingress is not exhausted"
        );

        // A continuation of the affected request cannot enter the ingress.
        worker
            .send(BridgeMessage::RequestChunk(
                crate::protocol::BridgeRequestChunk {
                    request_id: "affected".to_string(),
                    data: vec![7; 8],
                },
            ))
            .await
            .expect("the frame reaches the worker");
        // The retirement waits on the priority path, so it runs as soon as the
        // actor is free again.
        wait_until(|| async { handle.control.capacity() < super::PEER_CONTROL_BUFFER }).await;
        drop(version);
        let _ = advertisement.await;

        // Only the affected request is retired.
        wait_until(|| async { hub.owner_count().await == 1 }).await;

        // The upstream work of the retired request is cancelled once.
        let frames = drain(&mut nas_rx).await;
        let cancels = frames
            .iter()
            .filter(|envelope| matches!(envelope.message, Some(BridgeMessage::RequestCancel(_))))
            .count();
        assert_eq!(
            cancels, 1,
            "the upstream work of the retired request is cancelled once"
        );

        // The worker of the route, the route and its snapshot survive the
        // refusal.
        assert!(
            !state.inner.workers.lock().await.is_empty(),
            "the worker of the route survives the refusal"
        );
        assert_eq!(
            hub.selected_path().await,
            Some(vec![id(LOCAL), id(NAS)]),
            "the route of the node survives the refusal"
        );
        assert_eq!(
            *state.inner.config_version.lock().await,
            Some(2),
            "the snapshot of the route survives the refusal"
        );

        // The unrelated request can still finish.
        let nas_token = super::super::admission::mint_for_test(id(NAS), 1);
        assert!(
            hub.handle_peer_message(
                &nas_token,
                BridgeMessage::ResponseEnd(crate::protocol::ResponseEnd {
                    request_id: "unrelated".to_string(),
                }),
            )
            .await,
            "the answer of the unrelated request enters the ingress"
        );
        wait_until(|| async { hub.owner_count().await == 0 }).await;

        // A request that comes after the capacity drained works again.
        worker
            .send(http_start("later"))
            .await
            .expect("the request reaches the worker");
        wait_until(|| async { hub.owner_count().await == 1 }).await;
    }

    /// Block the actor of the hub in the snapshot of the relay state.
    ///
    /// The actor cannot consume the data path while it waits for the
    /// configuration version, so the traffic of the relay state piles up in
    /// the data channel of the actor.
    async fn block_the_actor<'a>(
        hub: &Arc<RouteHub>,
        state: &'a AppState,
        handle: &super::ActorHandle,
        generation: u64,
    ) -> (
        tokio::sync::MutexGuard<'a, Option<i64>>,
        tokio::task::JoinHandle<Result<super::RouteOutcome, super::RouteError>>,
    ) {
        let version = state.inner.config_version.lock().await;
        let path = PeerPath::new(vec![id(NAS)]).expect("a bounded path");
        let advertisement = tokio::spawn({
            let hub = hub.clone();
            let nas = super::super::admission::mint_for_test(id(NAS), 1);
            async move {
                hub.apply_update(&nas, generation, &path, snapshot(generation as i64))
                    .await
            }
        });
        // The command waits on the priority path and the actor takes it, so
        // the actor cannot consume the data path any more.
        wait_until(|| async {
            handle.control_budget.available_permits() < super::PEER_CONTROL_BUDGET_BYTES
        })
        .await;
        wait_until(|| async { handle.control.capacity() == super::PEER_CONTROL_BUFFER }).await;
        (version, advertisement)
    }

    /// Fill the control channel of the actor.
    ///
    /// The actor is blocked, so the commands pile up in the control channel
    /// until it holds every slot.
    fn fill_control_channel(handle: &super::ActorHandle) -> usize {
        let mut queued = 0usize;
        while queued < super::PEER_CONTROL_BUFFER {
            let (reply, _wait) = oneshot::channel();
            if handle
                .control
                .try_send(super::Command::OwnerCount { reply })
                .is_err()
            {
                break;
            }
            queued += 1;
        }
        queued
    }

    #[tokio::test]
    async fn a_refusal_without_a_free_control_path_stops_the_link_of_the_peer() {
        let (hub, state, _nas_rx, _mac_rx) = routed_node().await;
        let mac = super::super::admission::mint_for_test(id(MAC), 2);
        hub.forward_start(&mac, forwarded_path(), http_start("peer-1"))
            .await
            .expect("the start of the peer is forwarded");
        wait_until(|| async { hub.owner_count().await == 1 }).await;
        let handle = hub.ensure_actor().await.expect("the actor of the hub");
        let (version, advertisement) = block_the_actor(&hub, &state, &handle, 2).await;

        // The data channel of the actor holds every slot, so the frame of the
        // peer cannot enter the ingress.
        let queued = fill_data_channel(&handle, id(NAS), super::PEER_INGRESS_BUFFER);
        assert_eq!(
            queued,
            super::PEER_INGRESS_BUFFER,
            "the data channel of the node is full"
        );
        // The control channel holds every slot too, so the retirement of the
        // request cannot be scheduled.
        let queued = fill_control_channel(&handle);
        assert_eq!(
            queued,
            super::PEER_CONTROL_BUFFER,
            "the control channel of the node is full"
        );

        // The link of the peer ends, because the retirement of the request
        // could not be scheduled.
        assert!(
            !hub.handle_peer_message(
                &mac,
                BridgeMessage::RequestChunk(crate::protocol::BridgeRequestChunk {
                    request_id: "peer-1".to_string(),
                    data: vec![7; 8],
                }),
            )
            .await,
            "the link of the peer ends without a free control path"
        );
        drop(version);
        let _ = advertisement.await;
    }

    #[tokio::test]
    async fn a_retirement_of_another_worker_epoch_keeps_the_request() {
        let (hub, state, _nas_rx, _mac_rx) = routed_node().await;
        wait_until(|| async { !state.inner.workers.lock().await.is_empty() }).await;
        let handle = hub.ensure_actor().await.expect("the actor of the hub");
        let worker = {
            state
                .inner
                .workers
                .lock()
                .await
                .values()
                .next()
                .cloned()
                .expect("the virtual worker of the route")
        };
        let worker_id = {
            let workers = state.inner.workers.lock().await;
            workers
                .keys()
                .next()
                .copied()
                .expect("the virtual worker of the route")
        };
        worker
            .send(http_start("local-1"))
            .await
            .expect("the request reaches the worker");
        wait_until(|| async { hub.owner_count().await == 1 }).await;

        // The retirement belongs to a worker that was replaced, so the
        // request of the current worker keeps its owner.
        let (reply, wait) = oneshot::channel();
        handle
            .control
            .try_send(super::Command::AbortLocal {
                worker_id,
                epoch: hub.epoch().await + 1,
                kind: super::RequestKind::Http,
                request_id: "local-1".to_string(),
                reply,
            })
            .expect("the retirement is queued");
        wait.await.expect("the retirement is answered");
        assert_eq!(
            hub.owner_count().await,
            1,
            "the request of the current worker keeps its owner"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_bounded_tracking_of_the_failed_flows_fails_the_worker_of_the_route() {
        let (hub, state, _nas_rx, _mac_rx) = routed_node().await;
        wait_until(|| async { !state.inner.workers.lock().await.is_empty() }).await;
        let handle = hub.ensure_actor().await.expect("the actor of the hub");
        let worker = {
            state
                .inner
                .workers
                .lock()
                .await
                .values()
                .next()
                .cloned()
                .expect("the virtual worker of the route")
        };

        // The byte budget of the ingress is exhausted, so the traffic of the
        // relay state cannot enter the ingress.
        let _budget = handle
            .data_budget
            .clone()
            .acquire_many_owned(super::PEER_INGRESS_BUDGET_BYTES as u32)
            .await
            .expect("the budget of the ingress");

        // Every message of a flow of its own is refused, so the bounded
        // tracking of the failed flows grows until it is exhausted. The test
        // hands one message over at a time and waits for the priority path to
        // be free again, so the retirement of every flow is carried by the
        // priority path.
        let mut refused = 0usize;
        for index in 0..(super::MAX_FORWARDED_REQUESTS + 2) {
            let message = BridgeMessage::RequestChunk(crate::protocol::BridgeRequestChunk {
                request_id: format!("flow-{index}"),
                data: vec![7; 8],
            });
            worker
                .send(message)
                .await
                .expect("the frame reaches the worker");
            wait_until(|| async { worker.capacity() == worker.max_capacity() }).await;
            wait_until(|| async { handle.control.capacity() == super::PEER_CONTROL_BUFFER }).await;
            refused += 1;
            if worker.is_closed() {
                break;
            }
        }

        // The tracking of the failed flows is exhausted, so the worker of the
        // route fails as it did before. The priority path never failed, so
        // the tracking was the bound that was reached.
        wait_until(|| async { worker.is_closed() }).await;
        assert!(
            refused > super::MAX_FORWARDED_REQUESTS,
            "the tracking of the failed flows was exhausted: {refused}"
        );
    }

    /// One message of every frame variant of the protocol.
    fn every_frame_variant() -> Vec<(FrameVariant, BridgeMessage)> {
        let id = "frame".to_string();
        let start = http_start("frame");
        vec![
            (FrameVariant::RequestStart, start.clone()),
            (
                FrameVariant::RequestChunk,
                BridgeMessage::RequestChunk(crate::protocol::BridgeRequestChunk {
                    request_id: id.clone(),
                    data: vec![1],
                }),
            ),
            (
                FrameVariant::RequestEnd,
                BridgeMessage::RequestEnd(crate::protocol::BridgeRequestEnd {
                    request_id: id.clone(),
                    http_request_compressed_bytes: None,
                    http_request_decompressed_bytes: None,
                    http_request_compression_ratio: None,
                }),
            ),
            (
                FrameVariant::RequestCancel,
                BridgeMessage::RequestCancel(crate::protocol::BridgeRequestCancel {
                    request_id: id.clone(),
                    reason: "client_cancel".to_string(),
                    response_started: false,
                }),
            ),
            (
                FrameVariant::McpRequestStart,
                BridgeMessage::McpRequestStart(crate::protocol::McpRequestStart {
                    request_id: id.clone(),
                    server_name: None,
                    method: "POST".to_string(),
                    path: "/mcp".to_string(),
                    headers: vec![],
                    user_id: None,
                    client_key_hash: None,
                    http_request_content_encoding: None,
                    http_request_compressed: false,
                    http_request_compressed_bytes: None,
                }),
            ),
            (
                FrameVariant::McpRequestChunk,
                BridgeMessage::McpRequestChunk(crate::protocol::McpRequestChunk {
                    request_id: id.clone(),
                    data: vec![1],
                }),
            ),
            (
                FrameVariant::McpRequestEnd,
                BridgeMessage::McpRequestEnd(crate::protocol::McpRequestEnd {
                    request_id: id.clone(),
                    http_request_compressed_bytes: None,
                    http_request_decompressed_bytes: None,
                    http_request_compression_ratio: None,
                }),
            ),
            (
                FrameVariant::McpRequestCancel,
                BridgeMessage::McpRequestCancel(crate::protocol::McpRequestCancel {
                    request_id: id.clone(),
                    reason: "client_cancel".to_string(),
                    response_started: false,
                }),
            ),
            (
                FrameVariant::RealtimeSessionStart,
                BridgeMessage::RealtimeSessionStart(crate::protocol::RealtimeSessionStart {
                    request_id: id.clone(),
                    model: "model".to_string(),
                    path: "/realtime".to_string(),
                    user_id: None,
                    route_id: None,
                    client_key_hash: None,
                    request_user_agent: None,
                }),
            ),
            (
                FrameVariant::RealtimeClientEvent,
                BridgeMessage::RealtimeClientEvent(crate::protocol::RealtimeClientEventMessage {
                    request_id: id.clone(),
                    event_json: "{}".to_string(),
                }),
            ),
            (
                FrameVariant::RealtimeServerEvent,
                BridgeMessage::RealtimeServerEvent(crate::protocol::RealtimeServerEventMessage {
                    request_id: id.clone(),
                    event_json: "{}".to_string(),
                }),
            ),
            (
                FrameVariant::RealtimeSessionClose,
                BridgeMessage::RealtimeSessionClose(crate::protocol::RealtimeSessionClose {
                    request_id: id.clone(),
                    code: None,
                    reason: None,
                    response_started: false,
                }),
            ),
            (
                FrameVariant::ResponseStart,
                BridgeMessage::ResponseStart(ResponseStart {
                    request_id: id.clone(),
                    status: 200,
                    content_type: None,
                    headers: vec![],
                }),
            ),
            (
                FrameVariant::ResponseChunk,
                BridgeMessage::ResponseChunk(crate::protocol::ResponseChunk {
                    request_id: id.clone(),
                    data: vec![1],
                }),
            ),
            (
                FrameVariant::ResponseEnd,
                BridgeMessage::ResponseEnd(ResponseEnd {
                    request_id: id.clone(),
                }),
            ),
            (
                FrameVariant::McpResponseStart,
                BridgeMessage::McpResponseStart(crate::protocol::McpResponseStart {
                    request_id: id.clone(),
                    status: 200,
                    content_type: None,
                    headers: vec![],
                }),
            ),
            (
                FrameVariant::McpResponseChunk,
                BridgeMessage::McpResponseChunk(crate::protocol::McpResponseChunk {
                    request_id: id.clone(),
                    data: vec![1],
                }),
            ),
            (
                FrameVariant::McpResponseEnd,
                BridgeMessage::McpResponseEnd(crate::protocol::McpResponseEnd {
                    request_id: id.clone(),
                }),
            ),
            (
                FrameVariant::ResponseError,
                BridgeMessage::ResponseError(crate::protocol::ResponseError {
                    request_id: id.clone(),
                    status: 503,
                    code: "peer_overloaded".to_string(),
                    message: "the ingress of the node is overloaded".to_string(),
                }),
            ),
            (
                FrameVariant::Control,
                BridgeMessage::ConfigSnapshot(snapshot(1)),
            ),
        ]
    }

    #[test]
    fn the_frame_variant_agrees_with_the_dispatch_of_the_node() {
        for (variant, message) in every_frame_variant() {
            assert_eq!(
                frame_variant(&message),
                variant,
                "the variant names the frame of the protocol"
            );
            // The upstream direction accepts a response of the executor and
            // nothing else.
            assert_eq!(
                variant.is_response(),
                response_kind(&message).is_some()
                    || matches!(message, BridgeMessage::ResponseError(_)),
                "the acceptance of the upstream direction is the one of the node: {variant:?}"
            );
            // The downstream direction accepts every frame but a start.
            assert_eq!(
                variant.is_start(),
                matches!(classify(&message), MessageRole::Start(_)),
                "the acceptance of the downstream direction is the one of the node: {variant:?}"
            );
        }
    }

    #[test]
    fn the_action_of_a_refused_frame_follows_its_role() {
        // A new start is failed to the pending request of the relay state.
        assert_eq!(
            refusal_action(MessageRole::Start(RequestKind::Http), true),
            RefusalAction::FailRequest
        );
        assert_eq!(
            refusal_action(MessageRole::Start(RequestKind::Http), false),
            RefusalAction::FailRequest
        );
        // A continuation of a running request is retired in its scope.
        for role in [
            MessageRole::Stream(RequestKind::Http),
            MessageRole::End(RequestKind::Http),
            MessageRole::Cancel(RequestKind::Realtime),
        ] {
            assert_eq!(
                refusal_action(role, true),
                RefusalAction::RetireRequest,
                "a continuation is retired in its scope"
            );
            // An exhausted tracking fails the worker of the route.
            assert_eq!(
                refusal_action(role, false),
                RefusalAction::FailWorker,
                "an exhausted tracking fails the worker of the route"
            );
        }
        // A frame that belongs to no request fails the worker of the route.
        assert_eq!(
            refusal_action(MessageRole::Control, true),
            RefusalAction::FailWorker
        );
        assert_eq!(
            refusal_action(MessageRole::Response(RequestKind::Http), true),
            RefusalAction::FailWorker
        );
    }

    #[tokio::test]
    async fn the_local_client_learns_the_overload_of_the_ingress() {
        let (router, hub, state, stop_tx, node_task, nas_task) =
            node_with_a_database_peer(false).await;
        let handle = hub.ensure_actor().await.expect("the actor of the hub");

        // The actor blocks in the snapshot of the relay state, so the traffic
        // of the relay state piles up in the data channel of the actor.
        let (version, advertisement) = block_the_actor(&hub, &state, &handle, 2).await;

        // The data channel of the actor holds every slot but one, so the
        // start of the request of the local client fits and its continuation
        // does not.
        let queued = fill_data_channel(&handle, id(NAS), super::PEER_INGRESS_BUFFER - 1);
        assert_eq!(
            queued,
            super::PEER_INGRESS_BUFFER - 1,
            "the data channel of the node is nearly full"
        );

        // The local client asks the node.
        let client = tokio::spawn(async move {
            let request = client_request(CLIENT_KEY);
            tokio::time::timeout(
                Duration::from_secs(10),
                tower::ServiceExt::oneshot(router, request),
            )
            .await
            .expect("the local client is answered")
            .expect("the router answers the local client")
        });
        // The retirement of the refused continuation waits on the priority
        // path of the actor, so the test releases the actor first.
        wait_until(|| async { handle.control.capacity() < super::PEER_CONTROL_BUFFER }).await;
        drop(version);
        let _ = advertisement.await;

        // The local client learns the overload of the ingress instead of
        // waiting for an answer that cannot come.
        let response = client.await.expect("the local client is answered");
        assert_eq!(
            response.status(),
            503,
            "the local client learns the overload of the ingress"
        );
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("the body of the answer");
        assert!(
            String::from_utf8_lossy(&body).contains("peer_overloaded"),
            "the local client learns the reason of the overload: {}",
            String::from_utf8_lossy(&body)
        );

        stop_tx.send(()).ok();
        let _ = node_task.await;
        nas_task.abort();
    }

    #[tokio::test]
    async fn an_aborted_local_start_never_creates_an_owner() {
        let (hub, state, mut nas_rx, _mac_rx) = routed_node().await;
        wait_until(|| async { !state.inner.workers.lock().await.is_empty() }).await;
        let handle = hub.ensure_actor().await.expect("the actor of the hub");
        let worker = {
            state
                .inner
                .workers
                .lock()
                .await
                .values()
                .next()
                .cloned()
                .expect("the virtual worker of the route")
        };
        let (version, advertisement) = block_the_actor(&hub, &state, &handle, 2).await;

        // The start of the request waits in the data channel of the actor.
        worker
            .send(http_start("queued"))
            .await
            .expect("the request reaches the worker");
        wait_until(|| async { handle.data.capacity() < super::PEER_INGRESS_BUFFER }).await;

        // The data channel of the actor holds every slot, so the continuation
        // of the request cannot enter the ingress.
        let queued = fill_data_channel(&handle, id(NAS), super::PEER_INGRESS_BUFFER - 1);
        assert_eq!(
            queued,
            super::PEER_INGRESS_BUFFER - 1,
            "the data channel of the node is full"
        );
        worker
            .send(BridgeMessage::RequestChunk(
                crate::protocol::BridgeRequestChunk {
                    request_id: "queued".to_string(),
                    data: vec![7; 8],
                },
            ))
            .await
            .expect("the frame reaches the worker");
        // The retirement waits on the priority path, so it runs as soon as the
        // actor is free again.
        wait_until(|| async { handle.control.capacity() < super::PEER_CONTROL_BUFFER }).await;
        drop(version);
        let _ = advertisement.await;

        // The start never creates an owner: the abort overtook it.
        wait_until(|| async { handle.data.capacity() == super::PEER_INGRESS_BUFFER }).await;
        assert_eq!(hub.owner_count().await, 0, "the aborted start owns nothing");
        // Nothing of the aborted start reached the executor.
        let frames = drain(&mut nas_rx).await;
        assert!(
            frames
                .iter()
                .all(|envelope| !matches!(envelope.message, Some(BridgeMessage::RequestStart(_)))),
            "the aborted start never reached the executor"
        );
    }

    #[tokio::test]
    async fn a_duplicate_local_start_cannot_bypass_the_fence_of_its_flow() {
        let (hub, state, mut nas_rx, _mac_rx) = routed_node().await;
        wait_until(|| async { !state.inner.workers.lock().await.is_empty() }).await;
        let handle = hub.ensure_actor().await.expect("the actor of the hub");
        let worker = {
            state
                .inner
                .workers
                .lock()
                .await
                .values()
                .next()
                .cloned()
                .expect("the virtual worker of the route")
        };
        let (version, advertisement) = block_the_actor(&hub, &state, &handle, 2).await;

        // Two starts of the same flow wait in the data channel of the actor.
        worker
            .send(http_start("duplicate"))
            .await
            .expect("the request reaches the worker");
        worker
            .send(http_start("duplicate"))
            .await
            .expect("the request reaches the worker");
        wait_until(|| async { handle.data.capacity() < super::PEER_INGRESS_BUFFER - 1 }).await;

        // The data channel of the actor holds every slot, so the continuation
        // of the flow cannot enter the ingress.
        let queued = fill_data_channel(&handle, id(NAS), super::PEER_INGRESS_BUFFER - 2);
        assert_eq!(
            queued,
            super::PEER_INGRESS_BUFFER - 2,
            "the data channel of the node is full"
        );
        worker
            .send(BridgeMessage::RequestChunk(
                crate::protocol::BridgeRequestChunk {
                    request_id: "duplicate".to_string(),
                    data: vec![7; 8],
                },
            ))
            .await
            .expect("the frame reaches the worker");
        wait_until(|| async { handle.control.capacity() < super::PEER_CONTROL_BUFFER }).await;
        drop(version);
        let _ = advertisement.await;

        // Neither start of the flow creates an owner.
        wait_until(|| async { handle.data.capacity() == super::PEER_INGRESS_BUFFER }).await;
        assert_eq!(hub.owner_count().await, 0, "the aborted flow owns nothing");
        let frames = drain(&mut nas_rx).await;
        assert!(
            frames
                .iter()
                .all(|envelope| !matches!(envelope.message, Some(BridgeMessage::RequestStart(_)))),
            "the aborted flow never reached the executor"
        );
    }

    #[tokio::test]
    async fn the_further_messages_of_a_failed_flow_are_coalesced() {
        let (hub, state, _nas_rx, _mac_rx) = routed_node().await;
        wait_until(|| async { !state.inner.workers.lock().await.is_empty() }).await;
        let handle = hub.ensure_actor().await.expect("the actor of the hub");
        let worker = {
            state
                .inner
                .workers
                .lock()
                .await
                .values()
                .next()
                .cloned()
                .expect("the virtual worker of the route")
        };
        let (version, advertisement) = block_the_actor(&hub, &state, &handle, 2).await;

        // The data channel of the actor holds every slot, so the traffic of
        // the relay state cannot enter the ingress.
        let queued = fill_data_channel(&handle, id(NAS), super::PEER_INGRESS_BUFFER);
        assert_eq!(
            queued,
            super::PEER_INGRESS_BUFFER,
            "the data channel of the node is full"
        );
        let chunk = BridgeMessage::RequestChunk(crate::protocol::BridgeRequestChunk {
            request_id: "failed".to_string(),
            data: vec![7; 8],
        });

        // The first message of the flow is retired.
        worker
            .send(chunk.clone())
            .await
            .expect("the frame reaches the worker");
        wait_until(|| async { handle.control.capacity() < super::PEER_CONTROL_BUFFER }).await;
        let after_the_first = handle.control.capacity();

        // A further message of the failed flow is discarded: it needs no
        // second retirement.
        worker
            .send(chunk.clone())
            .await
            .expect("the frame reaches the worker");
        wait_until(|| async { worker.capacity() == worker.max_capacity() }).await;
        assert_eq!(
            handle.control.capacity(),
            after_the_first,
            "the failed flow needs no second retirement"
        );

        // The terminal message of the flow retires its tracking.
        worker
            .send(BridgeMessage::RequestCancel(
                crate::protocol::BridgeRequestCancel {
                    request_id: "failed".to_string(),
                    reason: "client_cancel".to_string(),
                    response_started: false,
                },
            ))
            .await
            .expect("the frame reaches the worker");
        wait_until(|| async { worker.capacity() == worker.max_capacity() }).await;
        assert_eq!(
            handle.control.capacity(),
            after_the_first,
            "the terminal message of the failed flow needs no retirement"
        );

        // A message that comes after the terminal of the flow is retired
        // again, because the tracking of the flow is gone.
        worker
            .send(chunk)
            .await
            .expect("the frame reaches the worker");
        wait_until(|| async { handle.control.capacity() < after_the_first }).await;

        drop(version);
        let _ = advertisement.await;
    }

    /// Refuse a frame of a peer while the ingress of the node is full.
    ///
    /// The actor is blocked while the data channel fills up, so the frame of
    /// the peer cannot enter the ingress. The actor is released again, so the
    /// retirement of the request is acknowledged.
    async fn refuse_a_peer_frame_while_full(
        hub: &Arc<RouteHub>,
        state: &AppState,
        handle: &super::ActorHandle,
        token: super::super::admission::AdmissionToken,
        message: BridgeMessage,
        generation: u64,
    ) -> bool {
        let (version, advertisement) = block_the_actor(hub, state, handle, generation).await;
        let queued = fill_data_channel(handle, id(NAS), super::PEER_INGRESS_BUFFER);
        assert_eq!(
            queued,
            super::PEER_INGRESS_BUFFER,
            "the data channel of the node is full"
        );
        let refused = tokio::spawn({
            let hub = hub.clone();
            async move { hub.handle_peer_message(&token, message).await }
        });
        wait_until(|| async { handle.control.capacity() < super::PEER_CONTROL_BUFFER }).await;
        drop(version);
        let _ = advertisement.await;
        refused.await.expect("the refusal of the frame completes")
    }

    #[tokio::test]
    async fn a_closed_ingress_is_never_a_pressure_of_the_node() {
        let (hub, state, _nas_rx, _mac_rx) = routed_node().await;
        wait_until(|| async { !state.inner.workers.lock().await.is_empty() }).await;
        let handle = hub.ensure_actor().await.expect("the actor of the hub");
        let nas = super::super::admission::mint_for_test(id(NAS), 1);
        let worker = {
            state
                .inner
                .workers
                .lock()
                .await
                .values()
                .next()
                .cloned()
                .expect("the virtual worker of the route")
        };

        // The byte budget of the ingress is exhausted, so a closed ingress
        // must never be reported as a byte pressure.
        let _budget = handle
            .data_budget
            .clone()
            .acquire_many_owned(super::PEER_INGRESS_BUDGET_BYTES as u32)
            .await
            .expect("the budget of the ingress");

        // The actor ends, so the ingress of the node is closed.
        let task = handle
            .task
            .lock()
            .await
            .take()
            .expect("the task of the actor");
        task.abort();
        let _ = task.await;
        wait_until(|| async { handle.data.is_closed() }).await;

        // A start of a peer names the closed ingress.
        let path = PeerPath::new(vec![id(NAS)]).expect("a bounded path");
        let error = hub
            .forward_start(&nas, path, http_start("closed"))
            .await
            .expect_err("the start is refused");
        assert!(
            error.to_string().contains("closed"),
            "the start names the closed ingress: {error}"
        );

        // A frame of a peer is refused and the link of the peer ends.
        assert!(
            !hub.handle_peer_message(
                &nas,
                BridgeMessage::RequestChunk(crate::protocol::BridgeRequestChunk {
                    request_id: "closed".to_string(),
                    data: vec![7; 8],
                }),
            )
            .await,
            "the link of the peer ends with the closed ingress"
        );

        // The worker of the route cannot hand its traffic over any more, so
        // the pump of the route fails as it did before.
        worker
            .send(BridgeMessage::RequestChunk(
                crate::protocol::BridgeRequestChunk {
                    request_id: "closed".to_string(),
                    data: vec![7; 8],
                },
            ))
            .await
            .expect("the frame reaches the worker");
        wait_until(|| async { worker.is_closed() }).await;

        // The refusals never took a permit of the byte budget.
        assert_eq!(
            handle.data_budget.available_permits(),
            0,
            "the byte budget of the ingress is untouched"
        );
    }

    #[tokio::test]
    async fn a_refused_peer_frame_retires_only_the_request_of_the_peer() {
        let (hub, state, mut nas_rx, mut mac_rx) = routed_node().await;
        let mac = super::super::admission::mint_for_test(id(MAC), 2);
        // Two requests of the peer travel on the route of the node.
        hub.forward_start(&mac, forwarded_path(), http_start("affected"))
            .await
            .expect("the start of the peer is forwarded");
        hub.forward_start(&mac, forwarded_path(), http_start("unrelated"))
            .await
            .expect("the start of the peer is forwarded");
        wait_until(|| async { hub.owner_count().await == 2 }).await;
        let handle = hub.ensure_actor().await.expect("the actor of the hub");
        let (version, advertisement) = block_the_actor(&hub, &state, &handle, 2).await;

        // The data channel of the actor holds every slot, so the frame of the
        // peer cannot enter the ingress.
        let queued = fill_data_channel(&handle, id(NAS), super::PEER_INGRESS_BUFFER);
        assert_eq!(
            queued,
            super::PEER_INGRESS_BUFFER,
            "the data channel of the node is full"
        );

        // The refusal waits for the acknowledgement of the actor, so the test
        // releases the actor first.
        let refused = tokio::spawn({
            let hub = hub.clone();
            let mac = mac.clone();
            async move {
                hub.handle_peer_message(
                    &mac,
                    BridgeMessage::RequestChunk(crate::protocol::BridgeRequestChunk {
                        request_id: "affected".to_string(),
                        data: vec![7; 8],
                    }),
                )
                .await
            }
        });
        wait_until(|| async { handle.control.capacity() < super::PEER_CONTROL_BUFFER }).await;
        drop(version);
        let _ = advertisement.await;
        assert!(
            refused.await.expect("the refusal of the frame completes"),
            "the link of the peer survives the refusal of a frame"
        );

        // The request of the peer is retired with the overload error, and only
        // that request.
        let error = next_bridge(&mut mac_rx).await;
        let BridgeMessage::ResponseError(error) = error else {
            panic!("the peer learns the failure of its request");
        };
        assert_eq!(error.request_id, "affected");
        assert_eq!(error.status, 503);
        assert_eq!(error.code, "peer_overloaded");
        assert_eq!(
            hub.owner_count().await,
            1,
            "only the request of the refused frame is retired"
        );

        // The upstream work of the retired request is cancelled once.
        let frames = drain(&mut nas_rx).await;
        let cancels = frames
            .iter()
            .filter(|envelope| matches!(envelope.message, Some(BridgeMessage::RequestCancel(_))))
            .count();
        assert_eq!(
            cancels, 1,
            "the upstream work of the retired request is cancelled once"
        );

        // The worker of the route, the route, its snapshot and the
        // generation of the link of the peer survive the refusal.
        assert!(
            !state.inner.workers.lock().await.is_empty(),
            "the worker of the route survives the refusal"
        );
        assert_eq!(
            hub.selected_path().await,
            Some(vec![id(LOCAL), id(NAS)]),
            "the route of the node survives the refusal"
        );
        assert_eq!(
            *state.inner.config_version.lock().await,
            Some(2),
            "the snapshot of the route survives the refusal"
        );
        assert!(
            mac.is_live(),
            "the admission token of the peer survives the refusal"
        );

        // The unrelated request of the peer still finishes.
        wait_until(|| async { handle.data.capacity() == super::PEER_INGRESS_BUFFER }).await;
        assert!(
            hub.handle_peer_message(
                &mac,
                BridgeMessage::RequestCancel(crate::protocol::BridgeRequestCancel {
                    request_id: "unrelated".to_string(),
                    reason: "client_cancel".to_string(),
                    response_started: false,
                }),
            )
            .await,
            "the frame of the unrelated request enters the ingress"
        );
        wait_until(|| async { hub.owner_count().await == 0 }).await;

        // A request that comes after the capacity drained works again. The
        // start needs the registered link of the peer under the generation of
        // its token, so the successful start proves that the generation of
        // the link of the peer survived the refusal.
        hub.forward_start(&mac, forwarded_path(), http_start("later"))
            .await
            .expect("the start of the peer is forwarded");
        wait_until(|| async { hub.owner_count().await == 1 }).await;
    }

    /// The upstream id of the request the node forwarded.
    ///
    /// The node rewrites the id of a forwarded request, so the executor of
    /// the route knows the request by an id of its own.
    async fn upstream_id_of(receiver: &mut super::EnvelopeReceiver) -> String {
        let message = next_bridge(receiver).await;
        super::request_id_of(&message)
            .expect("the forwarded start carries a request id")
            .to_string()
    }

    #[tokio::test]
    async fn a_refused_frame_of_the_wrong_direction_retires_nothing() {
        let (hub, state, mut nas_rx, mut mac_rx) = routed_node().await;
        let mac = super::super::admission::mint_for_test(id(MAC), 2);
        let nas = super::super::admission::mint_for_test(id(NAS), 1);
        hub.forward_start(&mac, forwarded_path(), http_start("peer-1"))
            .await
            .expect("the start of the peer is forwarded");
        wait_until(|| async { hub.owner_count().await == 1 }).await;
        let handle = hub.ensure_actor().await.expect("the actor of the hub");

        // The node rewrites the id of a forwarded request, so the executor
        // knows the request by the id the node gave it.
        let upstream_id = upstream_id_of(&mut nas_rx).await;
        assert_ne!(
            upstream_id, "peer-1",
            "the upstream id of the request differs from the id of the peer"
        );

        // The executor sends a frame that is not a response. The normal
        // dispatch drops it, so a full ingress must not retire the request.
        assert!(
            refuse_a_peer_frame_while_full(
                &hub,
                &state,
                &handle,
                nas,
                BridgeMessage::RequestChunk(crate::protocol::BridgeRequestChunk {
                    request_id: upstream_id.clone(),
                    data: vec![7; 8],
                }),
                2,
            )
            .await,
            "the link of the executor survives the refusal of a frame"
        );
        // The refusal waits for the answer of the actor, so the retirement
        // was processed before the state of the node is checked.
        assert_eq!(
            hub.owner_count().await,
            1,
            "the request of the executor keeps its owner"
        );
        assert!(
            drain(&mut mac_rx)
                .await
                .iter()
                .all(|envelope| !matches!(envelope.message, Some(BridgeMessage::ResponseError(_)))),
            "the peer learns nothing"
        );

        // The peer sends a start outside a request start envelope. The normal
        // dispatch drops it, so a full ingress must not retire the request.
        assert!(
            refuse_a_peer_frame_while_full(&hub, &state, &handle, mac, http_start("peer-1"), 3,)
                .await,
            "the link of the peer survives the refusal of a frame"
        );
        assert_eq!(
            hub.owner_count().await,
            1,
            "the request of the peer keeps its owner"
        );

        // The upstream work of the request was never cancelled.
        let frames = drain(&mut nas_rx).await;
        assert_eq!(
            frames
                .iter()
                .filter(|envelope| matches!(
                    envelope.message,
                    Some(BridgeMessage::RequestCancel(_))
                ))
                .count(),
            0,
            "the upstream work of the request is never cancelled"
        );
    }

    #[tokio::test]
    async fn a_refused_local_continuation_of_another_kind_retires_nothing() {
        let (hub, state, mut nas_rx, _mac_rx) = routed_node().await;
        wait_until(|| async { !state.inner.workers.lock().await.is_empty() }).await;
        let handle = hub.ensure_actor().await.expect("the actor of the hub");
        let worker = {
            state
                .inner
                .workers
                .lock()
                .await
                .values()
                .next()
                .cloned()
                .expect("the virtual worker of the route")
        };

        // A local request of the route runs under its own id.
        worker
            .send(http_start("collision"))
            .await
            .expect("the request reaches the worker");
        wait_until(|| async { hub.owner_count().await == 1 }).await;

        // The actor blocks in the snapshot of the relay state, so the
        // traffic of the data path piles up.
        let (version, advertisement) = block_the_actor(&hub, &state, &handle, 2).await;
        let queued = fill_data_channel(&handle, id(NAS), super::PEER_INGRESS_BUFFER);
        assert_eq!(
            queued,
            super::PEER_INGRESS_BUFFER,
            "the data channel of the node is full"
        );

        // A continuation of another kind cannot enter the ingress, so the
        // pump retires it in its own scope.
        worker
            .send(BridgeMessage::McpRequestChunk(
                crate::protocol::McpRequestChunk {
                    request_id: "collision".to_string(),
                    data: vec![7; 8],
                },
            ))
            .await
            .expect("the frame reaches the worker");
        // The retirement waits on the priority path, so the actor takes it
        // as soon as it is free again.
        wait_until(|| async { handle.control.capacity() < super::PEER_CONTROL_BUFFER }).await;
        drop(version);
        let _ = advertisement.await;

        // The actor answers the commands of the priority path in order, so
        // the answer of the owner count comes after the retirement of the
        // request was processed.
        assert_eq!(
            hub.owner_count().await,
            1,
            "the request of the local client keeps its owner"
        );

        // The upstream work of the request was never cancelled.
        let frames = drain(&mut nas_rx).await;
        assert_eq!(
            frames
                .iter()
                .filter(|envelope| matches!(
                    envelope.message,
                    Some(BridgeMessage::RequestCancel(_))
                ))
                .count(),
            0,
            "the upstream work of the request is never cancelled"
        );
    }

    /// A command of the ingress with the byte permit of its frame.
    fn local_command(handle: &super::ActorHandle, id: &str) -> super::Command {
        let message = http_start(id);
        let permit = handle
            .data_budget
            .clone()
            .try_acquire_many_owned(super::ingress_bytes(&message) as u32)
            .expect("the byte budget of the ingress");
        super::Command::ForwardLocal {
            worker_id: 0,
            epoch: 0,
            message,
            fence: None,
            permit,
        }
    }

    /// A command of the ingress with a byte permit of another budget.
    ///
    /// The filler of a controlled channel holds a permit of its own, so the
    /// byte budget of the ingress only carries the command of the test.
    async fn filler_command(message: BridgeMessage) -> super::Command {
        let budget = Arc::new(tokio::sync::Semaphore::new(1));
        let permit = budget
            .acquire_owned()
            .await
            .expect("the byte budget of the filler");
        super::Command::ForwardLocal {
            worker_id: 0,
            epoch: 0,
            message,
            fence: None,
            permit,
        }
    }

    /// The request id of a command of the ingress.
    fn command_request_id(command: &super::Command) -> String {
        let message = match command {
            super::Command::ForwardLocal { message, .. }
            | super::Command::ForwardStart { message, .. }
            | super::Command::HandlePeerMessage { message, .. } => message,
            _ => return String::new(),
        };
        super::request_id_of(message)
            .unwrap_or_default()
            .to_string()
    }

    /// The frame of a command of the ingress.
    fn command_frame(command: &super::Command) -> super::FrameVariant {
        let message = match command {
            super::Command::ForwardLocal { message, .. }
            | super::Command::ForwardStart { message, .. }
            | super::Command::HandlePeerMessage { message, .. } => message,
            _ => return super::FrameVariant::Control,
        };
        super::frame_variant(message)
    }

    #[tokio::test]
    async fn a_full_ingress_recovers_the_command_it_refused() {
        let (hub, _state, _nas_rx, _mac_rx) = routed_node().await;
        let handle = hub.ensure_actor().await.expect("the actor of the hub");
        let (_failure, failure_rx) = tokio::sync::watch::channel(());

        // A real bounded channel stands in for the ingress of the node, so
        // the recovery runs against a real sender.
        let (tx, mut rx) = tokio::sync::mpsc::channel::<super::Command>(1);
        let filler = filler_command(http_start("filler")).await;
        tx.try_send(filler)
            .expect("the channel accepts its first command");

        let mut attempts = 0usize;
        let mut ids = Vec::new();
        let mut frames = Vec::new();
        let mut budgets = Vec::new();
        let available = handle.data_budget.available_permits();
        let command = local_command(&handle, "recovered");
        let size = super::ingress_bytes(&http_start("recovered")) as isize;
        let outcome = super::admit_into_ingress(
            command,
            |command| {
                attempts += 1;
                ids.push(command_request_id(&command));
                frames.push(command_frame(&command));
                budgets.push(handle.data_budget.available_permits() as isize);
                // The slot of the channel is released once, so a later
                // attempt succeeds.
                if attempts > 1 {
                    let _ = rx.try_recv();
                }
                match tx.try_send(command) {
                    Ok(()) => super::Attempt::Queued,
                    Err(tokio::sync::mpsc::error::TrySendError::Full(unsent)) => {
                        super::Attempt::Full(unsent)
                    }
                    Err(tokio::sync::mpsc::error::TrySendError::Closed(unsent)) => {
                        super::Attempt::Closed(unsent)
                    }
                }
            },
            &super::AdmissionGuard::Local(&failure_rx),
        )
        .await;

        assert!(
            matches!(outcome, super::Admission::Queued),
            "the command entered the ingress after the recovery"
        );
        assert_eq!(attempts, 2, "the recovery retries the refused command once");
        assert_eq!(
            ids,
            vec!["recovered".to_string(), "recovered".to_string()],
            "the recovery keeps the original command"
        );
        assert_eq!(
            frames[0], frames[1],
            "the recovery keeps the original frame"
        );
        assert_eq!(
            budgets,
            vec![available as isize - size, available as isize - size],
            "the recovery reserves the byte budget once"
        );
        assert_eq!(
            handle.data_budget.available_permits() as isize,
            available as isize - size,
            "the admitted command holds its byte budget"
        );

        // The command that entered the ingress is the original one.
        let queued = rx.try_recv().expect("the command waits in the channel");
        assert_eq!(command_request_id(&queued), "recovered");
    }

    #[tokio::test]
    async fn an_admitted_command_never_waits_for_the_ingress() {
        let (hub, _state, _nas_rx, _mac_rx) = routed_node().await;
        let handle = hub.ensure_actor().await.expect("the actor of the hub");
        let (_failure, failure_rx) = tokio::sync::watch::channel(());
        let (tx, mut rx) = tokio::sync::mpsc::channel::<super::Command>(1);

        let mut attempts = 0usize;
        let command = local_command(&handle, "immediate");
        let outcome = super::admit_into_ingress(
            command,
            |command| {
                attempts += 1;
                match tx.try_send(command) {
                    Ok(()) => super::Attempt::Queued,
                    Err(tokio::sync::mpsc::error::TrySendError::Full(unsent)) => {
                        super::Attempt::Full(unsent)
                    }
                    Err(tokio::sync::mpsc::error::TrySendError::Closed(unsent)) => {
                        super::Attempt::Closed(unsent)
                    }
                }
            },
            &super::AdmissionGuard::Local(&failure_rx),
        )
        .await;

        assert!(
            matches!(outcome, super::Admission::Queued),
            "the command entered the ingress at once"
        );
        assert_eq!(attempts, 1, "an admitted command is never retried");
        assert_eq!(
            command_request_id(&rx.try_recv().expect("the command waits in the channel")),
            "immediate"
        );
    }

    #[tokio::test]
    async fn a_persistent_full_ingress_refuses_the_command_after_five_attempts() {
        let (hub, _state, _nas_rx, _mac_rx) = routed_node().await;
        let handle = hub.ensure_actor().await.expect("the actor of the hub");
        let (_failure, failure_rx) = tokio::sync::watch::channel(());

        let mut attempts = 0usize;
        let command = local_command(&handle, "persistent");
        let outcome = super::admit_into_ingress(
            command,
            |command| {
                attempts += 1;
                super::Attempt::Full(command)
            },
            &super::AdmissionGuard::Local(&failure_rx),
        )
        .await;

        assert!(
            matches!(outcome, super::Admission::Full(_)),
            "the ingress refuses the command after the recovery"
        );
        assert_eq!(
            attempts, 5,
            "the recovery tries the command once and retries it four times"
        );
    }

    #[tokio::test]
    async fn a_closed_ingress_stops_the_recovery() {
        let (hub, _state, _nas_rx, _mac_rx) = routed_node().await;
        let handle = hub.ensure_actor().await.expect("the actor of the hub");
        let (_failure, failure_rx) = tokio::sync::watch::channel(());
        let available = handle.data_budget.available_permits();

        // A closed ingress stops the recovery at once.
        let mut attempts = 0usize;
        let command = local_command(&handle, "closed");
        let outcome = super::admit_into_ingress(
            command,
            |command| {
                attempts += 1;
                super::Attempt::Closed(command)
            },
            &super::AdmissionGuard::Local(&failure_rx),
        )
        .await;
        assert!(
            matches!(outcome, super::Admission::Closed(_)),
            "a closed ingress stops the recovery"
        );
        assert_eq!(attempts, 1, "a closed ingress is never retried");
        drop(outcome);
        assert_eq!(
            handle.data_budget.available_permits(),
            available,
            "the unsent command releases its byte budget"
        );

        // A closed ingress after a full one stops the recovery as well.
        let mut attempts = 0usize;
        let command = local_command(&handle, "closed-after-full");
        let outcome = super::admit_into_ingress(
            command,
            |command| {
                attempts += 1;
                if attempts == 1 {
                    super::Attempt::Full(command)
                } else {
                    super::Attempt::Closed(command)
                }
            },
            &super::AdmissionGuard::Local(&failure_rx),
        )
        .await;
        assert!(
            matches!(outcome, super::Admission::Closed(_)),
            "a closed ingress stops the recovery"
        );
        assert_eq!(attempts, 2, "the recovery stops at the closed ingress");
        drop(outcome);
        assert_eq!(
            handle.data_budget.available_permits(),
            available,
            "the unsent command releases its byte budget"
        );
    }

    #[tokio::test]
    async fn a_ready_failure_stops_the_admission_before_the_first_attempt() {
        let (hub, _state, _nas_rx, _mac_rx) = routed_node().await;
        let handle = hub.ensure_actor().await.expect("the actor of the hub");
        let (failure, failure_rx) = tokio::sync::watch::channel(());
        let available = handle.data_budget.available_permits();

        // The failure of the worker is already pending, so the command of the
        // worker never enters the ingress.
        let _ = failure.send(());

        let mut attempts = 0usize;
        let command = local_command(&handle, "ready-failure");
        let outcome = super::admit_into_ingress(
            command,
            |_command| {
                attempts += 1;
                super::Attempt::Queued
            },
            &super::AdmissionGuard::Local(&failure_rx),
        )
        .await;

        assert_eq!(
            attempts, 0,
            "a pending failure stops the admission before its first attempt"
        );
        assert!(
            matches!(outcome, super::Admission::Stopped(_, super::Stop::Failed)),
            "a pending failure stops the admission"
        );
        drop(outcome);
        assert_eq!(
            handle.data_budget.available_permits(),
            available,
            "the rejected command releases its byte budget"
        );
    }

    #[tokio::test]
    async fn a_closed_failure_watch_stops_the_recovery_without_a_failure() {
        let (hub, _state, _nas_rx, _mac_rx) = routed_node().await;
        let handle = hub.ensure_actor().await.expect("the actor of the hub");
        let (failure, failure_rx) = tokio::sync::watch::channel(());
        let mut failure = Some(failure);

        let mut attempts = 0usize;
        let command = local_command(&handle, "closed-watch");
        let outcome = super::admit_into_ingress(
            command,
            |command| {
                attempts += 1;
                if attempts == 1 {
                    // The failure watch of the worker ends while the command
                    // waits for the ingress.
                    failure.take();
                    super::Attempt::Full(command)
                } else {
                    super::Attempt::Queued
                }
            },
            &super::AdmissionGuard::Local(&failure_rx),
        )
        .await;

        assert_eq!(
            attempts, 1,
            "the end of the failure watch stops the retry of the command"
        );
        assert!(
            matches!(outcome, super::Admission::Stopped(_, super::Stop::Ended)),
            "the end of the failure watch is not a failure of the worker"
        );
    }

    #[tokio::test]
    async fn an_unread_failure_wins_over_the_closure_of_its_watch() {
        let (hub, _state, _nas_rx, _mac_rx) = routed_node().await;
        let handle = hub.ensure_actor().await.expect("the actor of the hub");
        let (failure, failure_rx) = tokio::sync::watch::channel(());
        let mut failure = Some(failure);

        // The failure of the worker is notified and its watch ends right
        // after it.
        let _ = failure.as_ref().expect("the failure watch").send(());
        failure.take();

        let mut attempts = 0usize;
        let command = local_command(&handle, "unread-failure");
        let outcome = super::admit_into_ingress(
            command,
            |_command| {
                attempts += 1;
                super::Attempt::Queued
            },
            &super::AdmissionGuard::Local(&failure_rx),
        )
        .await;

        assert_eq!(
            attempts, 0,
            "an unread failure stops the admission before its first attempt"
        );
        assert!(
            matches!(outcome, super::Admission::Stopped(_, super::Stop::Failed)),
            "an unread failure is never lost by the closure of its watch"
        );
    }

    #[tokio::test]
    async fn a_stale_token_stops_the_recovery_of_a_peer() {
        let (hub, _state, _nas_rx, _mac_rx) = routed_node().await;
        let handle = hub.ensure_actor().await.expect("the actor of the hub");
        let token = super::super::admission::mint_for_test(id(MAC), 2);

        let mut attempts = 0usize;
        let command = local_command(&handle, "stale");
        let outcome = super::admit_into_ingress(
            command,
            |command| {
                attempts += 1;
                // The link of the peer ends while the command waits.
                token.mark_dead();
                super::Attempt::Full(command)
            },
            &super::AdmissionGuard::Peer(&token),
        )
        .await;

        assert!(
            matches!(outcome, super::Admission::Stopped(_, _)),
            "a stale token stops the recovery"
        );
        assert_eq!(attempts, 1, "a stale token is never retried");
    }

    #[tokio::test]
    async fn a_failure_of_the_worker_stops_the_recovery() {
        let (hub, _state, _nas_rx, _mac_rx) = routed_node().await;
        let handle = hub.ensure_actor().await.expect("the actor of the hub");
        let (failure, failure_rx) = tokio::sync::watch::channel(());
        let available = handle.data_budget.available_permits();

        let mut attempts = 0usize;
        let command = local_command(&handle, "failed");
        let outcome = super::admit_into_ingress(
            command,
            |command| {
                attempts += 1;
                // The worker of the route fails while the command waits.
                let _ = failure.send(());
                super::Attempt::Full(command)
            },
            &super::AdmissionGuard::Local(&failure_rx),
        )
        .await;

        assert!(
            matches!(outcome, super::Admission::Stopped(_, super::Stop::Failed)),
            "a failure of the worker stops the recovery"
        );
        assert_eq!(attempts, 1, "a failed worker is never retried");
        drop(outcome);
        assert_eq!(
            handle.data_budget.available_permits(),
            available,
            "the unsent command releases its byte budget"
        );
    }

    #[tokio::test]
    async fn a_failure_that_arrives_during_the_recovery_stops_the_next_attempt() {
        let (hub, _state, _nas_rx, _mac_rx) = routed_node().await;
        let handle = hub.ensure_actor().await.expect("the actor of the hub");
        let (failure, failure_rx) = tokio::sync::watch::channel(());
        let available = handle.data_budget.available_permits();

        let mut attempts = 0usize;
        let command = local_command(&handle, "late-failure");
        let outcome = super::admit_into_ingress(
            command,
            |command| {
                attempts += 1;
                // The worker of the route fails while the command waits for
                // the ingress, so the failure is pending between the yield of
                // the recovery and its next attempt.
                let _ = failure.send(());
                super::Attempt::Full(command)
            },
            &super::AdmissionGuard::Local(&failure_rx),
        )
        .await;

        assert_eq!(
            attempts, 1,
            "a failure pending after the recovery yield stops the next attempt"
        );
        assert!(
            matches!(outcome, super::Admission::Stopped(_, super::Stop::Failed)),
            "a failure that arrives during the recovery stops it"
        );
        drop(outcome);
        assert_eq!(
            handle.data_budget.available_permits(),
            available,
            "the unsent command releases its byte budget"
        );
    }

    #[tokio::test]
    async fn a_burst_of_local_commands_waits_in_the_ingress_of_the_node() {
        let (hub, state, mut nas_rx, _mac_rx) = routed_node().await;
        wait_until(|| async { !state.inner.workers.lock().await.is_empty() }).await;
        let handle = hub.ensure_actor().await.expect("the actor of the hub");
        let worker = {
            state
                .inner
                .workers
                .lock()
                .await
                .values()
                .next()
                .cloned()
                .expect("the virtual worker of the route")
        };

        // The actor blocks in the snapshot of the relay state, so the traffic
        // of the data path piles up.
        let (version, advertisement) = block_the_actor(&hub, &state, &handle, 2).await;

        // A burst of small local commands waits in the ingress of the node.
        // The burst is sent beside the test, so a full ingress never blocks
        // the test itself.
        let burst = tokio::spawn({
            let worker = worker.clone();
            async move {
                for index in 0..160 {
                    worker
                        .send(http_start(&format!("burst-{index}")))
                        .await
                        .expect("the request reaches the worker");
                }
            }
        });
        wait_until(|| async {
            handle.data.capacity() <= super::PEER_INGRESS_BUFFER.saturating_sub(160)
        })
        .await;
        assert_eq!(
            super::PEER_INGRESS_BUFFER - handle.data.capacity(),
            160,
            "the ingress of the node holds the whole burst"
        );
        burst.await.expect("the burst reaches the worker");

        // The byte budget of the ingress is not exhausted by the burst.
        assert!(
            handle.data_budget.available_permits() > 0,
            "the byte budget of the ingress is not exhausted"
        );
        assert!(
            !state.inner.workers.lock().await.is_empty(),
            "the worker of the route survives the burst"
        );

        drop(version);
        let _ = advertisement.await;

        // The route of the node survives the burst.
        assert_eq!(
            hub.selected_path().await,
            Some(vec![id(LOCAL), id(NAS)]),
            "the route of the node survives the burst"
        );

        // The egress of the node delivers every command of the burst in the
        // order it waited in the ingress.
        wait_until(|| async { hub.owner_count().await == 160 }).await;
        let frames = drain(&mut nas_rx).await;
        let delivered: Vec<String> = frames
            .iter()
            .filter_map(|envelope| match &envelope.message {
                Some(BridgeMessage::RequestStart(start)) => Some(start.request_id.clone()),
                _ => None,
            })
            .collect();
        let expected: Vec<String> = (0..160).map(|index| format!("burst-{index}")).collect();
        assert_eq!(
            delivered, expected,
            "the egress of the node delivers the burst in order"
        );
    }

    #[tokio::test]
    async fn a_refused_peer_start_never_reaches_the_route() {
        let (hub, state, mut nas_rx, _mac_rx) = routed_node().await;
        let mac = super::super::admission::mint_for_test(id(MAC), 2);
        let handle = hub.ensure_actor().await.expect("the actor of the hub");

        // The actor blocks in the snapshot of the relay state and the data
        // channel holds every slot, so the start of the peer cannot enter
        // the ingress.
        let (version, advertisement) = block_the_actor(&hub, &state, &handle, 2).await;
        let queued = fill_data_channel(&handle, id(NAS), super::PEER_INGRESS_BUFFER);
        assert_eq!(
            queued,
            super::PEER_INGRESS_BUFFER,
            "the data channel of the node is full"
        );

        let error = hub
            .forward_start(&mac, forwarded_path(), http_start("peer-start"))
            .await
            .expect_err("the start of the peer is refused");
        assert!(
            error.to_string().contains("full"),
            "the start names the full ingress: {error}"
        );

        drop(version);
        let _ = advertisement.await;

        // The refused start of the peer creates no owner, and the route of
        // the node and the egress of the node survive the refusal.
        assert_eq!(
            hub.owner_count().await,
            0,
            "the refused start of the peer creates no owner"
        );
        assert_eq!(
            hub.selected_path().await,
            Some(vec![id(LOCAL), id(NAS)]),
            "the route of the node survives the refusal"
        );
        assert!(
            drain(&mut nas_rx)
                .await
                .iter()
                .all(|envelope| !matches!(envelope.message, Some(BridgeMessage::RequestStart(_)))),
            "the refused start never reaches the executor"
        );
    }

    #[tokio::test]
    async fn a_refused_peer_response_retires_the_request_of_the_executor() {
        let (hub, state, mut nas_rx, mut mac_rx) = routed_node().await;
        let mac = super::super::admission::mint_for_test(id(MAC), 2);
        let nas = super::super::admission::mint_for_test(id(NAS), 1);
        hub.forward_start(&mac, forwarded_path(), http_start("peer-response"))
            .await
            .expect("the start of the peer is forwarded");
        wait_until(|| async { hub.owner_count().await == 1 }).await;
        let upstream_id = upstream_id_of(&mut nas_rx).await;
        let handle = hub.ensure_actor().await.expect("the actor of the hub");

        // The response of the executor cannot enter the full ingress of the
        // node, so the request of the peer is retired in its own scope.
        assert!(
            refuse_a_peer_frame_while_full(
                &hub,
                &state,
                &handle,
                nas,
                BridgeMessage::ResponseChunk(crate::protocol::ResponseChunk {
                    request_id: upstream_id.clone(),
                    data: vec![7; 8],
                }),
                2,
            )
            .await,
            "the link of the executor survives the refusal of a frame"
        );

        // The peer learns the overload of the ingress of the node.
        let error = next_bridge(&mut mac_rx).await;
        let BridgeMessage::ResponseError(error) = error else {
            panic!("the peer learns the failure of its request");
        };
        assert_eq!(error.request_id, "peer-response");
        assert_eq!(error.status, 503);
        assert_eq!(error.code, "peer_overloaded");
        assert_eq!(
            hub.owner_count().await,
            0,
            "the request of the peer is retired"
        );

        // The upstream work of the retired request is cancelled once.
        let frames = drain(&mut nas_rx).await;
        assert_eq!(
            frames
                .iter()
                .filter(|envelope| matches!(
                    envelope.message,
                    Some(BridgeMessage::RequestCancel(_))
                ))
                .count(),
            1,
            "the upstream work of the retired request is cancelled once"
        );
    }

    #[tokio::test]
    async fn an_admitted_frame_keeps_its_byte_budget_until_the_actor_retires_it() {
        let (hub, state, _nas_rx, _mac_rx) = routed_node().await;
        let nas = super::super::admission::mint_for_test(id(NAS), 1);
        let handle = hub.ensure_actor().await.expect("the actor of the hub");

        // The actor blocks in the snapshot of the relay state, so the frame
        // waits in the ingress of the node.
        let (version, advertisement) = block_the_actor(&hub, &state, &handle, 2).await;
        let available = handle.data_budget.available_permits();
        let message = BridgeMessage::ResponseEnd(crate::protocol::ResponseEnd {
            request_id: "held".to_string(),
        });
        let size = super::ingress_bytes(&message);
        let admitted = tokio::spawn({
            let hub = hub.clone();
            let nas = nas.clone();
            async move { hub.handle_peer_message(&nas, message).await }
        });
        wait_until(|| async { handle.data.capacity() == super::PEER_INGRESS_BUFFER - 1 }).await;
        assert_eq!(
            handle.data_budget.available_permits(),
            available - size,
            "the admitted frame holds its byte budget while it waits"
        );

        // The sender of the queued frame is cancelled while the actor is
        // gated, so the command waits in the ingress without a waiting
        // sender. Its byte budget stays reserved until the actor retires the
        // command.
        admitted.abort();
        assert!(
            admitted
                .await
                .expect_err("the sender is cancelled")
                .is_cancelled(),
            "the sender of the queued frame is cancelled"
        );
        assert_eq!(
            handle.data_budget.available_permits(),
            available - size,
            "the queued command keeps its byte budget after its sender was cancelled"
        );

        drop(version);
        let _ = advertisement.await;
        wait_until(|| async { handle.data_budget.available_permits() == available }).await;
    }

    #[tokio::test]
    async fn an_unsent_frame_releases_its_byte_budget_at_the_refusal() {
        let (hub, state, _nas_rx, _mac_rx) = routed_node().await;
        let nas = super::super::admission::mint_for_test(id(NAS), 1);
        let handle = hub.ensure_actor().await.expect("the actor of the hub");

        // The actor blocks in the snapshot of the relay state, so both paths
        // of the node are full and the frame cannot be retired.
        let (version, advertisement) = block_the_actor(&hub, &state, &handle, 2).await;
        assert_eq!(
            fill_control_channel(&handle),
            super::PEER_CONTROL_BUFFER,
            "the control path of the node is full"
        );
        let queued = fill_data_channel(&handle, id(NAS), super::PEER_INGRESS_BUFFER);
        assert_eq!(
            queued,
            super::PEER_INGRESS_BUFFER,
            "the data channel of the node is full"
        );
        let available = handle.data_budget.available_permits();
        let message = BridgeMessage::ResponseEnd(crate::protocol::ResponseEnd {
            request_id: "abandoned".to_string(),
        });

        // The frame cannot enter the ingress, so it is refused and its byte
        // budget is released without waiting for the actor.
        assert!(
            !hub.handle_peer_message(&nas, message).await,
            "the frame of the full ingress is refused"
        );
        assert_eq!(
            handle.data_budget.available_permits(),
            available,
            "the unsent frame releases its byte budget at the refusal"
        );

        drop(version);
        let _ = advertisement.await;
    }

    #[tokio::test]
    async fn a_ready_failure_of_the_worker_stops_the_pump_before_the_next_frame() {
        let (hub, state, mut nas_rx, _mac_rx) = routed_node().await;
        wait_until(|| async { !state.inner.workers.lock().await.is_empty() }).await;
        let handle = hub.ensure_actor().await.expect("the actor of the hub");
        let worker_id = {
            *state
                .inner
                .workers
                .lock()
                .await
                .keys()
                .next()
                .expect("the virtual worker of the route")
        };
        let worker = {
            state
                .inner
                .workers
                .lock()
                .await
                .values()
                .next()
                .cloned()
                .expect("the virtual worker of the route")
        };

        // The actor blocks in the snapshot of the relay state and the data
        // channel holds every slot, so the frame of the relay state cannot
        // enter the ingress.
        let (version, advertisement) = block_the_actor(&hub, &state, &handle, 2).await;
        let queued = fill_data_channel(&handle, id(NAS), super::PEER_INGRESS_BUFFER);
        assert_eq!(
            queued,
            super::PEER_INGRESS_BUFFER,
            "the data channel of the node is full"
        );

        // The frame of the relay state waits in the inbox of the worker.
        worker
            .send(http_start("after-failure"))
            .await
            .expect("the frame reaches the worker");

        // The worker of the route fails while the frame waits.
        state.inner.signal_worker_failure(worker_id);

        // The worker publishes its failure on the priority path, so the
        // command of the failure is queued.
        wait_until(|| async { handle.control.capacity() < super::PEER_CONTROL_BUFFER }).await;

        drop(version);
        let _ = advertisement.await;

        // The failure of the worker wins over the retirement of the frame:
        // the upstream work of the frame is never cancelled.
        let frames = drain(&mut nas_rx).await;
        assert_eq!(
            frames
                .iter()
                .filter(|envelope| matches!(
                    envelope.message,
                    Some(BridgeMessage::RequestCancel(_))
                ))
                .count(),
            0,
            "the frame after the failure is never retired"
        );
        assert_eq!(
            hub.owner_count().await,
            0,
            "the frame after the failure never reaches the route"
        );
        assert!(
            hub.selected_path().await.is_none(),
            "the failure of the worker tears its epoch down"
        );
    }

    #[tokio::test]
    async fn a_ready_failure_of_the_worker_keeps_the_queued_frames_unpublished() {
        let (hub, state, mut nas_rx, _mac_rx) = routed_node().await;
        wait_until(|| async { !state.inner.workers.lock().await.is_empty() }).await;
        let (worker_id, worker) = {
            let workers = state.inner.workers.lock().await;
            let (worker_id, worker) = workers
                .iter()
                .next()
                .expect("the virtual worker of the route");
            (*worker_id, worker.clone())
        };

        // The failure of the worker is ready before the traffic of its route
        // is queued. The signal and the two frames are queued without an
        // await, so the pump observes the ready failure before it admits any
        // of them. This proves the lifecycle cleanup, not an interleaving
        // inside the recovery.
        state.inner.signal_worker_failure(worker_id);
        worker
            .try_send(http_start("before-failure"))
            .expect("the frame reaches the worker");
        worker
            .try_send(http_start("after-failure"))
            .expect("the later frame reaches the worker");

        // The ready failure enters the existing worker-failure cleanup: the
        // epoch of the route and the link of the database node end, no queued
        // frame is published and the failed worker releases its registration.
        wait_until(|| async { hub.selected_path().await.is_none() }).await;
        assert_eq!(
            hub.owner_count().await,
            0,
            "no frame of the failed worker reaches the route"
        );
        assert!(
            drain(&mut nas_rx)
                .await
                .iter()
                .all(|envelope| !matches!(envelope.message, Some(BridgeMessage::RequestStart(_)))),
            "no queued frame of the failed worker is published"
        );
        assert!(
            matches!(
                tokio::time::timeout(Duration::from_millis(200), nas_rx.recv()).await,
                Ok(None)
            ),
            "the ready failure stops the link of the database node"
        );
        assert!(
            !state.inner.workers.lock().await.contains_key(&worker_id),
            "the failed worker releases its registration"
        );
    }

    #[tokio::test]
    async fn a_closed_failure_watch_keeps_the_route_and_the_link_of_the_node() {
        let (hub, state, mut nas_rx, _mac_rx) = routed_node().await;
        wait_until(|| async { !state.inner.workers.lock().await.is_empty() }).await;
        let (worker_id, worker) = {
            let workers = state.inner.workers.lock().await;
            let (worker_id, worker) = workers
                .iter()
                .next()
                .expect("the virtual worker of the route");
            (*worker_id, worker.clone())
        };

        // The failure watch of the worker is closed before the traffic of its
        // route is queued, and the end of the watch is no failure
        // notification. The closure and the two frames are queued without an
        // await, so no interleaving inside the recovery is claimed.
        state.inner.remove_worker_failure(worker_id);
        worker
            .try_send(http_start("before-closure"))
            .expect("the frame reaches the worker");
        worker
            .try_send(http_start("after-closure"))
            .expect("the later frame reaches the worker");

        // The end of the watch enters the ordinary invalidation: the route and
        // the link of the database node stay, no queued frame is published and
        // the worker releases its registration.
        wait_until(|| async { !state.inner.workers.lock().await.contains_key(&worker_id) }).await;
        assert!(
            hub.selected_path().await.is_some(),
            "the end of the failure watch keeps the route of the node"
        );
        assert_eq!(
            hub.owner_count().await,
            0,
            "no frame of the ended watch reaches the route"
        );
        assert!(
            drain(&mut nas_rx)
                .await
                .iter()
                .all(|envelope| !matches!(envelope.message, Some(BridgeMessage::RequestStart(_)))),
            "no queued frame of the ended watch is published"
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(200), nas_rx.recv())
                .await
                .is_err(),
            "the end of the failure watch keeps the link of the database node"
        );
    }

    #[tokio::test]
    async fn a_stale_token_of_a_peer_publishes_nothing() {
        let (hub, state, mut nas_rx, mut mac_rx) = routed_node().await;
        let mac = super::super::admission::mint_for_test(id(MAC), 2);
        hub.forward_start(&mac, forwarded_path(), http_start("peer-1"))
            .await
            .expect("the start of the peer is forwarded");
        wait_until(|| async { hub.owner_count().await == 1 }).await;

        // The link of the peer ended, so its frame is a stale no-op.
        mac.mark_dead();
        assert!(
            hub.handle_peer_message(
                &mac,
                BridgeMessage::RequestChunk(crate::protocol::BridgeRequestChunk {
                    request_id: "peer-1".to_string(),
                    data: vec![7; 8],
                }),
            )
            .await,
            "the link of the peer survives the frame of a stale token"
        );
        assert_eq!(
            hub.owner_count().await,
            1,
            "the request of the peer keeps its owner"
        );
        assert!(
            drain(&mut mac_rx)
                .await
                .iter()
                .all(|envelope| !matches!(envelope.message, Some(BridgeMessage::ResponseError(_)))),
            "the peer learns nothing"
        );

        // The upstream work of the request was never cancelled.
        let frames = drain(&mut nas_rx).await;
        assert_eq!(
            frames
                .iter()
                .filter(|envelope| matches!(
                    envelope.message,
                    Some(BridgeMessage::RequestCancel(_))
                ))
                .count(),
            0,
            "the upstream work of the request is never cancelled"
        );
        let _ = state;
    }

    #[tokio::test]
    async fn a_retirement_of_an_unknown_or_foreign_request_retires_nothing() {
        let (hub, state, mut nas_rx, mut mac_rx) = routed_node().await;
        let mac = super::super::admission::mint_for_test(id(MAC), 2);
        hub.forward_start(&mac, forwarded_path(), http_start("peer-1"))
            .await
            .expect("the start of the peer is forwarded");
        wait_until(|| async { hub.owner_count().await == 1 }).await;
        let handle = hub.ensure_actor().await.expect("the actor of the hub");
        let worker = {
            state
                .inner
                .workers
                .lock()
                .await
                .values()
                .next()
                .cloned()
                .expect("the virtual worker of the route")
        };

        // A frame of an unknown request of the peer is refused, and the
        // request of the peer keeps its owner.
        assert!(
            refuse_a_peer_frame_while_full(
                &hub,
                &state,
                &handle,
                mac.clone(),
                BridgeMessage::RequestChunk(crate::protocol::BridgeRequestChunk {
                    request_id: "unknown".to_string(),
                    data: vec![7; 8],
                }),
                2,
            )
            .await,
            "the link of the peer survives the refusal of a frame"
        );
        assert_eq!(
            hub.owner_count().await,
            1,
            "the request of the peer keeps its owner"
        );
        assert!(
            drain(&mut mac_rx)
                .await
                .iter()
                .all(|envelope| !matches!(envelope.message, Some(BridgeMessage::ResponseError(_)))),
            "the peer of an unknown request learns nothing"
        );

        // A frame of a replaced generation of the peer is refused, and the
        // request of the peer keeps its owner.
        let stale = super::super::admission::mint_for_test(id(MAC), 9);
        assert!(
            refuse_a_peer_frame_while_full(
                &hub,
                &state,
                &handle,
                stale,
                BridgeMessage::RequestChunk(crate::protocol::BridgeRequestChunk {
                    request_id: "peer-1".to_string(),
                    data: vec![7; 8],
                }),
                3,
            )
            .await,
            "the link of the peer survives the refusal of a frame"
        );
        assert_eq!(
            hub.owner_count().await,
            1,
            "the request of the peer keeps its owner"
        );

        // The request of the local client carries the same id as the frame
        // of the peer, so the collision is a real one.
        worker
            .send(http_start("collision"))
            .await
            .expect("the request reaches the worker");
        wait_until(|| async { hub.owner_count().await == 2 }).await;

        // A frame of a peer that names the request of the local client never
        // retires it: the direction of the request is derived again.
        assert!(
            refuse_a_peer_frame_while_full(
                &hub,
                &state,
                &handle,
                mac.clone(),
                BridgeMessage::RequestChunk(crate::protocol::BridgeRequestChunk {
                    request_id: "collision".to_string(),
                    data: vec![7; 8],
                }),
                4,
            )
            .await,
            "the link of the peer survives the refusal of a frame"
        );
        assert_eq!(
            hub.owner_count().await,
            2,
            "the request of the local client keeps its owner"
        );

        // The upstream work of the requests was never cancelled.
        let frames = drain(&mut nas_rx).await;
        assert_eq!(
            frames
                .iter()
                .filter(|envelope| matches!(
                    envelope.message,
                    Some(BridgeMessage::RequestCancel(_))
                ))
                .count(),
            0,
            "the upstream work of the requests is never cancelled"
        );
    }

    #[tokio::test]
    async fn a_refused_upstream_frame_fails_the_downstream_caller() {
        let (hub, _state) = node(LOCAL);
        let nas = super::super::admission::mint_for_test(id(NAS), 1);
        let (nas_tx, mut nas_rx) = super::envelope_channel();
        let (nas_shutdown, _nas_shutdown_rx) = tokio::sync::watch::channel(false);
        assert!(hub.register_link(&nas, nas_tx.clone(), nas_shutdown).await);
        let mac = super::super::admission::mint_for_test(id(MAC), 2);
        let (mac_tx, mut mac_rx) = super::envelope_channel();
        let (mac_shutdown, _mac_shutdown_rx) = tokio::sync::watch::channel(false);
        assert!(hub.register_link(&mac, mac_tx, mac_shutdown).await);

        // The database node advertises its route.
        let path = PeerPath::new(vec![id(NAS)]).expect("a bounded path");
        hub.apply_update(&nas, 1, &path, snapshot(1))
            .await
            .expect("the route is applied");

        // A peer of the node sends a request.
        hub.forward_start(&mac, forwarded_path(), http_start("downstream-1"))
            .await
            .expect("the start is forwarded");
        let BridgeMessage::RequestStart(_) = next_bridge(&mut nas_rx).await else {
            panic!("the database node receives the start");
        };

        // The queue of the database node is full, so the body of the request
        // cannot travel upstream.
        let filler = PeerEnvelope::bridge(BridgeMessage::Pong);
        for _ in 0..super::PEER_ENVELOPE_BUFFER {
            nas_tx
                .try_send(filler.clone())
                .expect("the queue accepts its frames");
        }
        let chunk: BridgeMessage = serde_json::from_value(serde_json::json!({
            "type":"request_chunk", "request_id":"downstream-1", "data":[1,2,3]
        }))
        .unwrap();
        hub.handle_peer_message(&mac, chunk).await;

        // The healthy downstream caller learns that the request failed.
        let failure = loop {
            let queued = tokio::time::timeout(Duration::from_secs(5), mac_rx.recv())
                .await
                .expect("the peer receives the failure")
                .expect("the link is open");
            let (bytes, _permit) = queued.into_parts();
            let envelope = PeerEnvelope::decode(&bytes).expect("the envelope decodes");
            if let Some(BridgeMessage::ResponseError(error)) = envelope.message {
                break error;
            }
        };
        assert_eq!(
            failure.request_id, "downstream-1",
            "the failure reaches the caller of the request"
        );
        assert_eq!(
            hub.owner_count().await,
            0,
            "the owner of the failed request is retired exactly once"
        );
    }

    #[tokio::test]
    async fn the_counters_of_the_node_stay_bounded_under_peer_churn() {
        let (hub, _state) = node(LOCAL);
        let nas = super::super::admission::mint_for_test(id(NAS), 1);
        let (nas_tx, _nas_rx) = super::envelope_channel();
        let (nas_shutdown, _nas_shutdown_rx) = tokio::sync::watch::channel(false);
        assert!(hub.register_link(&nas, nas_tx, nas_shutdown).await);
        let path = PeerPath::new(vec![id(NAS)]).expect("a bounded path");
        hub.apply_update(&nas, 1, &path, snapshot(1))
            .await
            .expect("the route is applied");

        // A long series of distinct peers connects, learns the route and
        // disconnects again.
        for index in 0..40u128 {
            let peer = id(1000 + index);
            let (tx, _rx) = super::envelope_channel();
            let (shutdown, _rx) = tokio::sync::watch::channel(false);
            assert!(
                hub.register_link(
                    &super::super::admission::mint_for_test(peer, 1),
                    tx,
                    shutdown
                )
                .await
            );
            // The route changes, so the node advertises to every peer that is
            // connected.
            hub.apply_withdraw(&nas, 2 + index as u64 * 2)
                .await
                .unwrap_or_else(|err| panic!("the route is withdrawn at {index}: {err:?}"));
            hub.apply_update(&nas, 3 + index as u64 * 2, &path, snapshot(2))
                .await
                .expect("the route is applied");
            // The peer disconnects.
            hub.release_link(peer, 1).await;
        }

        // The counters stay bounded by the peers that are live.
        assert_eq!(
            hub.counter_count().await,
            1,
            "the counters stay bounded by the live peers"
        );
    }

    #[tokio::test]
    async fn a_cancelled_snapshot_caller_keeps_its_budget_until_the_command_retires() {
        let (hub, state) = node(LOCAL);
        let nas = super::super::admission::mint_for_test(id(NAS), 1);
        let (nas_tx, _nas_rx) = super::envelope_channel();
        let (nas_shutdown, _nas_shutdown_rx) = tokio::sync::watch::channel(false);
        assert!(hub.register_link(&nas, nas_tx, nas_shutdown).await);
        let handle = hub.ensure_actor().await.expect("the actor of the hub");
        let path = PeerPath::new(vec![id(NAS)]).expect("a bounded path");

        // The actor blocks in the snapshot of the relay state, so the
        // advertisement waits in the control path.
        let version = state.inner.config_version.lock().await;
        let abandoned = tokio::time::timeout(
            Duration::from_millis(100),
            hub.apply_update(&nas, 1, &path, snapshot(1)),
        )
        .await;
        assert!(
            abandoned.is_err(),
            "the caller abandons the answer of its command"
        );

        // The budget of the queued snapshot is still held, so a later
        // snapshot cannot take the same budget.
        assert!(
            handle.control_budget.available_permits() < super::PEER_CONTROL_BUDGET_BYTES,
            "the queued snapshot keeps its budget"
        );
        drop(version);

        // The command retires and releases the budget.
        wait_until(|| async {
            handle.control_budget.available_permits() == super::PEER_CONTROL_BUDGET_BYTES
        })
        .await;
    }
}
