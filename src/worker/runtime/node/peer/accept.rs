//! The accepted side of a peer link.
//!
//! The peer endpoint of a node is part of its single listener: the node
//! authenticates the peer during the HTTP exchange that precedes the
//! websocket upgrade, and it then runs the ordinary link session. No
//! websocket frame of an unauthenticated peer is ever read.

use std::sync::Arc;

use axum::extract::FromRequestParts;
use axum::extract::ws::WebSocketUpgrade;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use tracing::warn;

use super::auth;
use super::topology::{NodeRole, check_pairing};
use super::transport::axum_peer_transport;
use super::{PeerLinkContext, run_link};
use crate::bridge::wire::BRIDGE_WS_MAX_FRAME_BYTES;
use crate::worker::runtime::node::identity::NodeId;

/// Hand out the challenge of this node.
///
/// The challenge carries the hello of this node and a ticket that seals it
/// under the master secret. The ticket never carries the master secret
/// itself, and it is only valid for a short time.
pub(crate) async fn peer_challenge(context: Arc<PeerLinkContext>) -> Response {
    // The unauthenticated work of the node is bounded: a peer that floods
    // the challenge endpoint cannot take more than the handshake slots.
    let permit = match context.preauth.clone().try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                "too many peer handshakes are in flight",
            )
                .into_response();
        }
    };
    let hello = context.identity.hello();
    let challenge = auth::issue_challenge(
        &context.identity.manager,
        hello,
        &context.epoch,
        auth::now_ms(),
    );
    drop(permit);
    match challenge {
        Ok(challenge) => axum::Json(challenge).into_response(),
        Err(err) => {
            warn!(error = %err, "failed to issue the peer challenge");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to issue the peer challenge",
            )
                .into_response()
        }
    }
}

/// Accept a peer link.
///
/// The proof of the peer is verified before the upgrade, so the websocket
/// only ever carries the frames of an authenticated peer. The unauthenticated
/// handshakes are bounded by a semaphore, so a node cannot be flooded with
/// half-open connections.
pub(crate) async fn peer_upgrade(
    context: Arc<PeerLinkContext>,
    request: axum::extract::Request,
) -> Response {
    let (mut parts, _body) = request.into_parts();
    let permit = match context.preauth.clone().try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                "too many peer handshakes are in flight",
            )
                .into_response();
        }
    };
    let ticket = parts.headers.get(auth::TICKET_HEADER);
    let proof = parts.headers.get(auth::PROOF_HEADER);
    let verified = match (ticket, proof) {
        (Some(ticket), Some(proof)) => {
            let ticket = match ticket.to_str() {
                Ok(ticket) => ticket,
                Err(_) => {
                    drop(permit);
                    return (StatusCode::UNAUTHORIZED, "the peer proof is malformed")
                        .into_response();
                }
            };
            let proof = match proof.to_str() {
                Ok(proof) => proof,
                Err(_) => {
                    drop(permit);
                    return (StatusCode::UNAUTHORIZED, "the peer proof is malformed")
                        .into_response();
                }
            };
            let local_hello = context.identity.hello();
            match auth::verify_peer(
                &context.identity.manager,
                &local_hello,
                &context.epoch,
                ticket,
                proof,
                auth::now_ms(),
                &context.replay,
            ) {
                Ok(verified) => verified,
                Err(err) => {
                    warn!(error = %err, "the peer proof was refused");
                    drop(permit);
                    return (StatusCode::UNAUTHORIZED, "the peer proof was refused")
                        .into_response();
                }
            }
        }
        _ => {
            drop(permit);
            return (
                StatusCode::UNAUTHORIZED,
                "the peer request carries no proof",
            )
                .into_response();
        }
    };
    let remote_hello = verified.hello;
    // The confirmation of this node is sealed only after the proof of the peer
    // was verified, so it is never a public signing oracle. The dialer checks
    // it in the upgrade response before it sends a single business frame.
    let confirmation =
        match auth::seal_confirmation(&context.identity.manager, &remote_hello, &verified.ticket) {
            Ok(confirmation) => confirmation,
            Err(err) => {
                warn!(error = %err, "failed to seal the confirmation of the peer");
                drop(permit);
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "failed to seal the confirmation of the peer",
                )
                    .into_response();
            }
        };
    if crate::relay::node_peer_cap_reached(context.directory.len().await) {
        warn!("the node already serves the maximum number of peers");
        drop(permit);
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "the node already serves the maximum number of peers",
        )
            .into_response();
    }
    let remote = NodeId::from_uuid(remote_hello.node_id);
    let remote_role = NodeRole::from_has_database(remote_hello.has_database);
    if let Err(err) = check_pairing(
        context.identity.role,
        context.identity.node_id,
        remote_role,
        remote,
    ) {
        warn!(error = %err, "the peer cannot be linked to this node");
        drop(permit);
        return (
            StatusCode::FORBIDDEN,
            "the peer cannot be linked to this node",
        )
            .into_response();
    }
    if context.is_stopping() {
        warn!("the node stopped its peer links");
        drop(permit);
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "the node stopped its peer links",
        )
            .into_response();
    }
    // The upgrade itself is the last step: the proof of the peer is already
    // verified, so the websocket only carries the frames of an authenticated
    // peer.
    let upgrade = match WebSocketUpgrade::from_request_parts(&mut parts, &()).await {
        Ok(upgrade) => upgrade,
        Err(rejection) => {
            drop(permit);
            return rejection.into_response();
        }
    };
    let response = upgrade
        .max_frame_size(BRIDGE_WS_MAX_FRAME_BYTES + super::wire::PEER_ENVELOPE_MAX_HEADER_BYTES)
        .max_message_size(super::transport::peer_max_message_bytes())
        .on_upgrade(move |socket| async move {
            let (reader, writer) = axum_peer_transport(socket);
            // The session is owned by the node, so the shutdown path can close
            // it before the resources of the node are released. The upgrade
            // callback waits for the session, because the connection lives as
            // long as the session does.
            let (done_tx, done_rx) = tokio::sync::oneshot::channel();
            let session_context = context.clone();
            let session = tokio::spawn(async move {
                if let Err(err) = run_link(
                    &session_context,
                    reader,
                    writer,
                    remote_hello,
                    false,
                    Some(permit),
                )
                .await
                {
                    warn!(error = %err, "the peer link ended with an error");
                }
                let _ = done_tx.send(());
            });
            context.register_session(session).await;
            let _ = done_rx.await;
        });
    // The confirmation travels in the response of the upgrade, so the dialer
    // can check that the node that answered is the node that owns the master
    // secret and that it answered this very handshake.
    attach_confirmation(response, &confirmation)
}

/// Attach the sealed confirmation of this node to the upgrade response.
///
/// The dialer reads the confirmation from the response of the actual upgrade
/// and checks it before it admits the peer or sends a business frame.
fn attach_confirmation(mut response: Response, confirmation: &str) -> Response {
    if let Ok(value) = axum::http::HeaderValue::from_str(confirmation) {
        response.headers_mut().insert(
            axum::http::HeaderName::from_static(auth::CONFIRMATION_HEADER),
            value,
        );
    }
    response
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use axum::routing::get;
    use tokio::sync::Semaphore;
    use tower::Service;

    use super::super::auth::{self, Challenge, ReplayCache};
    use super::super::handshake::{Hello, PREAUTH_CONCURRENCY};
    use super::super::transport::MemoryPeer;
    use super::super::{NodeIdentity, NodeRole, PeerLinkContext, PeerRoleContext, run_link};
    use crate::relay_secrets::RelaySecretManager;
    use crate::worker::runtime::node::identity::NodeId;

    /// The context of a node that accepts peers.
    fn accepting_context() -> Arc<PeerLinkContext> {
        PeerLinkContext::new(
            NodeIdentity {
                node_id: NodeId::from_uuid(uuid::Uuid::from_u128(1000)),
                role: NodeRole::Edge,
                manager: RelaySecretManager::from_key_bytes([7; 32]),
            },
            PeerRoleContext::Edge(Arc::new(crate::relay::test_state())),
            std::time::Duration::from_secs(90),
        )
    }

    /// A request that asks for a websocket upgrade.
    fn upgrade_request() -> Request<Body> {
        Request::builder()
            .method("GET")
            .uri(crate::config::NODE_ENDPOINT_PATH)
            .header("connection", "Upgrade")
            .header("upgrade", "websocket")
            .header("sec-websocket-version", "13")
            .header("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==")
            .body(Body::empty())
            .expect("request")
    }

    #[tokio::test]
    async fn the_challenge_endpoint_hands_out_a_ticket_that_opens_the_link() {
        let context = accepting_context();
        let handler_context = context.clone();
        let mut app = axum::Router::new().route(
            super::super::CHALLENGE_PATH,
            get(move || {
                let context = handler_context.clone();
                async move { super::peer_challenge(context).await }
            }),
        );
        let request = Request::builder()
            .uri(super::super::CHALLENGE_PATH)
            .body(Body::empty())
            .expect("request");
        let response = app.call(request).await.expect("the challenge answers");
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), auth::MAX_AUTH_BYTES)
            .await
            .expect("the challenge body");
        let challenge: Challenge =
            serde_json::from_slice(&body).expect("the challenge is a challenge");

        // The client answers the challenge with a proof, and the node opens
        // the link with it.
        let client = Hello::new(
            uuid::Uuid::from_u128(1),
            true,
            env!("CARGO_PKG_VERSION"),
            crate::worker::runtime::node::peer::wire::PEER_PROTOCOL_VERSION,
        );
        let (proof, _server) = auth::build_proof(
            &context.identity.manager,
            &client,
            &challenge,
            auth::now_ms(),
        )
        .expect("proof");
        let verified = auth::verify_peer(
            &context.identity.manager,
            &context.identity.hello(),
            &context.epoch,
            &challenge.ticket,
            &proof,
            auth::now_ms(),
            &context.replay,
        )
        .expect("the peer is verified");
        assert_eq!(verified.hello.node_id, client.node_id);
    }

    #[tokio::test]
    async fn the_upgrade_is_refused_without_a_proof() {
        let context = accepting_context();
        let handler_context = context.clone();
        let mut app = axum::Router::new().route(
            crate::config::NODE_ENDPOINT_PATH,
            get(move |request: axum::extract::Request| {
                let context = handler_context.clone();
                async move { super::peer_upgrade(context, request).await }
            }),
        );
        let response = app
            .call(upgrade_request())
            .await
            .expect("the upgrade answers");
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "a websocket upgrade without a proof is refused"
        );
    }

    #[tokio::test]
    async fn the_upgrade_is_refused_with_an_invalid_proof() {
        let context = accepting_context();
        let handler_context = context.clone();
        let mut app = axum::Router::new().route(
            crate::config::NODE_ENDPOINT_PATH,
            get(move |request: axum::extract::Request| {
                let context = handler_context.clone();
                async move { super::peer_upgrade(context, request).await }
            }),
        );
        let mut request = upgrade_request();
        request
            .headers_mut()
            .insert(auth::TICKET_HEADER, "not-a-ticket".parse().expect("header"));
        request
            .headers_mut()
            .insert(auth::PROOF_HEADER, "not-a-proof".parse().expect("header"));
        let response = app.call(request).await.expect("the upgrade answers");
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "a websocket upgrade with an invalid proof is refused"
        );
    }

    #[test]
    fn the_confirmation_of_the_server_travels_in_the_upgrade_response() {
        let context = accepting_context();
        let server = Hello::new(
            uuid::Uuid::from_u128(1000),
            true,
            env!("CARGO_PKG_VERSION"),
            crate::worker::runtime::node::peer::wire::PEER_PROTOCOL_VERSION,
        );
        let dialer = Hello::new(
            uuid::Uuid::from_u128(1),
            false,
            env!("CARGO_PKG_VERSION"),
            crate::worker::runtime::node::peer::wire::PEER_PROTOCOL_VERSION,
        );
        let epoch = auth::startup_epoch();
        let challenge = auth::issue_challenge(
            &context.identity.manager,
            server.clone(),
            &epoch,
            auth::now_ms(),
        )
        .expect("challenge");
        let (proof, ticket) = auth::build_proof(
            &context.identity.manager,
            &dialer,
            &challenge,
            auth::now_ms(),
        )
        .expect("proof");
        let verified = auth::verify_peer(
            &context.identity.manager,
            &server,
            &epoch,
            &challenge.ticket,
            &proof,
            auth::now_ms(),
            &auth::ReplayCache::new(),
        )
        .expect("the peer is verified");
        let confirmation =
            auth::seal_confirmation(&context.identity.manager, &verified.hello, &verified.ticket)
                .expect("seal");

        // The upgrade response carries the confirmation of the server.
        let response = super::attach_confirmation(
            axum::response::Response::builder()
                .status(axum::http::StatusCode::SWITCHING_PROTOCOLS)
                .body(axum::body::Body::empty())
                .expect("response"),
            &confirmation,
        );
        let header = response
            .headers()
            .get(auth::CONFIRMATION_HEADER)
            .expect("the confirmation header")
            .to_str()
            .expect("the confirmation is text");
        assert_eq!(header, confirmation);
        // The dialer can open it with the master secret.
        auth::verify_confirmation(&context.identity.manager, &dialer, &ticket, header)
            .expect("the confirmation opens");
    }

    #[tokio::test]
    async fn authenticated_links_do_not_hold_handshake_slots() {
        let slots = Arc::new(Semaphore::new(PREAUTH_CONCURRENCY));
        let mut connected = Vec::new();
        let mut sessions = Vec::new();
        let mut states = Vec::new();
        let mut contexts = Vec::new();
        for id in 0..(PREAUTH_CONCURRENCY + 1) {
            let permit = slots.clone().try_acquire_owned().expect("slot available");
            let (left, right) = MemoryPeer::pair(8);
            let (left_reader, left_writer) = left.into_halves();
            let (right_reader, right_writer) = right.into_halves();
            let manager = RelaySecretManager::from_key_bytes([7; 32]);
            let server = Hello::new(
                uuid::Uuid::from_u128(1000),
                true,
                "1",
                crate::worker::runtime::node::peer::wire::PEER_PROTOCOL_VERSION,
            );
            let client = Hello::new(
                uuid::Uuid::from_u128(id as u128 + 1),
                false,
                "1",
                crate::worker::runtime::node::peer::wire::PEER_PROTOCOL_VERSION,
            );
            // The peer proves itself before the link starts: the challenge of
            // the node and the proof of the peer replace the frames of the
            // old handshake.
            let epoch = auth::startup_epoch();
            let challenge = auth::issue_challenge(&manager, server.clone(), &epoch, auth::now_ms())
                .expect("challenge");
            let (proof, _server) =
                auth::build_proof(&manager, &client, &challenge, auth::now_ms()).expect("proof");
            let replay = ReplayCache::new();
            let verified = auth::verify_peer(
                &manager,
                &server,
                &epoch,
                &challenge.ticket,
                &proof,
                auth::now_ms(),
                &replay,
            )
            .expect("accepted proof");
            let hello = verified.hello;
            connected.push((left_reader, left_writer));
            let state = crate::relay::test_state();
            states.push(state.clone());
            let context = PeerLinkContext::new(
                NodeIdentity {
                    node_id: NodeId::from_uuid(uuid::Uuid::from_u128(1000)),
                    role: NodeRole::Edge,
                    manager: RelaySecretManager::from_key_bytes([7; 32]),
                },
                PeerRoleContext::Edge(Arc::new(state)),
                std::time::Duration::from_secs(90),
            );
            contexts.push(context.clone());
            sessions.push(tokio::spawn(async move {
                run_link(
                    &context,
                    right_reader,
                    right_writer,
                    hello,
                    false,
                    Some(permit),
                )
                .await
            }));
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                while slots.available_permits() != PREAUTH_CONCURRENCY {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("admission releases permit");
        }
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            for context in &contexts {
                while context.directory.len().await == 0 {
                    tokio::task::yield_now().await;
                }
            }
        })
        .await
        .expect("all authenticated sessions are admitted");
        assert_eq!(slots.available_permits(), PREAUTH_CONCURRENCY);
        drop(connected);
        for session in sessions {
            session
                .await
                .expect("session joins")
                .expect("session closes");
        }
        for state in &states {
            assert!(state.inner.workers.lock().await.is_empty());
        }
    }
}
