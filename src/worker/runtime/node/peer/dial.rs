//! The dialing side of a peer link.
//!
//! A node may dial its peers as well as accept them. The dialer fetches the
//! challenge of the peer over a small HTTP request, proves that it shares the
//! master secret in the headers of the websocket upgrade, and it then runs
//! the link. The dialer retries with a bounded exponential backoff and a
//! jitter, so a peer that is not up yet does not turn into a busy loop, and a
//! fleet of nodes does not dial in lockstep.

use std::sync::Arc;
use std::time::Duration;

use anyhow::anyhow;
use futures::StreamExt;
use rand::RngExt;
use rustls::pki_types::ServerName;
use tokio::net::TcpStream;
use tokio::sync::watch;
use tokio_rustls::{TlsConnector, client::TlsStream};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tracing::{info, warn};

use crate::config::normalize_peer_url;
use crate::relay_secrets::RelaySecretManager;

use super::auth::{self, Challenge};
use super::handshake::Hello;
use super::topology::{NodeRole, check_pairing};
use super::transport::{
    PeerReader, PeerWriter, TungstenitePeerReader, TungstenitePeerWriter,
    tungstenite_peer_transport,
};
use super::{PeerLinkContext, run_link};
use crate::worker::runtime::node::identity::NodeId;

/// The first delay between two dial attempts.
const RECONNECT_BASE_DELAY: Duration = Duration::from_secs(1);

/// The largest delay between two dial attempts.
const RECONNECT_MAX_DELAY: Duration = Duration::from_secs(30);

/// The timeout of the challenge request of a peer.
const CHALLENGE_TIMEOUT: Duration = Duration::from_secs(5);

/// The deadline of a whole dial attempt.
///
/// The challenge, the TCP connection, the TLS handshake, the websocket upgrade
/// and the confirmation of the peer all have to finish inside it, so a peer
/// that stalls cannot pin the dialer.
const DIAL_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(20);

/// An outbound peer target.
///
/// The target is the normalized peer URL plus the parts the dialer needs:
/// the host, the port, the node endpoint path and whether the link is
/// secured. The scheme of the URL alone decides whether the link is secured,
/// independently of the TLS mode of the local listener.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PeerTarget {
    /// The normalized peer URL.
    pub(crate) url: String,
    /// The host of the peer.
    pub(crate) host: String,
    /// The port of the peer.
    pub(crate) port: u16,
    /// The path of the node endpoint.
    pub(crate) path: String,
    /// Whether the link is secured.
    pub(crate) secure: bool,
}

impl PeerTarget {
    /// Build the target of a peer URL.
    pub(crate) fn parse(value: &str) -> anyhow::Result<Self> {
        let canonical = normalize_peer_url(value)?;
        let parsed = reqwest::Url::parse(&canonical)
            .map_err(|_| anyhow!("the peer URL is not a valid URL"))?;
        let secure = parsed.scheme() == "wss";
        let host = parsed
            .host_str()
            .ok_or_else(|| anyhow!("the peer URL has no host"))?
            .to_string();
        let port = parsed.port().unwrap_or(if secure { 443 } else { 80 });
        let path = parsed.path().to_string();
        Ok(Self {
            url: canonical,
            host,
            port,
            path,
            secure,
        })
    }

    /// The URL of the challenge endpoint of the peer.
    ///
    /// The challenge travels over the same origin as the websocket link, so
    /// the peer needs no second port and no second address.
    fn challenge_url(&self) -> anyhow::Result<String> {
        let origin = crate::config::peer_http_origin(&self.url)?;
        Ok(format!("{origin}{}", auth::CHALLENGE_PATH))
    }
}

/// The delay before the next dial attempt.
///
/// The delay grows exponentially and is capped, and a jitter keeps the
/// retries of many nodes apart.
fn reconnect_delay(attempt: u32) -> Duration {
    let exponent = attempt.min(5);
    let base = RECONNECT_BASE_DELAY.saturating_mul(1_u32 << exponent);
    let capped = base.min(RECONNECT_MAX_DELAY);
    let jitter = rand::rng().random_range(0..500_u64);
    capped.saturating_add(Duration::from_millis(jitter))
}

/// Dial a peer and keep the link up.
///
/// The dialer stops when the shutdown signal fires. Every failed attempt is
/// reported and retried with the bounded backoff.
pub(crate) async fn run_peer_dialer(
    context: Arc<PeerLinkContext>,
    target: PeerTarget,
    tls: Option<Arc<rustls::ClientConfig>>,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut attempt: u32 = 0;
    loop {
        if *shutdown.borrow() {
            return;
        }
        // The establishment of the link is bounded; the admitted session runs
        // outside that deadline and is closed by the cooperative stop of the
        // node, so a healthy link is never dropped by a timeout.
        let outcome = if target.secure {
            Box::pin(establish_and_run_peer(
                &context,
                establish_peer_secure(&context, &target, tls.as_ref()),
                DIAL_ATTEMPT_TIMEOUT,
                &mut shutdown,
            ))
            .await
        } else {
            Box::pin(establish_and_run_peer(
                &context,
                establish_peer_plain(&context, &target),
                DIAL_ATTEMPT_TIMEOUT,
                &mut shutdown,
            ))
            .await
        };
        match outcome {
            Ok(()) => {
                // The link ended cleanly: the next attempt starts over.
                attempt = 0;
            }
            Err(err) => {
                warn!(error = %err, url = %target.url, "the peer link failed");
            }
        }
        if *shutdown.borrow() {
            return;
        }
        let delay = reconnect_delay(attempt);
        attempt = attempt.saturating_add(1);
        tokio::select! {
            _ = tokio::time::sleep(delay) => {}
            _ = shutdown.changed() => return,
        }
    }
}

/// The authenticated peer link that the establishment produced.
///
/// The establishment returns the owned transport and the validated metadata of
/// the peer; nothing of the link is admitted yet, so a dropped establishment
/// owns no admitted state, no tasks and no requests.
struct EstablishedPeer<R, W> {
    /// The reading half of the transport.
    reader: R,
    /// The writing half of the transport.
    writer: W,
    /// The hello of the peer that was authenticated.
    remote_hello: Hello,
}

/// Establish the peer link and run the admitted session.
///
/// The establishment is bounded by the deadline and the shutdown signal: the
/// challenge, the TCP connection, the TLS handshake, the websocket upgrade and
/// the confirmation of the peer all have to finish inside it. A dropped
/// establishment closes the transport and owns nothing else.
///
/// The admitted session runs outside that deadline. It is closed by the
/// cooperative stop of the node, which runs the cleanup of the link, so the
/// counters, the directory, the snapshot and the requests of the link are
/// always released.
async fn establish_and_run_peer<R, W, F>(
    context: &Arc<PeerLinkContext>,
    establishment: F,
    deadline: Duration,
    shutdown: &mut watch::Receiver<bool>,
) -> anyhow::Result<()>
where
    F: std::future::Future<Output = anyhow::Result<EstablishedPeer<R, W>>>,
    R: PeerReader + Send,
    W: PeerWriter + Send + 'static,
{
    let established = run_bounded_attempt(establishment, deadline, shutdown).await?;
    // The handoff between the establishment and the admission is a race:
    // the shutdown may fire while the establishment is finishing. The
    // transport is closed in that case and nothing is admitted, so the link
    // owns no counter, no directory slot and no request.
    if *shutdown.borrow() {
        anyhow::bail!("the node stopped its peer links before the link was admitted");
    }
    let remote = NodeId::from_uuid(established.remote_hello.node_id);
    let remote_role = NodeRole::from_has_database(established.remote_hello.has_database);
    check_pairing(
        context.identity.role,
        context.identity.node_id,
        remote_role,
        remote,
    )?;
    info!(node_id = %remote.as_uuid(), "the peer link is established");
    run_link(
        context,
        established.reader,
        established.writer,
        established.remote_hello,
        true,
        None,
    )
    .await
}

/// Run one dial attempt under the deadline of the attempt and the shutdown
/// signal.
///
/// The deadline encloses the whole establishment: the challenge, the TCP
/// connection, the TLS handshake, the websocket upgrade and the confirmation
/// of the peer. A peer that answers the challenge and then stalls cannot pin
/// the dialer: the attempt is dropped, its resources are released and the
/// dialer goes back to its backoff.
async fn run_bounded_attempt<F, T>(
    attempt: F,
    deadline: Duration,
    shutdown: &mut watch::Receiver<bool>,
) -> anyhow::Result<T>
where
    F: std::future::Future<Output = anyhow::Result<T>>,
{
    if *shutdown.borrow() {
        anyhow::bail!("the node stopped its peer links");
    }
    tokio::select! {
        biased;
        result = tokio::time::timeout(deadline, attempt) => {
            result.map_err(|_| anyhow!("the peer dial attempt did not finish in time"))?
        }
        _ = shutdown.changed() => Err(anyhow!("the node stopped its peer links")),
    }
}

/// The proof of this node for the challenge of the peer.
struct PeerProof {
    /// The fresh hello of this node.
    local_hello: Hello,
    /// The sealed ticket that the challenge of the peer carried.
    sealed_ticket: String,
    /// The opened ticket of the peer.
    ticket: auth::ChallengeTicket,
    /// The sealed proof of this node.
    proof: String,
}

/// Prepare the proof of this node for the challenge of the peer.
async fn prepare_peer(
    context: &Arc<PeerLinkContext>,
    target: &PeerTarget,
    tls: Option<&Arc<rustls::ClientConfig>>,
) -> anyhow::Result<PeerProof> {
    // The peer proves that it shares the master secret before the websocket
    // starts: the challenge carries its sealed hello, so the master secret
    // never travels.
    let challenge = fetch_challenge(target, tls).await?;
    let local_hello = context.identity.hello();
    let sealed_ticket = challenge.ticket.clone();
    let (proof, ticket) = auth::build_proof(
        &context.identity.manager,
        &local_hello,
        &challenge,
        auth::now_ms(),
    )?;
    Ok(PeerProof {
        local_hello,
        sealed_ticket,
        ticket,
        proof,
    })
}

/// The upgrade request of the peer link.
fn upgrade_request(
    target: &PeerTarget,
    sealed_ticket: &str,
    proof: &str,
) -> anyhow::Result<tokio_tungstenite::tungstenite::http::Request<()>> {
    let mut request = target
        .url
        .as_str()
        .into_client_request()
        .map_err(|err| anyhow!("the peer request is invalid: {err}"))?;
    let headers = request.headers_mut();
    headers.insert(
        auth::TICKET_HEADER,
        HeaderValue::from_str(sealed_ticket)
            .map_err(|_| anyhow!("the peer ticket is not a valid header value"))?,
    );
    headers.insert(
        auth::PROOF_HEADER,
        HeaderValue::from_str(proof)
            .map_err(|_| anyhow!("the peer proof is not a valid header value"))?,
    );
    Ok(request)
}

/// Establish the peer link over a plain connection.
///
/// The establishment owns the transport and the validated metadata of the
/// peer, and nothing else: the topology of the link is admitted by the caller
/// after the establishment succeeded.
async fn establish_peer_plain(
    context: &Arc<PeerLinkContext>,
    target: &PeerTarget,
) -> anyhow::Result<
    EstablishedPeer<TungstenitePeerReader<TcpStream>, TungstenitePeerWriter<TcpStream>>,
> {
    let prepared = prepare_peer(context, target, None).await?;
    let request = upgrade_request(target, &prepared.sealed_ticket, &prepared.proof)?;
    let stream = TcpStream::connect((target.host.as_str(), target.port))
        .await
        .map_err(|err| anyhow!("the peer connection failed: {err}"))?;
    upgrade_and_verify(
        context,
        request,
        stream,
        &prepared.local_hello,
        &prepared.ticket,
    )
    .await
}

/// Establish the peer link over a TLS connection.
///
/// The challenge and the websocket use the same client identity and the same
/// trust anchor, so the peer is reached exactly like the websocket link.
async fn establish_peer_secure(
    context: &Arc<PeerLinkContext>,
    target: &PeerTarget,
    tls: Option<&Arc<rustls::ClientConfig>>,
) -> anyhow::Result<
    EstablishedPeer<
        TungstenitePeerReader<TlsStream<TcpStream>>,
        TungstenitePeerWriter<TlsStream<TcpStream>>,
    >,
> {
    let tls = tls.ok_or_else(|| {
        anyhow!("the peer link is secured but no client trust anchor is configured")
    })?;
    let prepared = prepare_peer(context, target, Some(tls)).await?;
    let request = upgrade_request(target, &prepared.sealed_ticket, &prepared.proof)?;
    let stream = TcpStream::connect((target.host.as_str(), target.port))
        .await
        .map_err(|err| anyhow!("the peer connection failed: {err}"))?;
    let server_name = ServerName::try_from(target.host.clone())
        .map_err(|_| anyhow!("the peer host is not a valid TLS server name"))?;
    let stream = TlsConnector::from(tls.clone())
        .connect(server_name, stream)
        .await
        .map_err(|err| anyhow!("the peer TLS handshake failed: {err}"))?;
    upgrade_and_verify(
        context,
        request,
        stream,
        &prepared.local_hello,
        &prepared.ticket,
    )
    .await
}

/// Upgrade the connection and verify the confirmation of the peer.
///
/// The confirmation is checked in the response of the actual upgrade, before
/// the topology of the peer is admitted and before a single business frame is
/// sent.
async fn upgrade_and_verify<S>(
    context: &Arc<PeerLinkContext>,
    request: tokio_tungstenite::tungstenite::http::Request<()>,
    stream: S,
    local_hello: &Hello,
    ticket: &auth::ChallengeTicket,
) -> anyhow::Result<EstablishedPeer<TungstenitePeerReader<S>, TungstenitePeerWriter<S>>>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let config = super::transport::peer_socket_config();
    let (socket, response) =
        tokio_tungstenite::client_async_with_config(request, stream, Some(config))
            .await
            .map_err(|err| anyhow!("the peer websocket handshake failed: {err}"))?;
    // The socket is dropped when the confirmation is missing or wrong, so an
    // unverified peer never carries a business frame.
    verify_upgrade_confirmation(
        response.headers(),
        &context.identity.manager,
        local_hello,
        ticket,
    )?;
    let (reader, writer) = tungstenite_peer_transport(socket);
    Ok(EstablishedPeer {
        reader,
        writer,
        remote_hello: ticket.server_hello.clone(),
    })
}

/// Fetch the challenge of a peer.
///
/// The request uses the same TLS configuration as the websocket link, so the
/// peer is reached with the same client identity and the same trust anchor.
/// The body of the answer is bounded, the whole request has a timeout, and
/// redirects are refused: the request never leaves the origin of the peer, so
/// the client identity is never offered to another host.
async fn fetch_challenge(
    target: &PeerTarget,
    tls: Option<&Arc<rustls::ClientConfig>>,
) -> anyhow::Result<Challenge> {
    let url = target.challenge_url()?;
    let mut builder = reqwest::Client::builder()
        .timeout(CHALLENGE_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy();
    if let Some(tls) = tls {
        // The peer link and the challenge use exactly the same identity: the
        // client certificate of this node and the trust anchor of the peer.
        builder = builder.tls_backend_preconfigured((**tls).clone());
    }
    let client = builder
        .build()
        .map_err(|err| anyhow!("failed to build the peer challenge client: {err}"))?;
    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|err| anyhow!("the peer challenge request failed: {err}"))?;
    if !challenge_status_ok(response.status()) {
        anyhow::bail!(
            "the peer refused the challenge request with status {}",
            response.status().as_u16()
        );
    }
    // The answer must come from the origin of the request. The redirect
    // policy already refuses to follow a redirect, so this is the second
    // check that keeps the request on the origin of the peer.
    if !same_origin(&url, response.url()) {
        anyhow::bail!("the peer challenge came from another origin");
    }
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|err| anyhow!("failed to read the peer challenge: {err}"))?;
        if body.len() + chunk.len() > auth::MAX_AUTH_BYTES {
            anyhow::bail!("the peer challenge is larger than the accepted size");
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body).map_err(|_| anyhow!("the peer challenge is malformed"))
}

/// Whether a challenge answer may be used.
///
/// Only a success is accepted: a redirect is refused, so the request never
/// leaves the origin of the peer.
fn challenge_status_ok(status: reqwest::StatusCode) -> bool {
    status.is_success()
}

/// Whether two URLs have the same origin.
///
/// The scheme, the host and the port decide the origin; the path does not.
fn same_origin(expected: &str, actual: &reqwest::Url) -> bool {
    let Ok(expected) = reqwest::Url::parse(expected) else {
        return false;
    };
    expected.scheme() == actual.scheme()
        && expected.host_str() == actual.host_str()
        && expected.port_or_known_default() == actual.port_or_known_default()
}

/// Check the confirmation of the peer in the response of the upgrade.
///
/// The dialer never admits the topology of the peer and never sends a
/// business frame before this check passed, so a replayed challenge or a
/// forged upgrade response is never good enough.
fn verify_upgrade_confirmation(
    headers: &tokio_tungstenite::tungstenite::http::HeaderMap,
    manager: &RelaySecretManager,
    local_hello: &Hello,
    ticket: &auth::ChallengeTicket,
) -> anyhow::Result<()> {
    let confirmation = headers
        .get(auth::CONFIRMATION_HEADER)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| anyhow!("the peer did not confirm its identity"))?;
    auth::verify_confirmation(manager, local_hello, ticket, confirmation)
}

#[cfg(test)]
mod tests {
    use super::{
        EstablishedPeer, PeerTarget, challenge_status_ok, establish_and_run_peer, reconnect_delay,
        run_bounded_attempt, same_origin, verify_upgrade_confirmation,
    };
    use std::sync::Arc;
    use std::time::Duration;

    use tokio::sync::watch;

    use crate::relay::{AppState, test_state};
    use crate::relay_secrets::RelaySecretManager;
    use crate::worker::runtime::node::identity::NodeId;
    use crate::worker::runtime::node::peer::auth;
    use crate::worker::runtime::node::peer::handshake::Hello;
    use crate::worker::runtime::node::peer::topology::NodeRole;
    use crate::worker::runtime::node::peer::transport::{
        MemoryPeer, MemoryPeerReader, MemoryPeerWriter,
    };
    use crate::worker::runtime::node::peer::{NodeIdentity, PeerLinkContext, PeerRoleContext};

    fn manager(byte: u8) -> RelaySecretManager {
        RelaySecretManager::from_key_bytes([byte; 32])
    }

    fn hello(id: u128, has_database: bool) -> Hello {
        Hello::new(uuid::Uuid::from_u128(id), has_database, "1.2.3", 1)
    }

    #[test]
    fn a_peer_target_carries_the_scheme_and_the_endpoint() {
        let target = PeerTarget::parse("ws://peer-a:8787").expect("target");
        assert_eq!(target.host, "peer-a");
        assert_eq!(target.port, 8787);
        assert_eq!(target.path, "/_internal/node");
        assert!(!target.secure);

        let secure = PeerTarget::parse("wss://peer-b").expect("target");
        assert_eq!(secure.port, 443);
        assert!(secure.secure);
    }

    #[test]
    fn the_challenge_travels_over_the_origin_of_the_peer_link() {
        let target = PeerTarget::parse("ws://peer-a:8787").expect("target");
        assert_eq!(
            target.challenge_url().expect("challenge url"),
            "http://peer-a:8787/_internal/node/challenge"
        );
        let secure = PeerTarget::parse("wss://peer-b").expect("target");
        assert_eq!(
            secure.challenge_url().expect("challenge url"),
            "https://peer-b/_internal/node/challenge"
        );
    }

    #[test]
    fn the_reconnect_delay_grows_and_stays_bounded() {
        let first = reconnect_delay(0);
        let later = reconnect_delay(4);
        assert!(later > first);
        assert!(reconnect_delay(30) <= Duration::from_secs(31));
    }

    #[test]
    fn a_redirecting_challenge_is_refused() {
        assert!(challenge_status_ok(reqwest::StatusCode::OK));
        assert!(!challenge_status_ok(reqwest::StatusCode::FOUND));
        assert!(!challenge_status_ok(reqwest::StatusCode::MOVED_PERMANENTLY));
        assert!(!challenge_status_ok(
            reqwest::StatusCode::TEMPORARY_REDIRECT
        ));
        assert!(!challenge_status_ok(reqwest::StatusCode::BAD_REQUEST));
    }

    #[test]
    fn the_challenge_stays_on_the_origin_of_the_peer() {
        let expected = "http://peer-a:8787/_internal/node/challenge";
        assert!(same_origin(
            expected,
            &reqwest::Url::parse("http://peer-a:8787/_internal/node/challenge").expect("url")
        ));
        assert!(
            !same_origin(
                expected,
                &reqwest::Url::parse("http://peer-b:8787/_internal/node/challenge").expect("url")
            ),
            "another host is not the origin of the peer"
        );
        assert!(
            !same_origin(
                expected,
                &reqwest::Url::parse("https://peer-a:8787/_internal/node/challenge").expect("url")
            ),
            "another scheme is not the origin of the peer"
        );
        assert!(
            !same_origin(
                expected,
                &reqwest::Url::parse("http://peer-a:9999/_internal/node/challenge").expect("url")
            ),
            "another port is not the origin of the peer"
        );
    }

    #[tokio::test]
    async fn a_stalled_peer_does_not_pin_the_dialer() {
        let (_tx, mut shutdown) = watch::channel(false);
        let outcome = tokio::time::timeout(
            Duration::from_secs(5),
            run_bounded_attempt(
                std::future::pending::<anyhow::Result<()>>(),
                Duration::from_millis(50),
                &mut shutdown,
            ),
        )
        .await
        .expect("the attempt returns");
        assert!(
            outcome.is_err(),
            "a peer that never answers cannot hold the dial attempt"
        );
    }

    #[tokio::test]
    async fn the_shutdown_cancels_the_dial_attempt() {
        let (tx, mut shutdown) = watch::channel(false);
        let attempt = tokio::spawn(async move {
            run_bounded_attempt(
                std::future::pending::<anyhow::Result<()>>(),
                Duration::from_secs(30),
                &mut shutdown,
            )
            .await
        });
        tokio::time::sleep(Duration::from_millis(20)).await;
        tx.send_replace(true);
        let outcome = tokio::time::timeout(Duration::from_secs(5), attempt)
            .await
            .expect("the attempt returns")
            .expect("the attempt joins");
        assert!(outcome.is_err(), "the shutdown stops the dial attempt");
    }

    #[test]
    fn a_forged_upgrade_response_is_refused() {
        let server = hello(1, true);
        let dialer = hello(2, false);
        let epoch = auth::startup_epoch();
        let challenge = auth::issue_challenge(&manager(7), server.clone(), &epoch, auth::now_ms())
            .expect("challenge");
        let (proof, ticket) =
            auth::build_proof(&manager(7), &dialer, &challenge, auth::now_ms()).expect("proof");
        let verified = auth::verify_peer(
            &manager(7),
            &server,
            &epoch,
            &challenge.ticket,
            &proof,
            auth::now_ms(),
            &auth::ReplayCache::new(),
        )
        .expect("the peer is verified");
        let confirmation =
            auth::seal_confirmation(&manager(7), &verified.hello, &verified.ticket).expect("seal");

        // A response without a confirmation is refused.
        let forged = tokio_tungstenite::tungstenite::http::Response::builder()
            .status(101)
            .body(())
            .expect("response");
        assert!(
            verify_upgrade_confirmation(forged.headers(), &manager(7), &dialer, &ticket).is_err(),
            "a forged upgrade response without a confirmation is refused"
        );

        // The confirmation of the real server opens the link.
        let genuine = tokio_tungstenite::tungstenite::http::Response::builder()
            .status(101)
            .header(
                auth::CONFIRMATION_HEADER,
                tokio_tungstenite::tungstenite::http::HeaderValue::from_str(&confirmation)
                    .expect("header"),
            )
            .body(())
            .expect("response");
        assert!(
            verify_upgrade_confirmation(genuine.headers(), &manager(7), &dialer, &ticket).is_ok(),
            "the confirmation of the current server opens the link"
        );

        // A confirmation that was not sealed with the master secret is
        // refused, so a replayed challenge is never good enough.
        let attacker = tokio_tungstenite::tungstenite::http::Response::builder()
            .status(101)
            .header(
                auth::CONFIRMATION_HEADER,
                tokio_tungstenite::tungstenite::http::HeaderValue::from_str(
                    &auth::seal_confirmation(&manager(9), &verified.hello, &verified.ticket)
                        .expect("seal"),
                )
                .expect("header"),
            )
            .body(())
            .expect("response");
        assert!(
            verify_upgrade_confirmation(attacker.headers(), &manager(7), &dialer, &ticket).is_err(),
            "a confirmation of another master secret is refused"
        );
    }

    /// The peer context of an edge node and the relay state it forwards to.
    fn edge_context() -> (Arc<PeerLinkContext>, AppState) {
        let state = test_state();
        let context = PeerLinkContext::new(
            NodeIdentity {
                node_id: NodeId::from_uuid(uuid::Uuid::from_u128(1)),
                role: NodeRole::Edge,
                manager: manager(7),
            },
            PeerRoleContext::Edge(Arc::new(state.clone())),
            Duration::from_secs(90),
        );
        (context, state)
    }

    /// The establishment of an in-memory peer link.
    ///
    /// The establishment returns the owned transport and the hello of the
    /// peer, exactly like the establishment over the network does.
    fn established(
        local: MemoryPeer,
        remote_hello: Hello,
    ) -> impl std::future::Future<
        Output = anyhow::Result<EstablishedPeer<MemoryPeerReader, MemoryPeerWriter>>,
    > {
        let (reader, writer) = local.into_halves();
        async move {
            Ok(EstablishedPeer {
                reader,
                writer,
                remote_hello,
            })
        }
    }

    #[tokio::test]
    async fn a_healthy_peer_survives_the_establishment_deadline() {
        let (context, state) = edge_context();
        let (remote, local) = MemoryPeer::pair(16);
        let (_tx, mut shutdown) = watch::channel(false);
        let link = tokio::spawn({
            let context = context.clone();
            async move {
                establish_and_run_peer(
                    &context,
                    established(local, hello(2, true)),
                    Duration::from_millis(50),
                    &mut shutdown,
                )
                .await
            }
        });
        tokio::time::timeout(Duration::from_secs(5), async {
            while state.inner.workers.lock().await.is_empty() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the peer registered as the executor");

        // The deadline of the establishment is long over, but the admitted
        // link is still alive: the deadline does not reach the session.
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(
            context.active_count(),
            1,
            "the admitted link outlived the establishment deadline"
        );
        assert_eq!(context.directory.len().await, 1);
        assert_eq!(state.inner.workers.lock().await.len(), 1);

        // The cooperative shutdown closes the link and releases the counter,
        // the directory slot and the worker of the link.
        context.stop_sessions().await;
        let outcome = tokio::time::timeout(Duration::from_secs(5), link)
            .await
            .expect("the link ends")
            .expect("the link joins");
        assert!(outcome.is_ok(), "the link ends cleanly: {outcome:?}");
        assert_eq!(context.active_count(), 0);
        assert_eq!(context.directory.len().await, 0);
        assert!(
            state.inner.workers.lock().await.is_empty(),
            "the worker of the closed link is released"
        );
        assert!(state.inner.pending.lock().await.is_empty());
        drop(remote);
    }

    #[tokio::test]
    async fn a_stalled_establishment_admits_nothing() {
        let (context, state) = edge_context();
        let (_tx, mut shutdown) = watch::channel(false);
        let outcome = establish_and_run_peer(
            &context,
            std::future::pending::<
                anyhow::Result<EstablishedPeer<MemoryPeerReader, MemoryPeerWriter>>,
            >(),
            Duration::from_millis(50),
            &mut shutdown,
        )
        .await;
        assert!(
            outcome.is_err(),
            "a peer that never completes the establishment is dropped"
        );
        assert_eq!(context.active_count(), 0);
        assert_eq!(context.directory.len().await, 0);
        assert!(state.inner.workers.lock().await.is_empty());
    }

    #[tokio::test]
    async fn the_shutdown_during_the_establishment_admits_nothing() {
        let (context, state) = edge_context();
        let (tx, mut shutdown) = watch::channel(false);
        let attempt = tokio::spawn({
            let context = context.clone();
            async move {
                establish_and_run_peer(
                    &context,
                    std::future::pending::<
                        anyhow::Result<EstablishedPeer<MemoryPeerReader, MemoryPeerWriter>>,
                    >(),
                    Duration::from_secs(30),
                    &mut shutdown,
                )
                .await
            }
        });
        tokio::time::sleep(Duration::from_millis(20)).await;
        tx.send_replace(true);
        let outcome = tokio::time::timeout(Duration::from_secs(5), attempt)
            .await
            .expect("the attempt returns")
            .expect("the attempt joins");
        assert!(outcome.is_err(), "the shutdown cancels the establishment");
        assert_eq!(context.active_count(), 0);
        assert_eq!(context.directory.len().await, 0);
        assert!(state.inner.workers.lock().await.is_empty());
    }

    #[tokio::test]
    async fn the_shutdown_in_the_handoff_avoids_the_admission() {
        let (context, state) = edge_context();
        let (_remote, local) = MemoryPeer::pair(16);
        let (tx, mut shutdown) = watch::channel(false);
        // The shutdown fires in the last moment of the establishment: the
        // transport is ready, but the link is not admitted yet.
        let establishment = async move {
            tx.send_replace(true);
            let (reader, writer) = local.into_halves();
            Ok(EstablishedPeer {
                reader,
                writer,
                remote_hello: hello(2, true),
            })
        };
        let outcome = establish_and_run_peer(
            &context,
            establishment,
            Duration::from_secs(5),
            &mut shutdown,
        )
        .await;
        let err = outcome.expect_err("the shutdown avoids the admission");
        assert!(
            err.to_string().contains("before the link was admitted"),
            "the link is not admitted after the shutdown: {err}"
        );
        assert_eq!(context.active_count(), 0);
        assert_eq!(context.directory.len().await, 0);
        assert!(state.inner.workers.lock().await.is_empty());
    }
}
