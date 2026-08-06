use crate::{bridge_wire, protocol::BridgeMessage};
use anyhow::anyhow;
use chacha20poly1305::{
    ChaCha20Poly1305, KeyInit,
    aead::{Aead, Payload},
};
use hkdf::Hkdf;
use sha2::Sha256;

mod frame;
mod handshake;

pub use handshake::{
    decode_hello, decode_ready, encode_hello, encode_ready, random_handshake_nonce,
    validate_settings,
};

#[cfg(test)]
mod tests;

pub(crate) const ALG: &str = "chacha20poly1305_hkdf_sha256_v1";
pub(crate) const VERSION: u8 = 1;
/// Encrypted frame envelope version, fixed independently of the wire payload
/// schema: existing encrypted deployments validate this byte, so it stays 3
/// while payload schemas evolve via [`bridge_wire::BRIDGE_WIRE_VERSION`].
pub(crate) const FRAME_VERSION: u8 = 3;
pub(crate) const KEY_BYTES: usize = 32;
pub(crate) const HANDSHAKE_NONCE_BYTES: usize = 32;
pub(crate) const FRAME_NONCE_BYTES: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    WorkerToRelay,
    RelayToWorker,
}

impl Direction {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::WorkerToRelay => "worker_to_relay",
            Self::RelayToWorker => "relay_to_worker",
        }
    }
}

pub struct BridgeCipher {
    cipher: ChaCha20Poly1305,
    send_direction: Direction,
    recv_direction: Direction,
    next_send_seq: u64,
    next_recv_seq: u64,
}

impl BridgeCipher {
    pub fn new(
        key: &str,
        worker_nonce: &[u8; HANDSHAKE_NONCE_BYTES],
        relay_nonce: &[u8; HANDSHAKE_NONCE_BYTES],
        send_direction: Direction,
        recv_direction: Direction,
    ) -> anyhow::Result<Self> {
        let key = handshake::parse_key(key)?;
        let mut salt = Vec::with_capacity(HANDSHAKE_NONCE_BYTES * 2);
        salt.extend_from_slice(worker_nonce);
        salt.extend_from_slice(relay_nonce);

        let hk = Hkdf::<Sha256>::new(Some(&salt), &key);
        let mut session_key = [0_u8; KEY_BYTES];
        hk.expand(crate::naming::HKDF_INFO, &mut session_key)
            .map_err(|_| anyhow!("failed to derive bridge encryption key"))?;

        Ok(Self {
            cipher: ChaCha20Poly1305::new(&session_key.into()),
            send_direction,
            recv_direction,
            next_send_seq: 1,
            next_recv_seq: 1,
        })
    }

    /// Encode the message on the wire and encrypt it into a versioned frame.
    pub fn encrypt_message(&mut self, message: &BridgeMessage) -> anyhow::Result<Vec<u8>> {
        let seq = self.next_send_seq;
        self.next_send_seq = self
            .next_send_seq
            .checked_add(1)
            .ok_or_else(|| anyhow!("bridge encryption send sequence overflow"))?;

        let plaintext = bridge_wire::encode_message(message)?;
        let nonce = frame::random_frame_nonce();
        let aad = frame::aad(self.send_direction, seq);
        let ciphertext = self
            .cipher
            .encrypt(
                (&nonce).into(),
                Payload {
                    msg: &plaintext,
                    aad: &aad,
                },
            )
            .map_err(|_| anyhow!("failed to encrypt bridge message (seq {seq})"))?;

        Ok(frame::encode_encrypted_frame(seq, &nonce, &ciphertext))
    }

    /// Decrypt a frame and decode the plaintext wire message.
    pub fn decrypt_message(&mut self, bytes: &[u8]) -> anyhow::Result<BridgeMessage> {
        let (seq, nonce, ciphertext) = frame::decode_encrypted_frame(bytes)?;
        if seq != self.next_recv_seq {
            return Err(anyhow!(
                "unexpected bridge encryption sequence: got {}, expected {}",
                seq,
                self.next_recv_seq
            ));
        }

        let aad = frame::aad(self.recv_direction, seq);
        let plaintext = self
            .cipher
            .decrypt(
                (&nonce).into(),
                Payload {
                    msg: &ciphertext,
                    aad: &aad,
                },
            )
            .map_err(|_| {
                anyhow!(
                    "failed to decrypt bridge message (seq {seq}, frame {} bytes)",
                    bytes.len()
                )
            })?;
        self.next_recv_seq = self
            .next_recv_seq
            .checked_add(1)
            .ok_or_else(|| anyhow!("bridge encryption receive sequence overflow"))?;

        bridge_wire::decode_message(&plaintext)
    }
}
