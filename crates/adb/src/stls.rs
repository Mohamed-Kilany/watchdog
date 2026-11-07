//! STLS upgrade — the TLS 1.3 layer used by wireless debugging (Android 11+).
//!
//! After pairing, the device listens TLS on its connect port. The client sends
//! `A_STLS` (arg0 = version = 1, no payload), the device accepts, TLS 1.3
//! handshake completes (verify-anything on both sides), and the CNXN/AUTH
//! exchange proceeds inside TLS. AOSP uses BoringSSL's
//! `SSL_set_cert_verify_callback` to accept any chain; rustls is configured
//! identically here via an allow-anything server-certificate verifier.

use std::sync::Arc;

use async_trait::async_trait;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use rustls::{ClientConfig, DigitallySignedStruct, Error as TlsError, SignatureScheme, UnixTime};
use tokio_rustls::client::TlsConnector;
use tokio_rustls::client::TlsStream;

use crate::error::{AdbError, Result};
use crate::packet::{Command, Packet};
use crate::tcp::TcpTransport;
use crate::transport::Transport;

/// STLS version we offer ([`crate::packet::A_STLS_VERSION`]).
pub const STLS_VERSION: u32 = crate::packet::A_STLS_VERSION;

/// Post-handshake grace period for the device to begin CNXN.
const STLS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// TLS 1.3 transport wrapper produced by [`upgrade_stls`].
pub struct StlsTransport {
    tls: TlsStream<tokio::net::TcpStream>,
    peer: String,
}

impl StlsTransport {
    /// Peer name (`stls:<host>:<port>`).
    #[must_use]
    pub fn peer_name(&self) -> &str {
        &self.peer
    }
}

#[async_trait]
impl Transport for StlsTransport {
    fn name(&self) -> String {
        self.peer.clone()
    }

    async fn read_exact(&mut self, buf: &mut [u8]) -> Result<()> {
        use tokio::io::AsyncReadExt;
        self.tls
            .read_exact(buf)
            .await
            .map_err(|e| AdbError::Transport(format!("stls read: {e}")))
    }

    async fn write_all(&mut self, buf: &[u8]) -> Result<()> {
        use tokio::io::AsyncWriteExt;
        self.tls
            .write_all(buf)
            .await
            .map_err(|e| AdbError::Transport(format!("stls write: {e}")))
    }

    async fn close(&mut self) -> Result<()> {
        let _ = self.tls.shutdown().await;
        Ok(())
    }
}

impl std::fmt::Debug for StlsTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StlsTransport")
            .field("peer", &self.peer)
            .finish()
    }
}

/// Server-certificate verifier that accepts any chain (AOSP behavior for adbd).
pub struct AllowAnyVerifier;

impl rustls::client::danger::ServerCertVerifier for AllowAnyVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer,
        _intermediates: &[CertificateDer],
        _server_name: &ServerName,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> std::result::Result<ServerCertVerified, TlsError> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer,
        _dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, TlsError> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer,
        _dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, TlsError> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        SignatureScheme::supported_from_tls13_and_tls12()
    }
}

/// Perform the STLS upgrade: send `A_STLS`, complete TLS 1.3, return the wrapped stream.
///
/// The caller must have an unnegotiated [`TcpTransport`] (no CNXN yet). After
/// this returns, CNXN proceeds inside TLS.
pub async fn upgrade_stls(
    mut plain: TcpTransport,
    cert_der: &[u8],
    key_der: &[u8],
    server_name: &str,
) -> Result<StlsTransport> {
    // 1) Offer STLS.
    let offer = Packet::new(Command::Stls, STLS_VERSION, 0, &[][..]);
    plain.write_all(&offer.encode()).await?;

    // 2) Device must answer A_STLS (any acceptable reply means we can try TLS).
    let mut reply = [0u8; 24];
    tokio::time::timeout(STLS_TIMEOUT, plain.read_exact(&mut reply))
        .await
        .map_err(|_| AdbError::Timeout("STLS: no A_STLS reply from device".into()))??;
    let command_id = u32::from_le_bytes(reply[0..4].try_into().expect("slice"));
    if command_id != Command::Stls.as_u32() {
        return Err(AdbError::Protocol(format!(
            "STLS: expected A_STLS reply, got 0x{command_id:08x}"
        )));
    }

    // 3) TLS 1.3 with mutual (self-signed device-side) certs.
    let config = build_client_config(cert_der, key_der)?;
    let connector = TlsConnector::from(Arc::new(config));
    let server_name = ServerName::try_from(server_name.to_string())
        .map_err(|_| AdbError::Protocol(format!("STLS: invalid server name {server_name:?}")))?;

    let peer = plain.peer();
    let (stream, peer_name) = plain.into_tcp_parts();
    let tls = tokio::time::timeout(STLS_TIMEOUT, connector.connect(server_name, stream))
        .await
        .map_err(|_| AdbError::Timeout("STLS: TLS handshake timed out".into()))?
        .map_err(|e| AdbError::Protocol(format!("STLS: TLS handshake failed: {e}")))?;

    let _ = peer;
    Ok(StlsTransport {
        tls,
        peer: peer_name,
    })
}

fn build_client_config(cert_der: &[u8], key_der: &[u8]) -> Result<ClientConfig> {
    let cert_chain = vec![CertificateDer::from(cert_der.to_vec())];
    let key = PrivateKeyDer::from(key_der.to_vec());
    let mut config = ClientConfig::builder()
        .with_root_certificates(rustls::RootCertStore::empty())
        .with_client_auth_cert(cert_chain, key)
        .map_err(|e| AdbError::Protocol(format!("STLS: client auth cert: {e}")))?;
    config
        .dangerous()
        .set_certificate_verifier(Arc::new(AllowAnyVerifier));
    Ok(config)
}
