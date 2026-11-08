//! Full wireless-debugging pairing flow (client side).
//!
//! `pair <host:port> <code>` per AOSP `client/adb_wifi.cpp`:
//! 1. Generate a self-signed X509 certificate from our ADB RSA key.
//!    `cert_der` + `key_der` are handed to TLS (client cert) as DER.
//! 2. TLS 1.3 handshake (mutual certs, verify-anything).
//! 3. Fetch the 64-byte TLS exporter (`adb-label\0`) and append it to the
//!    pairing code: `password = code.as_bytes() + exporter(64)`.
//! 4. Send our SPAKE2 blinded point (`SPAKE2_MSG`, 32 bytes).
//! 5. Read the device's SPAKE2 point, derive the 64-byte key.
//! 6. GCM key = HKDF-SHA256(spake_key, info="adb pairing_auth aes-128-gcm key").
//! 7. Encrypt PeerInfo(`ADB_RSA_PUB_KEY`, our Android user key) and send
//!    (`PEER_INFO`, counter 0).
//! 8. Read the device's encrypted PeerInfo, decrypt (counter 0), expect
//!    `ADB_DEVICE_GUID`. Returns the GUID.

use std::sync::Arc;

use async_trait::async_trait;
use rsa::pkcs1::EncodeRsaPublicKey;
use rsa::pkcs8::EncodePrivateKey;
use sha1::Digest as Sha1Digest;
use tokio::net::TcpStream;

use crate::auth::AdbKeyPair;
use crate::error::{AdbError, Result};
use crate::pairing::gcm;
use crate::pairing::{derive_gcm_key, packet, peer_info, Spake2, SpakeRole};
use crate::transport::Transport;

/// Outcome of a successful pairing.
#[derive(Debug, Clone)]
pub struct PairingOutcome {
    /// Device GUID as reported in PeerInfo (UTF-8).
    pub device_guid: String,
}

/// Generate a self-signed X509 cert (DER) from an RSA key, AOSP profile:
/// serial 1, notBefore=now, notAfter=+10 years, C=US/O=Android/CN=Adb,
/// critical basicConstraints `CA:TRUE`, critical keyUsage
/// `keyCertSign,cRLSign,digitalSignature`, subjectKeyIdentifier = SHA-1
/// (OpenSSL `hash`), signed RSA-SHA256 (AOSP `crypto/x509_generator.cpp`).
#[allow(clippy::field_reassign_with_default)] // CertificateParams is non_exhaustive
pub fn generate_x509_der(private_key: &rsa::RsaPrivateKey) -> Result<Vec<u8>> {
    let key_der = private_key
        .to_pkcs8_der()
        .map_err(|e| AdbError::Protocol(format!("X509: cannot encode private key: {e}")))?;
    let key_pair = rcgen::KeyPair::from_pkcs8_der_and_sign_algo(
        &rustls::pki_types::PrivatePkcs8KeyDer::from(key_der.as_bytes().to_vec()),
        &rcgen::PKCS_RSA_SHA256,
    )
    .map_err(|e| AdbError::Protocol(format!("X509: cannot load RSA key: {e}")))?;

    // OpenSSL's subjectKeyIdentifier "hash" is SHA-1 over the subjectPublicKey
    // BIT STRING contents, i.e. the PKCS#1 RSAPublicKey DER for RSA keys.
    let public_key_der = private_key
        .to_public_key()
        .to_pkcs1_der()
        .map_err(|e| AdbError::Protocol(format!("X509: cannot encode public key: {e}")))?;
    let key_id = sha1::Sha1::digest(public_key_der.as_bytes());

    let now = time::OffsetDateTime::now_utc();
    let mut params = rcgen::CertificateParams::default();
    params.not_before = now;
    params.not_after = now + time::Duration::days(10 * 365);
    params.serial_number = Some(rcgen::SerialNumber::from_slice(&[1]));
    params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    params.key_usages = vec![
        rcgen::KeyUsagePurpose::KeyCertSign,
        rcgen::KeyUsagePurpose::CrlSign,
        rcgen::KeyUsagePurpose::DigitalSignature,
    ];
    params.key_identifier_method = rcgen::KeyIdMethod::PreSpecified(key_id.to_vec());
    params.use_authority_key_identifier_extension = false;

    let mut name = rcgen::DistinguishedName::new();
    name.push(rcgen::DnType::CountryName, "US");
    name.push(rcgen::DnType::OrganizationName, "Android");
    name.push(rcgen::DnType::CommonName, "Adb");
    params.distinguished_name = name;

    let cert = params
        .self_signed(&key_pair)
        .map_err(|e| AdbError::Protocol(format!("X509: signing failed: {e}")))?;
    Ok(cert.der().to_vec())
}

/// TLS 1.3 wrapper around a TcpStream used for pairing (mutual certs, verify-any).
pub struct PairingTls {
    tls: tokio_rustls::client::TlsStream<TcpStream>,
}

#[async_trait]
impl Transport for PairingTls {
    fn name(&self) -> String {
        "pairing-tls".to_string()
    }

    async fn read_exact(&mut self, buf: &mut [u8]) -> Result<()> {
        use tokio::io::AsyncReadExt;
        self.tls
            .read_exact(buf)
            .await
            .map(|_| ())
            .map_err(|e| AdbError::Transport(format!("pairing-tls read: {e}")))
    }

    async fn write_all(&mut self, buf: &[u8]) -> Result<()> {
        use tokio::io::AsyncWriteExt;
        self.tls
            .write_all(buf)
            .await
            .map(|_| ())
            .map_err(|e| AdbError::Transport(format!("pairing-tls write: {e}")))
    }

    async fn close(&mut self) -> Result<()> {
        Ok(())
    }
}

impl std::fmt::Debug for PairingTls {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PairingTls").finish()
    }
}

/// Run the full pairing exchange over an unencrypted TcpStream.
///
/// `keys` supplies the ADB RSA key: its self-signed certificate is the TLS
/// client certificate, and its user key is sent in PeerInfo.
/// `host_pubkey_userkey` is the `adbkey.pub` body (`base64 user@host`).
/// Returns the device GUID when pairing succeeds.
pub async fn pair(
    stream: TcpStream,
    code: &str,
    keys: &AdbKeyPair,
    host_pubkey_userkey: &str,
) -> Result<PairingOutcome> {
    let cert_der = keys.certificate_der()?;
    let key_der = keys.private_key_der()?;

    // TLS 1.3 (verify-anything on both ends).
    let config = client_config(&cert_der, &key_der)?;
    let connector = tokio_rustls::client::TlsConnector::from(Arc::new(config));
    let server_name = rustls::pki_types::ServerName::try_from("adb".to_string())
        .map_err(|_| AdbError::Protocol("pairing: invalid server name".into()))?;
    let tls = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        connector.connect(server_name, stream),
    )
    .await
    .map_err(|_| AdbError::Timeout("pairing TLS handshake timed out".into()))?
    .map_err(|e| AdbError::Protocol(format!("pairing TLS handshake failed: {e}")))?;

    // AOSP appends the TLS exporter (label `adb-label\0`, 64 bytes, no
    // context) to the pairing code before running SPAKE2.
    let exporter = {
        let (_, connection) = tls.get_ref();
        let mut out = [0u8; 64];
        connection
            .export_keying_material(&mut out[..], b"adb-label\0", None)
            .map_err(|e| AdbError::Protocol(format!("pairing: TLS exporter failed: {e}")))?;
        out
    };
    let mut password = code.as_bytes().to_vec();
    password.extend_from_slice(&exporter);

    let mut tls = PairingTls { tls };
    pair_with_password(&mut tls, &password, host_pubkey_userkey).await
}

/// The testable core: SPAKE2 + PeerInfo over any [`Transport`].
///
/// `password` is code bytes + 64 exporter bytes (assembled by the caller from
/// the TLS session).
pub async fn pair_with_password<T: Transport>(
    transport: &mut T,
    password: &[u8],
    host_pubkey_android_b64: &str,
) -> Result<PairingOutcome> {
    let mut spake = Spake2::new(
        SpakeRole::Client,
        b"adb pair client\0",
        b"adb pair server\0",
    );
    let my_msg = spake.generate_msg(password)?;

    // 1) Send our SPAKE2 point.
    transport
        .write_all(&packet::encode(packet::SPAKE2_MSG, &my_msg))
        .await?;

    // 2) Read the device's SPAKE2 point.
    let their_msg = read_next_spake_msg(transport).await?;
    let key = spake.process_msg(&their_msg)?;

    // 3) GCM key + encrypt our PeerInfo (counter 0).
    let gcm_key = derive_gcm_key(&key);
    let mut peer_info = [0u8; peer_info::SIZE];
    peer_info[0] = peer_info::ADB_RSA_PUB_KEY;
    let pubkey = host_pubkey_android_b64.as_bytes();
    if pubkey.len() > peer_info::SIZE - 1 {
        return Err(AdbError::Protocol("pairing: user key too large".into()));
    }
    peer_info[1..1 + pubkey.len()].copy_from_slice(pubkey);
    let encrypted = gcm::encrypt(&gcm_key, 0, &peer_info);
    transport
        .write_all(&packet::encode(packet::PEER_INFO, &encrypted))
        .await?;

    // 4) Read the device's encrypted PeerInfo (counter 0), decrypt, check type.
    let device_encrypted = read_next_peer_info(transport).await?;
    let decrypted = gcm::decrypt(&gcm_key, 0, &device_encrypted)
        .ok_or_else(|| AdbError::Protocol("pairing: PeerInfo GCM tag mismatch".into()))?;

    if decrypted.is_empty() {
        return Err(AdbError::Protocol("pairing: empty PeerInfo".into()));
    }
    if decrypted[0] != peer_info::ADB_DEVICE_GUID {
        return Err(AdbError::Protocol(format!(
            "pairing: expected ADB_DEVICE_GUID, got {}",
            decrypted[0]
        )));
    }
    let guid_end = decrypted
        .iter()
        .skip(1)
        .position(|&b| b == 0)
        .unwrap_or_else(|| decrypted.len() - 1)
        + 1;
    let device_guid = String::from_utf8_lossy(&decrypted[1..guid_end]).into_owned();

    Ok(PairingOutcome { device_guid })
}

async fn read_next_spake_msg<T: Transport>(transport: &mut T) -> Result<Vec<u8>> {
    loop {
        let mut header = [0u8; packet::HEADER_LEN];
        transport.read_exact(&mut header).await?;
        let ty = header[1];
        let len = u32::from_be_bytes(header[2..6].try_into().expect("slice")) as usize;
        if len > 8192 {
            return Err(AdbError::Protocol("pairing packet too large".into()));
        }
        let mut payload = vec![0u8; len];
        transport.read_exact(&mut payload).await?;
        if ty == packet::SPAKE2_MSG {
            return Ok(payload);
        }
        // Ignore others until we see SPAKE2_MSG.
    }
}

async fn read_next_peer_info<T: Transport>(transport: &mut T) -> Result<Vec<u8>> {
    loop {
        let mut header = [0u8; packet::HEADER_LEN];
        transport.read_exact(&mut header).await?;
        let ty = header[1];
        let len = u32::from_be_bytes(header[2..6].try_into().expect("slice")) as usize;
        if len > 8192 {
            return Err(AdbError::Protocol("pairing packet too large".into()));
        }
        let mut payload = vec![0u8; len];
        transport.read_exact(&mut payload).await?;
        if ty == packet::PEER_INFO {
            return Ok(payload);
        }
    }
}

fn client_config(cert_der: &[u8], key_der: &[u8]) -> Result<rustls::ClientConfig> {
    use rustls::pki_types::PrivateKeyDer;

    let key = PrivateKeyDer::try_from(key_der.to_vec())
        .map_err(|e| AdbError::Protocol(format!("pairing: invalid private key DER: {e}")))?;

    // Both rustls providers (ring via this crate, aws-lc-rs via tokio-rustls
    // defaults) are compiled in, so `ClientConfig::builder()` would panic:
    // select the ring provider explicitly.
    let mut config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|e| AdbError::Protocol(format!("pairing: TLS versions: {e}")))?
    .with_root_certificates(rustls::RootCertStore::empty())
    .with_client_auth_cert(
        vec![rustls::pki_types::CertificateDer::from(cert_der.to_vec())],
        key,
    )
    .map_err(|e| AdbError::Protocol(format!("pairing: client auth cert: {e}")))?;
    config
        .dangerous()
        .set_certificate_verifier(Arc::new(crate::stls::AllowAnyVerifier));
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn x509_certificate_is_generated_for_rsa_key() {
        let mut rng = rand::thread_rng();
        let key = rsa::RsaPrivateKey::new(&mut rng, 2048).expect("keygen");
        let der = generate_x509_der(&key).expect("certificate");
        // DER SEQUENCE, well over the RSA signature size.
        assert_eq!(der.first(), Some(&0x30));
        assert!(der.len() > 500);
        // Subject CN=Adb (PrintableString/UTF8String contents).
        assert!(der.windows(3).any(|w| w == b"Adb"));
    }
}
