//! The authentication of a peer link that happens before the websocket
//! upgrade.
//!
//! A peer link proves that both nodes share the same master secret during the
//! HTTP exchange that precedes the websocket upgrade, so no websocket frame of
//! an unauthenticated peer is ever read. The master secret itself never
//! travels on the wire:
//!
//! * the server hands out a challenge that carries its hello and a ticket,
//!   and the ticket seals that hello, the issue time, the expiry and the
//!   startup epoch of the server under the master secret,
//! * the client opens the ticket, checks that it is fresh and that it belongs
//!   to a node of the same version, and answers with a proof that seals its
//!   own hello and the transcript of both hellos,
//! * the server opens the proof, checks the transcript, the identity, the
//!   version and the topology, and consumes the nonce of the ticket once,
//! * the server answers with a confirmation that seals the fresh hello of the
//!   client, the hello of the server, the nonce of the ticket and the startup
//!   epoch, and the client checks that confirmation before it admits the
//!   topology or sends a single business frame.
//!
//! The ticket is bound to the startup epoch of the server, so a ticket of a
//! previous process is worthless, and the consumed nonces are remembered for
//! the lifetime of the ticket, so a proof cannot be replayed. The
//! confirmation binds the server to the fresh hello of the client, so a
//! replayed challenge is not a proof of the current server: an endpoint that
//! only replays a challenge cannot answer with a confirmation of its own.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use rand::Rng;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::relay_secrets::{EncryptedSecretEnvelope, RelaySecretManager};

use super::handshake::{Hello, canonical_confirmation, canonical_transcript};

/// The largest ticket, proof, confirmation or challenge body of the
/// authentication exchange.
pub const MAX_AUTH_BYTES: usize = 16 * 1024;

/// How long a challenge ticket stays valid.
pub const CHALLENGE_TTL: Duration = Duration::from_secs(60);

/// How many consumed tickets the node remembers at the same time.
pub const REPLAY_CACHE_CAPACITY: usize = 1024;

/// The clock skew the issue time of a ticket may have.
const TICKET_CLOCK_SKEW: Duration = Duration::from_secs(5);

/// The header that carries the sealed ticket of the peer.
pub const TICKET_HEADER: &str = "x-prompt-ferry-peer-ticket";

/// The header that carries the sealed proof of the peer.
pub const PROOF_HEADER: &str = "x-prompt-ferry-peer-proof";

/// The header of the websocket upgrade response that carries the sealed
/// confirmation of the server.
pub const CONFIRMATION_HEADER: &str = "x-prompt-ferry-peer-confirmation";

/// The path of the challenge endpoint of a node.
pub const CHALLENGE_PATH: &str = "/_internal/node/challenge";

/// The domain of the sealed confirmation of the server.
///
/// The confirmation has its own domain, so it can never be read as a proof of
/// a peer.
const CONFIRMATION_DOMAIN: &str = "pf64-peer-confirmation-v1";

/// The challenge a node hands out before the websocket upgrade.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Challenge {
    /// The hello of the server.
    pub server_hello: Hello,
    /// The sealed ticket that carries the hello of the server.
    pub ticket: String,
}

/// What the client learned from the challenge of the server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChallengeTicket {
    /// The hello of the server that was sealed into the ticket.
    pub server_hello: Hello,
    /// The one-time nonce of the ticket.
    pub nonce: String,
    /// The startup epoch of the server.
    pub epoch: String,
    /// When the ticket expires, in milliseconds since the unix epoch.
    pub expires_at_ms: i64,
}

/// A verified peer and the ticket that authenticated it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedPeer {
    /// The hello of the peer.
    pub hello: Hello,
    /// The ticket that the peer answered.
    pub ticket: ChallengeTicket,
}

/// The plaintext of a sealed challenge ticket.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Ticket {
    /// The hello of the node that issued the ticket.
    hello: Hello,
    /// The startup epoch of the node that issued the ticket.
    epoch: String,
    /// The one-time nonce of the ticket.
    nonce: String,
    /// When the ticket was issued, in milliseconds since the unix epoch.
    issued_at_ms: i64,
    /// When the ticket expires, in milliseconds since the unix epoch.
    expires_at_ms: i64,
}

/// The plaintext of a sealed peer proof.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Proof {
    /// The hello of the peer that proved the master secret.
    hello: Hello,
    /// The transcript of both hellos.
    transcript: String,
}

/// The plaintext of the sealed confirmation of the server.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Confirmation {
    /// The domain of the confirmation.
    domain: String,
    /// The fresh hello of the dialer.
    dialer_hello: Hello,
    /// The hello of the server that was sealed into the ticket.
    server_hello: Hello,
    /// The nonce of the ticket that the dialer answered.
    nonce: String,
    /// The startup epoch of the server.
    epoch: String,
    /// The transcript of both hellos.
    transcript: String,
}

/// The current time in milliseconds since the unix epoch.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or_default()
}

/// The startup epoch of a node.
///
/// The epoch is fresh for every process, so a ticket of a previous process is
/// refused even when it did not expire yet.
pub fn startup_epoch() -> String {
    let mut bytes = [0_u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    STANDARD.encode(bytes)
}

/// The nonces of the tickets that were already used.
///
/// The cache is bounded: a node that cannot remember a nonce refuses the
/// ticket instead of evicting a live one, because an eviction would allow a
/// replay.
#[derive(Debug, Default)]
pub struct ReplayCache {
    entries: Mutex<HashMap<String, i64>>,
}

impl ReplayCache {
    /// An empty cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// Remember a nonce, or refuse it when it was used already or the cache
    /// is full.
    pub fn consume(&self, nonce: &str, expires_at_ms: i64) -> anyhow::Result<()> {
        let now = now_ms();
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| anyhow::anyhow!("the peer ticket cache is poisoned"))?;
        entries.retain(|_, expiry| *expiry > now);
        if entries.contains_key(nonce) {
            anyhow::bail!("the peer ticket was already used");
        }
        if entries.len() >= REPLAY_CACHE_CAPACITY {
            anyhow::bail!("the peer ticket cache is full");
        }
        entries.insert(nonce.to_string(), expires_at_ms);
        Ok(())
    }
}

/// Issue the challenge of this node.
///
/// The hello of the node is fresh, and the ticket seals it together with the
/// issue time, the expiry and the startup epoch of the node.
pub fn issue_challenge(
    manager: &RelaySecretManager,
    hello: Hello,
    epoch: &str,
    now: i64,
) -> anyhow::Result<Challenge> {
    let mut nonce = [0_u8; 16];
    rand::rng().fill_bytes(&mut nonce);
    let ticket = Ticket {
        hello: hello.clone(),
        epoch: epoch.to_string(),
        nonce: STANDARD.encode(nonce),
        issued_at_ms: now,
        expires_at_ms: now + CHALLENGE_TTL.as_millis() as i64,
    };
    let ticket = seal(manager, &ticket)?;
    Ok(Challenge {
        server_hello: hello,
        ticket,
    })
}

/// Open the challenge of a server.
///
/// The ticket is sealed under the master secret, so the client learns the
/// hello, the nonce and the startup epoch of the server from the ticket
/// itself and not from the announced body of the challenge.
pub fn open_challenge(
    manager: &RelaySecretManager,
    challenge: &Challenge,
    now: i64,
) -> anyhow::Result<ChallengeTicket> {
    let (ticket, _envelope) = open::<Ticket>(manager, &challenge.ticket)?;
    check_ticket_times(&ticket, now)?;
    if ticket.hello != challenge.server_hello {
        anyhow::bail!("the peer challenge does not match its ticket");
    }
    Ok(ChallengeTicket {
        server_hello: ticket.hello,
        nonce: ticket.nonce,
        epoch: ticket.epoch,
        expires_at_ms: ticket.expires_at_ms,
    })
}

/// Answer the challenge of a server with the proof of this node.
///
/// The proof carries the hello of this node and the transcript of both
/// hellos, so the server can bind the proof to the exact challenge it handed
/// out. The ticket that the challenge carried is returned with the proof: the
/// client needs its nonce and its epoch to check the confirmation of the
/// server.
pub fn build_proof(
    manager: &RelaySecretManager,
    local_hello: &Hello,
    challenge: &Challenge,
    now: i64,
) -> anyhow::Result<(String, ChallengeTicket)> {
    let ticket = open_challenge(manager, challenge, now)?;
    check_hello(&ticket.server_hello, local_hello)?;
    let proof = Proof {
        hello: local_hello.clone(),
        transcript: canonical_transcript(local_hello, &ticket.server_hello),
    };
    Ok((seal(manager, &proof)?, ticket))
}

/// Verify the ticket and the proof of a peer.
///
/// The verification happens before the websocket upgrade, so an unauthenticated
/// peer never reaches the websocket layer.
pub fn verify_peer(
    manager: &RelaySecretManager,
    local_hello: &Hello,
    local_epoch: &str,
    ticket: &str,
    proof: &str,
    now: i64,
    replay: &ReplayCache,
) -> anyhow::Result<VerifiedPeer> {
    if ticket.len() > MAX_AUTH_BYTES || proof.len() > MAX_AUTH_BYTES {
        anyhow::bail!("the peer authentication is larger than {MAX_AUTH_BYTES} bytes");
    }
    let (ticket, _envelope) = open::<Ticket>(manager, ticket)?;
    if ticket.epoch != local_epoch {
        anyhow::bail!("the peer ticket belongs to another process of the peer");
    }
    if ticket.hello.node_id != local_hello.node_id {
        anyhow::bail!("the peer ticket was not issued by this node");
    }
    check_ticket_times(&ticket, now)?;
    let (proof, _envelope) = open::<Proof>(manager, proof)?;
    let expected = canonical_transcript(&proof.hello, &ticket.hello);
    if proof.transcript != expected {
        anyhow::bail!("the peer proof does not match the challenge of this node");
    }
    check_hello(&proof.hello, local_hello)?;
    replay.consume(&ticket.nonce, ticket.expires_at_ms)?;
    Ok(VerifiedPeer {
        hello: proof.hello,
        ticket: ChallengeTicket {
            server_hello: ticket.hello,
            nonce: ticket.nonce,
            epoch: ticket.epoch,
            expires_at_ms: ticket.expires_at_ms,
        },
    })
}

/// Seal the confirmation of this node for a peer.
///
/// The confirmation binds the server to the fresh hello of the dialer, to the
/// nonce of the ticket and to the startup epoch of the server, so only the
/// node that owns the master secret and that issued the ticket can produce
/// it. It is sealed after the proof of the peer was verified, so it is never
/// a public signing oracle.
pub fn seal_confirmation(
    manager: &RelaySecretManager,
    dialer_hello: &Hello,
    ticket: &ChallengeTicket,
) -> anyhow::Result<String> {
    let confirmation = Confirmation {
        domain: CONFIRMATION_DOMAIN.to_string(),
        dialer_hello: dialer_hello.clone(),
        server_hello: ticket.server_hello.clone(),
        nonce: ticket.nonce.clone(),
        epoch: ticket.epoch.clone(),
        transcript: canonical_confirmation(dialer_hello, &ticket.server_hello),
    };
    seal(manager, &confirmation)
}

/// Verify the confirmation of the server.
///
/// The dialer checks the confirmation of the actual upgrade response before
/// it admits the topology of the peer or sends a single business frame, so a
/// replayed challenge or a forged upgrade response is never good enough.
pub fn verify_confirmation(
    manager: &RelaySecretManager,
    local_hello: &Hello,
    ticket: &ChallengeTicket,
    confirmation: &str,
) -> anyhow::Result<()> {
    if confirmation.len() > MAX_AUTH_BYTES {
        anyhow::bail!("the peer confirmation is larger than {MAX_AUTH_BYTES} bytes");
    }
    let (confirmation, _envelope) = open::<Confirmation>(manager, confirmation)?;
    if confirmation.domain != CONFIRMATION_DOMAIN {
        anyhow::bail!("the peer confirmation has an unknown domain");
    }
    if confirmation.dialer_hello != *local_hello {
        anyhow::bail!("the peer confirmation belongs to another dialer");
    }
    if confirmation.server_hello != ticket.server_hello {
        anyhow::bail!("the peer confirmation belongs to another server");
    }
    if confirmation.nonce != ticket.nonce {
        anyhow::bail!("the peer confirmation belongs to another challenge");
    }
    if confirmation.epoch != ticket.epoch {
        anyhow::bail!("the peer confirmation belongs to another process of the peer");
    }
    let expected = canonical_confirmation(local_hello, &ticket.server_hello);
    if confirmation.transcript != expected {
        anyhow::bail!("the peer confirmation does not match this handshake");
    }
    Ok(())
}

/// Check the issue time and the expiry of a ticket.
///
/// A ticket lives at most the challenge lifetime, and it was not issued in
/// the future: an attacker cannot make a ticket live longer by moving its
/// timestamps around.
fn check_ticket_times(ticket: &Ticket, now: i64) -> anyhow::Result<()> {
    let skew = TICKET_CLOCK_SKEW.as_millis() as i64;
    if ticket.issued_at_ms > now + skew {
        anyhow::bail!("the peer ticket was issued in the future");
    }
    if ticket.expires_at_ms <= now {
        anyhow::bail!("the peer ticket expired");
    }
    let lifetime = ticket.expires_at_ms - ticket.issued_at_ms;
    if lifetime <= 0 || lifetime > CHALLENGE_TTL.as_millis() as i64 {
        anyhow::bail!("the peer ticket has an invalid lifetime");
    }
    Ok(())
}

/// Check that a hello of a peer is compatible with the hello of this node.
///
/// The two nodes run the same version and speak the same peer protocol. Two
/// database nodes never form a star, but two database-less nodes may be
/// linked: such a link carries the traffic of a node that has no route of
/// its own.
fn check_hello(remote: &Hello, local: &Hello) -> anyhow::Result<()> {
    if remote.node_id == local.node_id {
        anyhow::bail!("the peer claims the identity of this node");
    }
    if remote.runtime_version != local.runtime_version {
        anyhow::bail!(
            "the peer runs runtime version {} while this node runs {}; both nodes must run the \
             same version",
            remote.runtime_version,
            local.runtime_version
        );
    }
    if remote.protocol_version != local.protocol_version {
        anyhow::bail!(
            "the peer speaks peer protocol version {} while this node speaks {}",
            remote.protocol_version,
            local.protocol_version
        );
    }
    if remote.has_database && local.has_database {
        anyhow::bail!(
            "the peer cannot be linked to this node: two database nodes cannot form a star"
        );
    }
    Ok(())
}

/// Seal a payload under the master secret.
fn seal<T: Serialize>(manager: &RelaySecretManager, payload: &T) -> anyhow::Result<String> {
    let plaintext = serde_json::to_string(payload)
        .map_err(|_| anyhow::anyhow!("failed to encode the payload"))?;
    let envelope = manager
        .encrypt(&plaintext)
        .map_err(|_| anyhow::anyhow!("failed to seal the peer payload"))?;
    let encoded = serde_json::to_string(&envelope)
        .map_err(|_| anyhow::anyhow!("failed to encode the sealed payload"))?;
    if encoded.len() > MAX_AUTH_BYTES {
        anyhow::bail!("the sealed peer payload is larger than {MAX_AUTH_BYTES} bytes");
    }
    Ok(encoded)
}

/// Open a payload that was sealed under the master secret.
fn open<T: DeserializeOwned>(
    manager: &RelaySecretManager,
    encoded: &str,
) -> anyhow::Result<(T, EncryptedSecretEnvelope)> {
    let envelope: EncryptedSecretEnvelope = serde_json::from_str(encoded)
        .map_err(|_| anyhow::anyhow!("the sealed peer payload is malformed"))?;
    let plaintext = manager
        .decrypt(&envelope)
        .map_err(|_| anyhow::anyhow!("the sealed peer payload does not open"))?;
    let payload = serde_json::from_str(&plaintext)
        .map_err(|_| anyhow::anyhow!("the sealed peer payload is malformed"))?;
    Ok((payload, envelope))
}

#[cfg(test)]
mod tests {
    use super::{
        CHALLENGE_TTL, Challenge, MAX_AUTH_BYTES, PROOF_HEADER, ReplayCache, TICKET_HEADER,
        build_proof, check_ticket_times, issue_challenge, now_ms, seal_confirmation, startup_epoch,
        verify_confirmation, verify_peer,
    };
    use crate::relay_secrets::RelaySecretManager;
    use crate::worker::runtime::node::peer::handshake::Hello;
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use uuid::Uuid;

    fn manager(byte: u8) -> RelaySecretManager {
        RelaySecretManager::from_key_bytes([byte; 32])
    }

    fn hello(id: u128, has_database: bool) -> Hello {
        Hello::new(Uuid::from_u128(id), has_database, "1.2.3", 1)
    }

    #[test]
    fn two_nodes_with_the_same_master_authenticate_each_other() {
        let server = hello(1, true);
        let client = hello(2, false);
        let epoch = startup_epoch();
        let challenge =
            issue_challenge(&manager(7), server.clone(), &epoch, now_ms()).expect("challenge");
        let (proof, _server) =
            build_proof(&manager(7), &client, &challenge, now_ms()).expect("proof");
        let replay = ReplayCache::new();
        let verified = verify_peer(
            &manager(7),
            &server,
            &epoch,
            &challenge.ticket,
            &proof,
            now_ms(),
            &replay,
        )
        .expect("the peer is verified");
        assert_eq!(verified.hello.node_id, client.node_id);
        assert!(!verified.hello.has_database);
    }

    #[test]
    fn a_peer_with_another_master_secret_is_refused() {
        let server = hello(1, true);
        let client = hello(2, false);
        let epoch = startup_epoch();
        let challenge =
            issue_challenge(&manager(7), server.clone(), &epoch, now_ms()).expect("challenge");
        // The peer seals its proof with another master secret, so the node
        // cannot open it.
        let proof = super::seal(
            &manager(9),
            &super::Proof {
                hello: client,
                transcript: super::canonical_transcript(&hello(2, false), &server),
            },
        )
        .expect("proof");
        let replay = ReplayCache::new();
        assert!(
            verify_peer(
                &manager(7),
                &server,
                &epoch,
                &challenge.ticket,
                &proof,
                now_ms(),
                &replay
            )
            .is_err(),
            "a peer with another master secret cannot prove anything"
        );
    }

    #[test]
    fn a_replayed_ticket_is_refused() {
        let server = hello(1, true);
        let client = hello(2, false);
        let epoch = startup_epoch();
        let challenge =
            issue_challenge(&manager(7), server.clone(), &epoch, now_ms()).expect("challenge");
        let (proof, _server) =
            build_proof(&manager(7), &client, &challenge, now_ms()).expect("proof");
        let replay = ReplayCache::new();
        verify_peer(
            &manager(7),
            &server,
            &epoch,
            &challenge.ticket,
            &proof,
            now_ms(),
            &replay,
        )
        .expect("the first attempt is accepted");
        assert!(
            verify_peer(
                &manager(7),
                &server,
                &epoch,
                &challenge.ticket,
                &proof,
                now_ms(),
                &replay
            )
            .is_err(),
            "the same ticket cannot be used twice"
        );
    }

    #[test]
    fn a_ticket_of_another_process_is_refused() {
        let server = hello(1, true);
        let client = hello(2, false);
        let epoch = startup_epoch();
        let challenge =
            issue_challenge(&manager(7), server.clone(), &epoch, now_ms()).expect("challenge");
        let (proof, _server) =
            build_proof(&manager(7), &client, &challenge, now_ms()).expect("proof");
        let replay = ReplayCache::new();
        assert!(
            verify_peer(
                &manager(7),
                &server,
                &startup_epoch(),
                &challenge.ticket,
                &proof,
                now_ms(),
                &replay
            )
            .is_err(),
            "a ticket of a previous process of the peer is worthless"
        );
    }

    #[test]
    fn an_expired_ticket_is_refused() {
        let server = hello(1, true);
        let client = hello(2, false);
        let epoch = startup_epoch();
        let now = now_ms();
        let challenge =
            issue_challenge(&manager(7), server.clone(), &epoch, now).expect("challenge");
        let (proof, _server) = build_proof(&manager(7), &client, &challenge, now).expect("proof");
        let replay = ReplayCache::new();
        let later = now + CHALLENGE_TTL.as_millis() as i64 + 1;
        assert!(
            verify_peer(
                &manager(7),
                &server,
                &epoch,
                &challenge.ticket,
                &proof,
                later,
                &replay
            )
            .is_err(),
            "an expired ticket cannot open a link"
        );
    }

    #[test]
    fn a_reflected_hello_is_refused() {
        let server = hello(1, true);
        let epoch = startup_epoch();
        let challenge =
            issue_challenge(&manager(7), server.clone(), &epoch, now_ms()).expect("challenge");
        // The peer answers with the hello of the server itself.
        let proof = super::seal(
            &manager(7),
            &super::Proof {
                hello: server.clone(),
                transcript: super::canonical_transcript(&server, &server),
            },
        )
        .expect("proof");
        let replay = ReplayCache::new();
        assert!(
            verify_peer(
                &manager(7),
                &server,
                &epoch,
                &challenge.ticket,
                &proof,
                now_ms(),
                &replay
            )
            .is_err(),
            "a peer cannot claim the identity of this node"
        );
    }

    #[test]
    fn a_ticket_of_another_node_is_refused() {
        let server = hello(1, true);
        let other = hello(3, true);
        let client = hello(2, false);
        let epoch = startup_epoch();
        let challenge =
            issue_challenge(&manager(7), other.clone(), &epoch, now_ms()).expect("challenge");
        let (proof, _server) =
            build_proof(&manager(7), &client, &challenge, now_ms()).expect("proof");
        let replay = ReplayCache::new();
        assert!(
            verify_peer(
                &manager(7),
                &server,
                &epoch,
                &challenge.ticket,
                &proof,
                now_ms(),
                &replay
            )
            .is_err(),
            "a ticket that was not issued by this node is refused"
        );
    }

    #[test]
    fn a_peer_with_another_version_is_refused() {
        let server = hello(1, true);
        let client = Hello::new(Uuid::from_u128(2), false, "9.9.9", 1);
        let epoch = startup_epoch();
        let challenge =
            issue_challenge(&manager(7), server.clone(), &epoch, now_ms()).expect("challenge");
        let replay = ReplayCache::new();
        assert!(
            build_proof(&manager(7), &client, &challenge, now_ms()).is_err(),
            "a peer that runs another version cannot be linked"
        );
        let _ = replay;
    }

    #[test]
    fn a_peer_with_another_protocol_version_is_refused() {
        let server = hello(1, true);
        let client = Hello::new(Uuid::from_u128(2), false, "1.2.3", 2);
        let epoch = startup_epoch();
        let challenge =
            issue_challenge(&manager(7), server.clone(), &epoch, now_ms()).expect("challenge");
        assert!(
            build_proof(&manager(7), &client, &challenge, now_ms()).is_err(),
            "a peer that speaks another protocol cannot be linked"
        );
    }

    #[test]
    fn two_nodes_with_the_same_role_are_refused() {
        let server = hello(1, true);
        let client = hello(2, true);
        let epoch = startup_epoch();
        let challenge =
            issue_challenge(&manager(7), server.clone(), &epoch, now_ms()).expect("challenge");
        assert!(
            build_proof(&manager(7), &client, &challenge, now_ms()).is_err(),
            "two database nodes cannot form a star"
        );
    }

    #[test]
    fn the_sealed_payloads_stay_small() {
        let server = hello(1, true);
        let client = hello(2, false);
        let epoch = startup_epoch();
        let challenge =
            issue_challenge(&manager(7), server.clone(), &epoch, now_ms()).expect("challenge");
        let (proof, _server) =
            build_proof(&manager(7), &client, &challenge, now_ms()).expect("proof");
        assert!(challenge.ticket.len() <= MAX_AUTH_BYTES);
        assert!(proof.len() <= MAX_AUTH_BYTES);
    }

    #[test]
    fn the_master_secret_never_travels_in_the_payloads() {
        let server = hello(1, true);
        let client = hello(2, false);
        let epoch = startup_epoch();
        let challenge =
            issue_challenge(&manager(7), server.clone(), &epoch, now_ms()).expect("challenge");
        let (proof, _server) =
            build_proof(&manager(7), &client, &challenge, now_ms()).expect("proof");
        let secret = STANDARD.encode([7_u8; 32]);
        assert!(!challenge.ticket.contains(&secret));
        assert!(!proof.contains(&secret));
    }

    #[test]
    fn a_ticket_with_a_wrong_lifetime_is_refused() {
        let server = hello(1, true);
        let epoch = startup_epoch();
        let now = now_ms();
        let ticket = super::Ticket {
            hello: server,
            epoch: epoch.clone(),
            nonce: "nonce".to_string(),
            issued_at_ms: now,
            expires_at_ms: now + CHALLENGE_TTL.as_millis() as i64 * 10,
        };
        assert!(
            check_ticket_times(&ticket, now).is_err(),
            "a ticket cannot claim to live longer than the challenge lifetime"
        );
        let future = super::Ticket {
            hello: hello(1, true),
            epoch: epoch.clone(),
            nonce: "nonce".to_string(),
            issued_at_ms: now + 60_000,
            expires_at_ms: now + 120_000,
        };
        assert!(
            check_ticket_times(&future, now).is_err(),
            "a ticket cannot be issued in the future"
        );
    }

    #[test]
    fn a_challenge_that_does_not_match_its_ticket_is_refused() {
        let server = hello(1, true);
        let client = hello(2, false);
        let epoch = startup_epoch();
        let challenge =
            issue_challenge(&manager(7), server.clone(), &epoch, now_ms()).expect("challenge");
        // The server announces one identity and seals another one.
        let forged = Challenge {
            server_hello: hello(9, true),
            ticket: challenge.ticket.clone(),
        };
        assert!(
            build_proof(&manager(7), &client, &forged, now_ms()).is_err(),
            "the announced identity of the server must match its ticket"
        );
    }

    #[test]
    fn the_confirmation_of_the_current_server_opens_the_link() {
        let server = hello(1, true);
        let dialer = hello(2, false);
        let epoch = startup_epoch();
        let challenge =
            issue_challenge(&manager(7), server.clone(), &epoch, now_ms()).expect("challenge");
        let (proof, ticket) =
            build_proof(&manager(7), &dialer, &challenge, now_ms()).expect("proof");
        let verified = verify_peer(
            &manager(7),
            &server,
            &epoch,
            &challenge.ticket,
            &proof,
            now_ms(),
            &ReplayCache::new(),
        )
        .expect("the peer is verified");
        let confirmation =
            seal_confirmation(&manager(7), &verified.hello, &verified.ticket).expect("seal");
        verify_confirmation(&manager(7), &dialer, &ticket, &confirmation)
            .expect("the confirmation of the current server opens the link");
    }

    #[test]
    fn a_replayed_challenge_is_not_a_proof_of_the_current_server() {
        let server = hello(1, true);
        let dialer = hello(2, false);
        let epoch = startup_epoch();
        let challenge =
            issue_challenge(&manager(7), server.clone(), &epoch, now_ms()).expect("challenge");
        let (proof, ticket) =
            build_proof(&manager(7), &dialer, &challenge, now_ms()).expect("proof");
        let verified = verify_peer(
            &manager(7),
            &server,
            &epoch,
            &challenge.ticket,
            &proof,
            now_ms(),
            &ReplayCache::new(),
        )
        .expect("the peer is verified");
        // An attacker that only replays the public challenge cannot seal a
        // confirmation: it does not own the master secret.
        let forged = super::seal(
            &manager(9),
            &super::Confirmation {
                domain: super::CONFIRMATION_DOMAIN.to_string(),
                dialer_hello: verified.hello.clone(),
                server_hello: ticket.server_hello.clone(),
                nonce: ticket.nonce.clone(),
                epoch: ticket.epoch.clone(),
                transcript: super::canonical_confirmation(&verified.hello, &ticket.server_hello),
            },
        )
        .expect("seal");
        assert!(
            verify_confirmation(&manager(7), &dialer, &ticket, &forged).is_err(),
            "a replayed challenge cannot prove the current server"
        );
    }

    #[test]
    fn a_confirmation_for_another_dialer_is_refused() {
        let server = hello(1, true);
        let dialer = hello(2, false);
        let other = hello(3, false);
        let epoch = startup_epoch();
        let challenge =
            issue_challenge(&manager(7), server.clone(), &epoch, now_ms()).expect("challenge");
        let (proof, ticket) =
            build_proof(&manager(7), &dialer, &challenge, now_ms()).expect("proof");
        let verified = verify_peer(
            &manager(7),
            &server,
            &epoch,
            &challenge.ticket,
            &proof,
            now_ms(),
            &ReplayCache::new(),
        )
        .expect("the peer is verified");
        let confirmation =
            seal_confirmation(&manager(7), &verified.hello, &verified.ticket).expect("seal");
        assert!(
            verify_confirmation(&manager(7), &other, &ticket, &confirmation).is_err(),
            "the confirmation of another dialer is refused"
        );
    }

    #[test]
    fn a_confirmation_for_another_challenge_is_refused() {
        let server = hello(1, true);
        let dialer = hello(2, false);
        let epoch = startup_epoch();
        let first =
            issue_challenge(&manager(7), server.clone(), &epoch, now_ms()).expect("challenge");
        let second =
            issue_challenge(&manager(7), server.clone(), &epoch, now_ms()).expect("challenge");
        let (proof, _ticket) = build_proof(&manager(7), &dialer, &first, now_ms()).expect("proof");
        let verified = verify_peer(
            &manager(7),
            &server,
            &epoch,
            &first.ticket,
            &proof,
            now_ms(),
            &ReplayCache::new(),
        )
        .expect("the peer is verified");
        let confirmation =
            seal_confirmation(&manager(7), &verified.hello, &verified.ticket).expect("seal");
        let (_, other_ticket) =
            build_proof(&manager(7), &dialer, &second, now_ms()).expect("proof");
        assert!(
            verify_confirmation(&manager(7), &dialer, &other_ticket, &confirmation).is_err(),
            "the confirmation of another challenge is refused"
        );
    }

    #[test]
    fn a_client_proof_is_never_a_confirmation() {
        let server = hello(1, true);
        let dialer = hello(2, false);
        let epoch = startup_epoch();
        let challenge =
            issue_challenge(&manager(7), server.clone(), &epoch, now_ms()).expect("challenge");
        let (proof, ticket) =
            build_proof(&manager(7), &dialer, &challenge, now_ms()).expect("proof");
        assert!(
            verify_confirmation(&manager(7), &dialer, &ticket, &proof).is_err(),
            "the proof of the dialer is not a confirmation of the server"
        );
    }

    #[test]
    fn a_confirmation_of_another_server_identity_is_refused() {
        let server = hello(1, true);
        let other_server = hello(9, true);
        let dialer = hello(2, false);
        let epoch = startup_epoch();
        let challenge =
            issue_challenge(&manager(7), server.clone(), &epoch, now_ms()).expect("challenge");
        let (proof, ticket) =
            build_proof(&manager(7), &dialer, &challenge, now_ms()).expect("proof");
        let verified = verify_peer(
            &manager(7),
            &server,
            &epoch,
            &challenge.ticket,
            &proof,
            now_ms(),
            &ReplayCache::new(),
        )
        .expect("the peer is verified");
        // The confirmation announces another identity of the server.
        let forged = super::seal(
            &manager(7),
            &super::Confirmation {
                domain: super::CONFIRMATION_DOMAIN.to_string(),
                dialer_hello: verified.hello.clone(),
                server_hello: other_server.clone(),
                nonce: ticket.nonce.clone(),
                epoch: ticket.epoch.clone(),
                transcript: super::canonical_confirmation(&verified.hello, &other_server),
            },
        )
        .expect("seal");
        assert!(
            verify_confirmation(&manager(7), &dialer, &ticket, &forged).is_err(),
            "the confirmation of another server identity is refused"
        );
    }

    #[test]
    fn the_replay_cache_refuses_a_full_cache() {
        let cache = ReplayCache::new();
        let expiry = now_ms() + 60_000;
        for index in 0..super::REPLAY_CACHE_CAPACITY {
            cache
                .consume(&format!("nonce-{index}"), expiry)
                .expect("the nonce is accepted");
        }
        assert!(
            cache.consume("one-too-many", expiry).is_err(),
            "a full cache refuses a ticket instead of evicting a live one"
        );
    }

    #[test]
    fn the_headers_carry_no_secret() {
        assert!(TICKET_HEADER.starts_with("x-"));
        assert!(PROOF_HEADER.starts_with("x-"));
    }
}
