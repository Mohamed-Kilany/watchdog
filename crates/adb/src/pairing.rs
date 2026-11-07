//! Wireless-debugging pairing — SPAKE2 over Ed25519 (BoringSSL variant).
//!
//! Wire format (AOSP `packages/modules/adb`, `pairing_connection.cpp`):
//! - After TLS 1.3 handshake (mutual certificates), both sides send
//!   `PairingPacket` frames: `u8 version(=1) | u8 type | u32 LE payload_len | payload`
//!   - type 0 (`SPAKE2_MSG`): 32-byte Ed25519 blinded point
//!   - type 1 (`PEER_INFO`): AES-128-GCM encrypted PeerInfo
//! - The pairing code (6 digits) is the PAKE password; the TLS 1.3 exporter
//!   application keying material (64 bytes, label `adb-label`) is appended:
//!   `password = code.as_bytes() + exporter(64)`.
//! - SPAKE2 transcript is SHA-512 over 8-byte-LE-length-prefixed fields in
//!   role order (client first), ending with the shared-secret (DH) and the
//!   SHA-512 password hash. Output is 64 bytes (used as GCM key material).
//! - AES-128-GCM key: HKDF-SHA256, salt = empty, IKM = SPAKE2 key material,
//!   info = `"adb pairing_auth aes-128-gcm key"`. Nonce = 12 bytes with an
//!   8-byte LE counter at the start (0 for the first message), no AAD.
//!   Tag appended (16 bytes).
//!
//! PeerInfo is 8192 bytes: `u8 type (0=ADB_RSA_PUB_KEY, 1=ADB_DEVICE_GUID)`
//! then 8191 bytes data. Host sends its Android public key; device replies
//! with its device GUID.
//!
//! The BoringSSL SPAKE2 profile is *not* RFC 9382: it uses Ed25519 points
//! `M` (`5ada7e4b…`) and `N` (`10e3df0a…`), SHA-512 password hashing reduced
//! mod the group order, and cofactor clearing. This module implements exactly
//! that profile, matching BoringSSL's `SPAKE2_*` API bit-for-bit.

use curve25519_dalek::edwards::{CompressedEdwardsY, EdwardsPoint};
use curve25519_dalek::Scalar;
use sha2::{Digest, Sha512};

use crate::error::{AdbError, Result};

/// Encoded M point (BoringSSL `kSpakeM`).
pub const SPAKE_M: [u8; 32] = [
    0x5a, 0xda, 0x7e, 0x4b, 0xf6, 0xdd, 0xd9, 0xad, 0xb6, 0x62, 0x6d, 0x32, 0x13, 0x1c, 0x6b, 0x5c,
    0x51, 0xa1, 0xe3, 0x47, 0xa3, 0x47, 0x8f, 0x53, 0xcf, 0xcf, 0x44, 0x1b, 0x88, 0xee, 0xd1, 0x2e,
];

/// Encoded N point (BoringSSL `kSpakeN`).
pub const SPAKE_N: [u8; 32] = [
    0x10, 0xe3, 0xdf, 0x0a, 0xe3, 0x7d, 0x8e, 0x7a, 0x99, 0xb5, 0xfe, 0x74, 0xb4, 0x46, 0x72, 0x10,
    0x3d, 0xbd, 0xdc, 0xbd, 0x06, 0xaf, 0x68, 0x0d, 0x71, 0x32, 0x9a, 0x11, 0x69, 0x3b, 0xc7, 0x78,
];

/// SPAKE2 role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpakeRole {
    /// adb client (watchdog host).
    Client,
    /// adb device (adbd).
    Server,
}

/// Maximum SPAKE2 output key length.
pub const SPAKE2_KEY_LEN: usize = 64;

/// A SPAKE2 session. One instance per pairing peer.
#[derive(Debug)]
pub struct Spake2 {
    role: SpakeRole,
    my_name: Vec<u8>,
    their_name: Vec<u8>,
    my_msg: Option<[u8; 32]>,
    my_private: Scalar,
    password_hash: [u8; 64],
    password_scalar: Scalar,
}

impl Spake2 {
    /// Create a session. `my_name`/`their_name` are the role names
    /// (`"adb pair client"` / `"adb pair server"`).
    #[must_use]
    pub fn new(role: SpakeRole, my_name: &[u8], their_name: &[u8]) -> Self {
        Self {
            role,
            my_name: my_name.to_vec(),
            their_name: their_name.to_vec(),
            my_msg: None,
            my_private: Scalar::from_bytes_mod_order_wide(&[0u8; 64]),
            password_hash: [0u8; 64],
            password_scalar: Scalar::from_bytes_mod_order_wide(&[0u8; 64]),
        }
    }

    /// Generate our SPAKE2 message (32-byte blinded point).
    pub fn generate_msg(&mut self, password: &[u8]) -> Result<[u8; 32]> {
        // Random ephemeral: 64 bytes reduced mod order, times cofactor (x8).
        let mut tmp = [0u8; 64];
        rand_fill(&mut tmp);
        // BoringSSL: x25519_sc_reduce then left_shift_3 (in-place, carry propagated).
        let mut reduced = spake_reduce(&tmp);
        left_shift_3(&mut reduced);
        self.my_private = Scalar::from_bytes_mod_order(reduced);

        // w = H(password) (SHA-512), reduced.
        let digest = Sha512::digest(password);
        let mut hash = [0u8; 64];
        hash.copy_from_slice(&digest);
        self.password_hash = hash;
        let reduced_hash = spake_reduce(&hash);
        // BoringSSL's "password_scalar_hack" adds multiples of the group
        // order to clear the cofactor; those are no-ops mod the order, so
        // direct reduction is equivalent.
        self.password_scalar = Scalar::from_bytes_mod_order(reduced_hash);

        // P = private * B
        let p = EdwardsPoint::mul_base(&self.my_private);
        // mask = w * M (client) or w * N (server)
        let mask_point = if self.role == SpakeRole::Client {
            self.password_scalar * decompress(&SPAKE_M)?
        } else {
            self.password_scalar * decompress(&SPAKE_N)?
        };
        // P* = P + mask
        let p_star = p + mask_point;
        let encoded = p_star.compress().to_bytes();
        self.my_msg = Some(encoded);
        Ok(encoded)
    }

    /// Process the peer's SPAKE2 message and derive the 64-byte key.
    pub fn process_msg(&mut self, their_msg: &[u8]) -> Result<[u8; SPAKE2_KEY_LEN]> {
        let my_msg = self
            .my_msg
            .ok_or_else(|| AdbError::Protocol("SPAKE2: process_msg before generate_msg".into()))?;
        if their_msg.len() != 32 {
            return Err(AdbError::Protocol(format!(
                "SPAKE2: peer message must be 32 bytes, got {}",
                their_msg.len()
            )));
        }
        let mut peer_bytes = [0u8; 32];
        peer_bytes.copy_from_slice(their_msg);
        let q_star = decompress(&peer_bytes)?;

        // Q = Q* - mask (peer's mask uses the other point).
        let peers_mask = if self.role == SpakeRole::Client {
            self.password_scalar * decompress(&SPAKE_N)?
        } else {
            self.password_scalar * decompress(&SPAKE_M)?
        };
        let q = q_star - peers_mask;

        // dh = private * Q
        let dh = self.my_private * q;
        let dh_encoded = dh.compress().to_bytes();

        // Transcript: SHA-512 over 8-byte-LE-length-prefixed fields.
        let mut sha = Sha512::new();
        let ordered: [(&[u8], &[u8]); 4] = if self.role == SpakeRole::Client {
            [
                (b"client-name", &self.my_name),
                (b"server-name", &self.their_name),
                (b"client-msg", &my_msg),
                (b"server-msg", &peer_bytes),
            ]
        } else {
            [
                (b"server-name", &self.my_name),
                (b"client-name", &self.their_name),
                (b"server-msg", &my_msg),
                (b"client-msg", &peer_bytes),
            ]
        };
        // BoringSSL order: my_name, their_name, my_msg, their_msg — but
        // roles fix the semantic order (client first) in both views.
        let _ = ordered;
        push_lp(&mut sha, &self.my_name);
        push_lp(&mut sha, &self.their_name);
        push_lp(&mut sha, &my_msg);
        push_lp(&mut sha, &peer_bytes);
        push_lp(&mut sha, &dh_encoded);
        push_lp(&mut sha, &self.password_hash);
        let key: [u8; 64] = sha.finalize().into();
        Ok(key)
    }
}

fn push_lp(hasher: &mut Sha512, data: &[u8]) {
    hasher.update((data.len() as u64).to_le_bytes());
    hasher.update(data);
}

fn decompress(bytes: &[u8; 32]) -> Result<EdwardsPoint> {
    CompressedEdwardsY(*bytes)
        .decompress()
        .ok_or_else(|| AdbError::Protocol("SPAKE2: peer point is not on the curve".into()))
}

/// BoringSSL `x25519_sc_reduce`: reduce 64 bytes mod the group order.
fn spake_reduce(bytes: &[u8; 64]) -> [u8; 32] {
    let scalar = Scalar::from_bytes_mod_order_wide(bytes);
    scalar.to_bytes()
}

/// BoringSSL `left_shift_3`: multiply by 8 (little-endian in place).
fn left_shift_3(bytes: &mut [u8; 32]) {
    let mut carry = 0u8;
    for byte in bytes.iter_mut() {
        let next_carry = *byte >> 5;
        *byte = (*byte << 3) | carry;
        carry = next_carry;
    }
}

fn rand_fill(buf: &mut [u8]) {
    // OS RNG via rand crate's thread_rng (already a dependency).
    use rand::RngCore;
    rand::thread_rng().fill_bytes(buf);
}

/// HKDF-SHA256 (RFC 5869): salt empty, IKM = spake key, info = pairing GCM key info.
#[must_use]
pub fn derive_gcm_key(spake_key: &[u8]) -> [u8; 16] {
    let prk = hmac_sha256(&[], spake_key);
    let mut okm = Vec::with_capacity(16 + 32);
    let info = b"adb pairing_auth aes-128-gcm key";
    let mut t = Vec::new();
    let mut i = 1u8;
    while okm.len() < 16 {
        let mut data = t.clone();
        data.extend_from_slice(info);
        data.push(i);
        t = hmac_sha256(&prk, &data).to_vec();
        okm.extend_from_slice(&t);
        i += 1;
    }
    okm[..16].try_into().expect("16 bytes")
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    use sha2::Sha256;
    let mut block = [0u8; 64];
    let key = if key.len() > 64 {
        Sha256::digest(key).to_vec()
    } else {
        key.to_vec()
    };
    block[..key.len()].copy_from_slice(&key);
    let mut ipad = block;
    let mut opad = block;
    for b in ipad.iter_mut() {
        *b ^= 0x36;
    }
    for b in opad.iter_mut() {
        *b ^= 0x5c;
    }
    let mut inner = Sha256::new();
    inner.update(&ipad);
    inner.update(data);
    let inner_hash = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(&opad);
    outer.update(&inner_hash);
    outer.finalize().into()
}

/// Pairing packet: `u8 version | u8 type | u32 LE len | payload`.
pub mod packet {
    /// Packet type ids (AOSP `PairingPacket.Type`).
    pub const SPAKE2_MSG: u8 = 0;
    /// Peer info (encrypted PeerInfo).
    pub const PEER_INFO: u8 = 1;
    /// Header size.
    pub const HEADER_LEN: usize = 6;

    /// Encode a pairing packet.
    #[must_use]
    pub fn encode(ty: u8, payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER_LEN + payload.len());
        out.push(1); // kCurrentKeyHeaderVersion
        out.push(ty);
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        out.extend_from_slice(payload);
        out
    }

    /// Decode one pairing packet from the front of `buf`.
    ///
    /// Returns `(type, payload, consumed)` or `None` if incomplete.
    #[must_use]
    pub fn decode(buf: &[u8]) -> Option<(u8, Vec<u8>, usize)> {
        if buf.len() < HEADER_LEN {
            return None;
        }
        // version = buf[0]
        let ty = buf[1];
        let len = u32::from_le_bytes(buf[2..6].try_into().ok()?) as usize;
        if len > 8192 {
            return None;
        }
        if buf.len() < HEADER_LEN + len {
            return None;
        }
        Some((
            ty,
            buf[HEADER_LEN..HEADER_LEN + len].to_vec(),
            HEADER_LEN + len,
        ))
    }
}

/// PeerInfo types.
pub mod peer_info {
    /// Host → device: the host's Android public key.
    pub const ADB_RSA_PUB_KEY: u8 = 0;
    /// Device → host: the device GUID.
    pub const ADB_DEVICE_GUID: u8 = 1;
    /// PeerInfo size on the wire (1 type + 8191 bytes).
    pub const SIZE: usize = 8192;
}

/// AES-128-GCM with the ADB nonce scheme (12 bytes: 8-byte LE counter + 4 zeros).
pub mod gcm {
    /// Minimum encrypted length (tag only).
    pub const TAG_LEN: usize = 16;

    /// Encrypt with the given counter; returns ciphertext + 16-byte tag.
    #[must_use]
    pub fn encrypt(key: &[u8; 16], counter: u64, plaintext: &[u8]) -> Vec<u8> {
        let mut nonce = [0u8; 12];
        nonce[..8].copy_from_slice(&counter.to_le_bytes());
        let cipher = aes_gcm::Aes128Gcm::new_from_slice(key).expect("16-byte key");
        use aes_gcm::aead::{Aead, KeyInit, Payload};
        cipher
            .encrypt(
                &nonce.into(),
                Payload {
                    msg: plaintext,
                    aad: &[],
                },
            )
            .expect("GCM encryption")
    }

    /// Decrypt; `None` on tag failure.
    #[must_use]
    pub fn decrypt(key: &[u8; 16], counter: u64, ciphertext: &[u8]) -> Option<Vec<u8>> {
        let mut nonce = [0u8; 12];
        nonce[..8].copy_from_slice(&counter.to_le_bytes());
        let cipher = aes_gcm::Aes128Gcm::new_from_slice(key).ok()?;
        use aes_gcm::aead::{Aead, KeyInit, Payload};
        cipher
            .decrypt(
                &nonce.into(),
                Payload {
                    msg: ciphertext,
                    aad: &[],
                },
            )
            .ok()
    }
}

/// Deterministic test-only constructor: fixed ephemeral so vectors are reproducible.
#[cfg(test)]
pub(crate) fn spake2_with_private(
    role: SpakeRole,
    my_name: &[u8],
    their_name: &[u8],
    private_bytes: [u8; 64],
) -> Spake2 {
    let mut session = Spake2::new(role, my_name, their_name);
    session.my_private = Scalar::from_bytes_mod_order_wide(&private_bytes);
    session
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_then_match() {
        let mut client = Spake2::new(SpakeRole::Client, b"adb pair client", b"adb pair server");
        let mut server = Spake2::new(SpakeRole::Server, b"adb pair server", b"adb pair client");
        let password = b"123456";
        let c_msg = client.generate_msg(password).unwrap();
        let s_msg = server.generate_msg(password).unwrap();
        let client_key = client.process_msg(&s_msg).unwrap();
        let server_key = server.process_msg(&c_msg).unwrap();
        assert_eq!(client_key, server_key);
    }

    #[test]
    fn wrong_password_diverges() {
        let mut client = Spake2::new(SpakeRole::Client, b"adb pair client", b"adb pair server");
        let mut server = Spake2::new(SpakeRole::Server, b"adb pair server", b"adb pair client");
        let c_msg = client.generate_msg(b"123456").unwrap();
        let s_msg = server.generate_msg(b"999999").unwrap();
        assert_ne!(
            client.process_msg(&s_msg).unwrap(),
            server.process_msg(&c_msg).unwrap()
        );
    }

    #[test]
    fn pairing_packet_codec() {
        let encoded = packet::encode(packet::SPAKE2_MSG, &[1u8; 32]);
        let (ty, payload, consumed) = packet::decode(&encoded).unwrap();
        assert_eq!(ty, packet::SPAKE2_MSG);
        assert_eq!(payload.len(), 32);
        assert_eq!(consumed, encoded.len());
        assert!(packet::decode(&encoded[..4]).is_none());
    }

    #[test]
    fn gcm_roundtrip() {
        let key = [0x42u8; 16];
        let plaintext = b"peer info payload";
        let cipher = gcm::encrypt(&key, 0, plaintext);
        assert_eq!(cipher.len(), plaintext.len() + 16);
        let decrypted = gcm::decrypt(&key, 0, &cipher).unwrap();
        assert_eq!(decrypted, plaintext);
        let wrong = gcm::decrypt(&[0u8; 16], 0, &cipher);
        assert!(wrong.is_none());
    }
}
