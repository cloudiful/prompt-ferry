use crate::naming::BRIDGE_FRAME_PREFIX;
use anyhow::anyhow;
use rand::Rng;

use super::{ALG, Direction, FRAME_NONCE_BYTES, FRAME_VERSION, VERSION};

pub(super) fn random_frame_nonce() -> [u8; FRAME_NONCE_BYTES] {
    let mut nonce = [0_u8; FRAME_NONCE_BYTES];
    rand::rng().fill_bytes(&mut nonce);
    nonce
}

pub(super) fn encode_encrypted_frame(
    seq: u64,
    nonce: &[u8; FRAME_NONCE_BYTES],
    ciphertext: &[u8],
) -> Vec<u8> {
    let mut frame =
        Vec::with_capacity(1 + std::mem::size_of::<u64>() + FRAME_NONCE_BYTES + ciphertext.len());
    frame.push(FRAME_VERSION);
    frame.extend_from_slice(&seq.to_be_bytes());
    frame.extend_from_slice(nonce);
    frame.extend_from_slice(ciphertext);
    frame
}

pub(super) fn decode_encrypted_frame(
    bytes: &[u8],
) -> anyhow::Result<(u64, [u8; FRAME_NONCE_BYTES], Vec<u8>)> {
    const HEADER_BYTES: usize = 1 + std::mem::size_of::<u64>() + FRAME_NONCE_BYTES;
    if bytes.len() < HEADER_BYTES {
        return Err(anyhow!("encrypted bridge frame too short"));
    }
    let version = bytes[0];
    if version != FRAME_VERSION {
        return Err(anyhow!(
            "unsupported encrypted bridge frame version {version}, expected {FRAME_VERSION}"
        ));
    }
    let seq = u64::from_be_bytes(
        bytes[1..1 + std::mem::size_of::<u64>()]
            .try_into()
            .expect("u64 seq slice length"),
    );
    let nonce = bytes[1 + std::mem::size_of::<u64>()..HEADER_BYTES]
        .try_into()
        .expect("nonce slice length");
    let ciphertext = bytes[HEADER_BYTES..].to_vec();
    Ok((seq, nonce, ciphertext))
}

pub(super) fn aad(direction: Direction, seq: u64) -> Vec<u8> {
    format!(
        "{BRIDGE_FRAME_PREFIX}:v{VERSION}:{ALG}:{}:{seq}",
        direction.as_str()
    )
    .into_bytes()
}
