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
use rustls::{ClientConfig, DigitallySignedStruct, Error as TlsError, SignatureScheme};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_rustls::client::TlsConnector;
use tokio_rustls::client::TlsStream;

use crate::error::{AdbError, Result};
use crate::packet::{Command, Packet};
use crate::tcp::TcpTransport;
use crate::transport::{Transport, TransportReader, TransportWriter};

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
impl TransportReader for StlsTransport {
    async fn read_exact(&mut self, buf: &mut [u8]) -> Result<()> {
        self.tls
            .read_exact(buf)
            .await
            .map(|_| ())
            .map_err(|e| AdbError::Transport(format!("stls read: {e}")))
    }
}

#[async_trait]
impl TransportWriter for StlsTransport {
    async fn write_all(&mut self, buf: &[u8]) -> Result<()> {
        self.tls
            .write_all(buf)
            .await
            .map(|_| ())
            .map_err(|e| AdbError::Transport(format!("stls write: {e}")))
    }
}

#[async_trait]
impl Transport for StlsTransport {
    fn name(&self) -> String {
        self.peer.clone()
    }

    type Reader = tokio::io::ReadHalf<TlsStream<tokio::net::TcpStream>>;
    type Writer = tokio::io::WriteHalf<TlsStream<tokio::net::TcpStream>>;

    fn split(self) -> (Self::Reader, Self::Writer) {
        tokio::io::split(self.tls)
    }

    async fn close(&mut self) -> Result<()> {
        use tokio::io::AsyncWriteExt as _;
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

/// Transport for a wireless-debugging connect port: plain TCP until the
/// device sends `A_STLS`, then TLS 1.3.
#[derive(Debug)]
pub enum LanTransport {
    /// Raw TCP (CNXN sent, STLS not yet negotiated).
    Plain(TcpTransport),
    /// TLS 1.3 after the STLS exchange.
    Tls(Box<StlsTransport>),
    /// Transient state while upgrading (never used for IO).
    Upgrading,
}

impl LanTransport {
    /// Wrap a fresh TCP connection.
    #[must_use]
    pub fn plain(transport: TcpTransport) -> Self {
        Self::Plain(transport)
    }

    /// Upgrade to TLS 1.3 after both sides have exchanged `A_STLS`.
    ///
    /// The caller must have replied to the device's `A_STLS` already. On
    /// success subsequent reads/writes go through TLS.
    pub async fn upgrade(
        &mut self,
        cert_der: &[u8],
        key_der: &[u8],
        server_name: &str,
    ) -> Result<()> {
        let plain = match std::mem::replace(self, Self::Upgrading) {
            Self::Plain(transport) => transport,
            other => {
                *self = other;
                return Err(AdbError::Protocol(
                    "STLS: transport is already upgraded".into(),
                ));
            }
        };
        let tls = tls_handshake(plain, cert_der, key_der, server_name).await?;
        *self = Self::Tls(Box::new(tls));
        Ok(())
    }

    /// Whether TLS is active.
    #[must_use]
    pub fn is_tls(&self) -> bool {
        matches!(self, Self::Tls(_))
    }
}

/// Read half of a [`LanTransport`].
pub enum LanReader {
    /// Plain-TCP read half.
    Plain(tokio::io::ReadHalf<tokio::net::TcpStream>),
    /// TLS read half.
    Tls(tokio::io::ReadHalf<TlsStream<tokio::net::TcpStream>>),
}

/// Write half of a [`LanTransport`].
pub enum LanWriter {
    /// Plain-TCP write half.
    Plain(tokio::io::WriteHalf<tokio::net::TcpStream>),
    /// TLS write half.
    Tls(tokio::io::WriteHalf<TlsStream<tokio::net::TcpStream>>),
}

#[async_trait]
impl TransportReader for LanTransport {
    async fn read_exact(&mut self, buf: &mut [u8]) -> Result<()> {
        match self {
            Self::Plain(transport) => transport.read_exact(buf).await,
            Self::Tls(transport) => transport.read_exact(buf).await,
            Self::Upgrading => Err(AdbError::Protocol("STLS: transport upgrading".into())),
        }
    }
}

#[async_trait]
impl TransportWriter for LanTransport {
    async fn write_all(&mut self, buf: &[u8]) -> Result<()> {
        match self {
            Self::Plain(transport) => transport.write_all(buf).await,
            Self::Tls(transport) => transport.write_all(buf).await,
            Self::Upgrading => Err(AdbError::Protocol("STLS: transport upgrading".into())),
        }
    }
}

#[async_trait]
impl TransportReader for LanReader {
    async fn read_exact(&mut self, buf: &mut [u8]) -> Result<()> {
        match self {
            Self::Plain(reader) => TransportReader::read_exact(reader, buf).await,
            Self::Tls(reader) => TransportReader::read_exact(reader, buf).await,
        }
    }
}

#[async_trait]
impl TransportWriter for LanWriter {
    async fn write_all(&mut self, buf: &[u8]) -> Result<()> {
        match self {
            Self::Plain(writer) => TransportWriter::write_all(writer, buf).await,
            Self::Tls(writer) => TransportWriter::write_all(writer, buf).await,
        }
    }
}

#[async_trait]
impl Transport for LanTransport {
    fn name(&self) -> String {
        match self {
            Self::Plain(transport) => transport.name(),
            Self::Tls(transport) => transport.peer_name().to_string(),
            Self::Upgrading => "stls:upgrading".to_string(),
        }
    }

    type Reader = LanReader;
    type Writer = LanWriter;

    fn split(self) -> (Self::Reader, Self::Writer) {
        match self {
            Self::Plain(transport) => {
                let (reader, writer) = transport.split();
                (LanReader::Plain(reader), LanWriter::Plain(writer))
            }
            Self::Tls(transport) => {
                let transport = *transport;
                let (reader, writer) = transport.split();
                (LanReader::Tls(reader), LanWriter::Tls(writer))
            }
            Self::Upgrading => unreachable!("STLS transport split while upgrading"),
        }
    }

    async fn close(&mut self) -> Result<()> {
        match self {
            Self::Plain(transport) => transport.close().await,
            Self::Tls(transport) => transport.close().await,
            Self::Upgrading => Ok(()),
        }
    }
}

/// Server-certificate verifier that accepts any chain (AOSP behavior for adbd).
#[derive(Debug)]
pub struct AllowAnyVerifier;

impl rustls::client::danger::ServerCertVerifier for AllowAnyVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer,
        _intermediates: &[CertificateDer],
        _server_name: &ServerName,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
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
        vec![
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::RSA_PSS_SHA384,
            SignatureScheme::RSA_PSS_SHA512,
            SignatureScheme::RSA_PKCS1_SHA256,
            SignatureScheme::RSA_PKCS1_SHA384,
            SignatureScheme::RSA_PKCS1_SHA512,
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::ECDSA_NISTP384_SHA384,
            SignatureScheme::ECDSA_NISTP521_SHA512,
            SignatureScheme::ED25519,
        ]
    }
}

/// Perform the STLS upgrade: send `A_STLS`, complete TLS 1.3, return the wrapped stream.
///
/// The caller must have an unnegotiated [`TcpTransport`] (no CNXN yet). After
/// this returns, CNXN proceeds inside TLS. Wireless-debugging connections use
/// [`LanTransport`] instead: the device sends `A_STLS` in response to the
/// client's first CNXN, so the offer/reply steps are already done.
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
    tls_handshake(plain, cert_der, key_der, server_name).await
}

/// Complete the TLS 1.3 handshake after both sides have exchanged `A_STLS`.
async fn tls_handshake(
    plain: TcpTransport,
    cert_der: &[u8],
    key_der: &[u8],
    server_name: &str,
) -> Result<StlsTransport> {
    let config = build_client_config(cert_der, key_der)?;
    let connector = TlsConnector::from(Arc::new(config));
    let server_name = ServerName::try_from(server_name.to_string())
        .map_err(|_| AdbError::Protocol(format!("STLS: invalid server name {server_name:?}")))?;

    let (stream, peer_name) = plain.into_tcp_parts();
    let tls = tokio::time::timeout(STLS_TIMEOUT, connector.connect(server_name, stream))
        .await
        .map_err(|_| AdbError::Timeout("STLS: TLS handshake timed out".into()))?
        .map_err(|e| AdbError::Protocol(format!("STLS: TLS handshake failed: {e}")))?;

    Ok(StlsTransport {
        tls,
        peer: peer_name,
    })
}

fn build_client_config(cert_der: &[u8], key_der: &[u8]) -> Result<ClientConfig> {
    let cert_chain = vec![CertificateDer::from(cert_der.to_vec())];
    let key = PrivateKeyDer::try_from(key_der.to_vec())
        .map_err(|e| AdbError::Protocol(format!("STLS: invalid private key DER: {e}")))?;
    // Both rustls providers (ring via this crate, aws-lc-rs via tokio-rustls
    // defaults) are compiled in, so `ClientConfig::builder()` would panic:
    // select the ring provider explicitly.
    let mut config =
        ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .map_err(|e| AdbError::Protocol(format!("STLS: TLS versions: {e}")))?
            .with_root_certificates(rustls::RootCertStore::empty())
            .with_client_auth_cert(cert_chain, key)
            .map_err(|e| AdbError::Protocol(format!("STLS: client auth cert: {e}")))?;
    config
        .dangerous()
        .set_certificate_verifier(Arc::new(AllowAnyVerifier));
    Ok(config)
}
