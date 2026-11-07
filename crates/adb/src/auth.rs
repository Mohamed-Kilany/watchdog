//! ADB authentication — RSA-2048 key pair and the Android public-key format.
//!
//! Wire profile (AOSP `client/auth.cpp`, `libcrypto_utils/android_pubkey.cpp`,
//! `crypto/rsa_2048_key.cpp`):
//!
//! - token signing: SHA-1 digest, RSASSA-PKCS1-v1_5, 2048-bit key
//! - public key on the wire: Android's custom RSAPublicKey struct, base64'd,
//!   with `" <user>@<host>"` appended; AUTH_RSAPUBLICKEY payload has a trailing NUL
//!
//! Android RSAPublicKey layout (524 bytes, all ints little-endian):
//!
//! ```text
//! u32 modulus_size_words   // 64
//! u32 n0inv                // -1/N[0] mod 2^32
//! u8  modulus[256]         // little-endian
//! u8  rr[256]              // (2^2048)^2 mod N, little-endian
//! u32 exponent             // 65537
//! ```

use std::path::{Path, PathBuf};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use rsa::pkcs1::DecodeRsaPrivateKey;
use rsa::pkcs8::{DecodePrivateKey, EncodePrivateKey, LineEnding};
use rsa::traits::PublicKeyParts;
use rsa::{Pkcs1v15Sign, RsaPrivateKey, RsaPublicKey};
use sha1::Digest as Sha1Digest;

use crate::error::{AdbError, Result};

/// Android public-key struct size (modulus 2048 bits).
pub const ANDROID_PUBKEY_ENCODED_SIZE: usize = 3 * 4 + 2 * 256;
/// Modulus length in 32-bit words.
pub const MODULUS_SIZE_WORDS: u32 = 64;

/// An ADB RSA key pair loaded from (or generated into) the data dir.
pub struct AdbKeyPair {
    /// Private key (never logged).
    private_key: RsaPrivateKey,
    /// Public key parts.
    public_key: RsaPublicKey,
}

impl std::fmt::Debug for AdbKeyPair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdbKeyPair").finish_non_exhaustive()
    }
}

impl AdbKeyPair {
    /// Load the key pair from `<dir>/adbkey`; generate and store one if absent.
    ///
    /// Files are created with owner-only permissions on Unix. The private key is
    /// stored as unencrypted PKCS#8 PEM — the same threat model as ssh(1)'s
    /// unencrypted private keys.
    pub fn load_or_create(dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
        }
        let key_path = dir.join("adbkey");
        let pub_path = dir.join("adbkey.pub");

        if key_path.exists() {
            let pem = std::fs::read_to_string(&key_path)?;
            let private_key = RsaPrivateKey::from_pkcs8_pem(&pem)
                .or_else(|_| RsaPrivateKey::from_pkcs1_pem(&pem))
                .map_err(|e| {
                    AdbError::Protocol(format!("cannot parse {}: {e}", key_path.display()))
                })?;
            let public_key = private_key.to_public_key();
            let pair = Self {
                private_key,
                public_key,
            };
            pair.write_public_key_file(&pub_path)?;
            return Ok(pair);
        }

        let mut rng = rand::thread_rng();
        let private_key = RsaPrivateKey::new(&mut rng, 2048)
            .map_err(|e| AdbError::Protocol(format!("RSA-2048 key generation failed: {e}")))?;
        let pem = private_key
            .to_pkcs8_pem(LineEnding::LF)
            .map_err(|e| AdbError::Protocol(format!("cannot encode private key: {e}")))?;
        std::fs::write(&key_path, pem.as_bytes())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600));
        }
        let public_key = private_key.to_public_key();
        let pair = Self {
            private_key,
            public_key,
        };
        pair.write_public_key_file(&pub_path)?;
        Ok(pair)
    }

    /// The public key as an `adbkey.pub` body (base64 + " user@host"), no trailing newline.
    #[must_use]
    pub fn public_key_userkey(&self) -> String {
        format!(
            "{} {}@{}",
            BASE64.encode(self.android_public_key()),
            user_name(),
            host_name()
        )
    }

    /// The full `AUTH_RSAPUBLICKEY` payload (user key + trailing NUL).
    #[must_use]
    pub fn public_key_payload(&self) -> Vec<u8> {
        let mut out = self.public_key_userkey().into_bytes();
        out.push(0);
        out
    }

    /// Android's binary RSAPublicKey (524 bytes).
    #[must_use]
    pub fn android_public_key(&self) -> [u8; ANDROID_PUBKEY_ENCODED_SIZE] {
        encode_android_public_key(&self.public_key)
    }

    /// Sign `token` (20-byte challenge) with SHA-1 + PKCS#1 v1.5.
    pub fn sign_token(&self, token: &[u8]) -> Result<Vec<u8>> {
        let digest = sha1::Sha1::digest(token);
        self.private_key
            .sign(Pkcs1v15Sign::new_unprefixed(), digest.as_slice())
            .map_err(|e| AdbError::Protocol(format!("token signing failed: {e}")))
    }

    fn write_public_key_file(&self, path: &PathBuf) -> Result<()> {
        let mut contents = self.public_key_userkey();
        contents.push('\n');
        std::fs::write(path, contents)?;
        Ok(())
    }
}

fn user_name() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "watchdog".to_string())
}

fn host_name() -> String {
    gethostname().unwrap_or_else(|| "localhost".to_string())
}

#[cfg(unix)]
fn gethostname() -> Option<String> {
    // Avoid extra deps; /proc/sys/kernel/hostname on Linux, `hostname` elsewhere.
    if let Ok(name) = std::fs::read_to_string("/proc/sys/kernel/hostname") {
        let trimmed = name.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    std::process::Command::new("hostname")
        .output()
        .ok()
        .and_then(|o| {
            let s = String::from_utf8(o.stdout).ok()?;
            Some(s.trim().to_string())
        })
}

#[cfg(windows)]
fn gethostname() -> Option<String> {
    std::env::var("COMPUTERNAME").ok()
}

/// Encode an RSA public key in Android's RSAPublicKey format.
fn encode_android_public_key(public_key: &RsaPublicKey) -> [u8; ANDROID_PUBKEY_ENCODED_SIZE] {
    use num_bigint::BigUint;

    let modulus = BigUint::from_bytes_be(&public_key.n().to_bytes_be());
    let exponent_bytes = public_key.e().to_bytes_be();
    let mut exponent: u64 = 0;
    for byte in exponent_bytes {
        exponent = (exponent << 8) | u64::from(byte);
    }
    let exponent = u32::try_from(exponent).expect("RSA exponent fits u32");

    // n0inv = -1 / n[0] (mod 2^32), where n[0] is the low 32-bit word.
    let low_word = modulus.iter_u32_digits().next().expect("non-empty modulus");
    let n0inv = mod_inverse_neg(low_word);

    // rr = (2^2048)^2 mod N = 2^4096 mod N.
    let rr = BigUint::from(2u8).modpow(&BigUint::from(4096u32), &modulus);

    let mut out = [0u8; ANDROID_PUBKEY_ENCODED_SIZE];
    out[0..4].copy_from_slice(&MODULUS_SIZE_WORDS.to_le_bytes());
    out[4..8].copy_from_slice(&n0inv.to_le_bytes());

    let mut offset = 8;
    let modulus_le = modulus.to_bytes_le();
    out[offset..offset + 256].copy_from_slice(&modulus_le);
    offset += 256;
    let rr_le = rr.to_bytes_le();
    out[offset..offset + 256].copy_from_slice(&rr_le);
    offset += 256;
    out[offset..offset + 4].copy_from_slice(&exponent.to_le_bytes());
    out
}

/// Compute `-1/x mod 2^32` for odd `x` (modular inverse, negated).
fn mod_inverse_neg(x: u32) -> u32 {
    // Newton–Hensel lifting: start with x0 = x (odd ⇒ inverse mod 2).
    // Iterate: y ← y * (2 - x*y) mod 2^32, doubles correct bits each step.
    let mut y: u32 = 1;
    for _ in 0..5 {
        y = y.wrapping_mul(2u32.wrapping_sub(x.wrapping_mul(y)));
    }
    0u32.wrapping_sub(y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn n0inv_satisfies_definition() {
        for modulus_limb in [3u32, 0x1234567f, 0xfffffff1, 0xdeadbeef | 1] {
            let inv = mod_inverse_neg(modulus_limb);
            // inv * n ≡ -1 (mod 2^32)
            assert_eq!(
                modulus_limb.wrapping_mul(inv),
                0u32.wrapping_sub(1),
                "n={modulus_limb:#x}"
            );
        }
    }

    #[test]
    fn android_pubkey_layout_is_valid() {
        let mut rng = rand::thread_rng();
        let private_key = RsaPrivateKey::new(&mut rng, 2048).expect("keygen");
        let public_key = private_key.to_public_key();
        let encoded = encode_android_public_key(&public_key);
        assert_eq!(
            u32::from_le_bytes(encoded[0..4].try_into().unwrap()),
            MODULUS_SIZE_WORDS
        );
        assert!(BASE64.encode(encoded).len() > 600); // ~704 chars
        let exponent = u32::from_le_bytes(encoded[520..524].try_into().unwrap());
        assert_eq!(exponent, 65537);
        // rr = 2^4096 mod N
        let n = num_bigint::BigUint::from_bytes_le(&encoded[8..264]);
        let rr = num_bigint::BigUint::from_bytes_le(&encoded[264..520]);
        assert_eq!(
            rr,
            num_bigint::BigUint::from(2u8).modpow(&num_bigint::BigUint::from(4096u32), &n)
        );
    }

    #[test]
    fn sign_verify_roundtrip() {
        let mut rng = rand::thread_rng();
        let private_key = RsaPrivateKey::new(&mut rng, 2048).expect("keygen");
        let pair = AdbKeyPair {
            private_key: private_key.clone(),
            public_key: private_key.to_public_key(),
        };
        let token = [0x5au8; 20];
        let signature = pair.sign_token(&token).expect("sign");
        let digest = sha1::Sha1::digest(token);
        let scheme = Pkcs1v15Sign::new_unprefixed();
        pair.public_key
            .verify(scheme, digest.as_slice(), &signature)
            .expect("verify");
    }

    #[test]
    fn sha1_known_answer() {
        // Guards the NID_sha1 compatibility requirement.
        assert_eq!(
            format!("{:x}", sha1::Sha1::digest(b"abc")),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
    }

    #[test]
    fn public_key_file_format() {
        let mut rng = rand::thread_rng();
        let private_key = RsaPrivateKey::new(&mut rng, 2048).expect("keygen");
        let pair = AdbKeyPair {
            private_key: private_key.clone(),
            public_key: private_key.to_public_key(),
        };
        let userkey = pair.public_key_userkey();
        // Must be `base64 user@host`
        let parts: Vec<&str> = userkey.split(' ').collect();
        assert_eq!(parts.len(), 2);
        let decoded = BASE64.decode(parts[0]).expect("valid base64");
        assert_eq!(decoded.len(), ANDROID_PUBKEY_ENCODED_SIZE);
        assert!(parts[1].contains('@'));
        let payload = pair.public_key_payload();
        assert_eq!(*payload.last().unwrap(), 0);
    }
}
