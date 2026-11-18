//! The identity a node proves to its peers.
//!
//! Two nodes that want to talk to each other prove that they share the same
//! master secret without ever putting the secret on the wire. The proof
//! happens during the HTTP exchange that precedes the websocket upgrade, so
//! no frame of an unauthenticated peer is ever read.
//!
//! Each side sends a hello with a fresh random challenge, and the proof
//! seals a canonical transcript of the two hellos with the master secret. The
//! transcript is directional, so a proof that is reflected back at its sender
//! does not verify, and a fresh challenge makes a replayed proof useless.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use rand::Rng;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// How many unauthenticated handshakes a node accepts at the same time.
pub const PREAUTH_CONCURRENCY: usize = 32;

/// The version tag of the proof transcript.
pub const PROOF_TRANSCRIPT_VERSION: &str = "pf64-peer-proof-v1";

/// The version tag of the confirmation transcript.
///
/// The confirmation of the server has its own domain: it is a different
/// direction of the same exchange, so a proof of a peer can never be read as
/// a confirmation of the server.
pub const CONFIRMATION_TRANSCRIPT_VERSION: &str = "pf64-peer-confirmation-v1";

/// The number of random bytes of a hello challenge.
const CHALLENGE_BYTES: usize = 32;

/// The hello of a node: who it is, what it can do and a fresh challenge.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    /// The persistent identity of the node.
    pub node_id: Uuid,
    /// Whether the node is the database node of its star.
    pub has_database: bool,
    /// The compiled runtime version of the node.
    pub runtime_version: String,
    /// The peer protocol version of the node.
    pub protocol_version: u32,
    /// The fresh random challenge of this handshake, base64 encoded.
    pub challenge: String,
}

impl Hello {
    /// Build a hello with a fresh random challenge.
    pub fn new(
        node_id: Uuid,
        has_database: bool,
        runtime_version: impl Into<String>,
        protocol_version: u32,
    ) -> Self {
        let mut challenge = [0_u8; CHALLENGE_BYTES];
        rand::rng().fill_bytes(&mut challenge);
        Self {
            node_id,
            has_database,
            runtime_version: runtime_version.into(),
            protocol_version,
            challenge: STANDARD.encode(challenge),
        }
    }
}

/// The canonical transcript of two hellos.
///
/// The transcript is directional: the sender comes first and the receiver
/// second, so the transcript a sender proves is never the transcript its
/// peer expects.
pub fn canonical_transcript(sender: &Hello, receiver: &Hello) -> String {
    let mut transcript = String::new();
    transcript.push_str(PROOF_TRANSCRIPT_VERSION);
    transcript.push('\n');
    transcript.push_str(&format!("sender={}\n", sender.node_id));
    transcript.push_str(&format!(
        "sender_has_database={}\n",
        u8::from(sender.has_database)
    ));
    transcript.push_str(&format!(
        "sender_runtime_version={}\n",
        sender.runtime_version
    ));
    transcript.push_str(&format!(
        "sender_protocol_version={}\n",
        sender.protocol_version
    ));
    transcript.push_str(&format!("sender_challenge={}\n", sender.challenge));
    transcript.push_str(&format!("receiver={}\n", receiver.node_id));
    transcript.push_str(&format!(
        "receiver_has_database={}\n",
        u8::from(receiver.has_database)
    ));
    transcript.push_str(&format!(
        "receiver_runtime_version={}\n",
        receiver.runtime_version
    ));
    transcript.push_str(&format!(
        "receiver_protocol_version={}\n",
        receiver.protocol_version
    ));
    transcript.push_str(&format!("receiver_challenge={}\n", receiver.challenge));
    transcript
}

/// The canonical transcript of the confirmation of the server.
///
/// The confirmation binds the fresh hello of the dialer to the hello of the
/// server. It has its own domain, so it is never equal to the transcript of
/// the proof of the dialer, and it is directional in the same way.
pub fn canonical_confirmation(dialer: &Hello, server: &Hello) -> String {
    let mut transcript = String::new();
    transcript.push_str(CONFIRMATION_TRANSCRIPT_VERSION);
    transcript.push('\n');
    transcript.push_str(&format!("dialer={}\n", dialer.node_id));
    transcript.push_str(&format!(
        "dialer_has_database={}\n",
        u8::from(dialer.has_database)
    ));
    transcript.push_str(&format!(
        "dialer_runtime_version={}\n",
        dialer.runtime_version
    ));
    transcript.push_str(&format!(
        "dialer_protocol_version={}\n",
        dialer.protocol_version
    ));
    transcript.push_str(&format!("dialer_challenge={}\n", dialer.challenge));
    transcript.push_str(&format!("server={}\n", server.node_id));
    transcript.push_str(&format!(
        "server_has_database={}\n",
        u8::from(server.has_database)
    ));
    transcript.push_str(&format!(
        "server_runtime_version={}\n",
        server.runtime_version
    ));
    transcript.push_str(&format!(
        "server_protocol_version={}\n",
        server.protocol_version
    ));
    transcript.push_str(&format!("server_challenge={}\n", server.challenge));
    transcript
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::{Hello, canonical_confirmation, canonical_transcript};

    fn hello(id: u128, has_database: bool) -> Hello {
        Hello::new(Uuid::from_u128(id), has_database, "1.2.3", 1)
    }

    #[test]
    fn the_transcript_is_directional() {
        let sender = hello(1, true);
        let receiver = hello(2, false);
        assert_ne!(
            canonical_transcript(&sender, &receiver),
            canonical_transcript(&receiver, &sender)
        );
    }

    #[test]
    fn the_confirmation_has_its_own_domain() {
        let dialer = hello(1, false);
        let server = hello(2, true);
        assert_ne!(
            canonical_confirmation(&dialer, &server),
            canonical_transcript(&dialer, &server),
            "the confirmation of the server is never the proof of the dialer"
        );
        assert_ne!(
            canonical_confirmation(&dialer, &server),
            canonical_confirmation(&server, &dialer),
            "the confirmation is directional"
        );
    }

    #[test]
    fn a_hello_carries_the_identity_and_a_fresh_challenge() {
        let first = hello(1, true);
        let second = hello(1, true);
        assert_ne!(first.challenge, second.challenge);
        assert_eq!(first.node_id, Uuid::from_u128(1));
        assert!(first.has_database);
    }
}
