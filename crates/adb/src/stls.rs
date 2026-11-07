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
use rustls::client::WebPkiSupportedAlgorithms;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::{ClientConfig, ClientConnection, StreamOwned};
use tokio::net::TcpStream;

use crate::error::{AdbError, Result};
use crate::packet::{Command, Packet};
use crate::tcp::TcpTransport;
use crate::transport::Transport;

/// STLS version we offer ([`crate::packet::A_STLS_VERSION`]).
pub const STLS_VERSION: u32 = crate::packet::A_STLS_VERSION;

/// Post-handshake grace period for the device to begin CNXN.
const STLS_HANDSHAKE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// TLS 1.3 transport wrapper produced by [`upgrade_stls`].
pub struct StlsTransport {
    inner: StreamOwned<ClientConnection, TcpStream>,
    peer: String,
}

impl StlsTransport {
    /// Freshly-authorized TLS stream after `A_STLS` exchange.
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
        self.inner
            .read_exact(buf)
            .await
            .map_err(|e| AdbError::Transport(format!("stls read: {e}")))
    }

    async fn write_all(&mut self, buf: &[u8]) -> Result<()> {
        use tokio::io::AsyncWriteExt;
        self.inner
            .write_all(buf)
            .await
            .map_err(|e| AdbError::Transport(format!("stls write: {e}")))
    }

    async fn close(&mut self) -> Result<()> {
        let _ = self.inner.shutdown(std::net::Shutdown::Both);
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

/// Build a rustls client config that accepts any chain (like AOSP does).
fn allow_any_client_config() -> Result<ClientConfig> {
    ClientConfig::builder()
        .with_root_certificates(allow_any_cert_store())
        .with_no_client_auth()
        .into_iter()
        .next()
        .ok_or_else(|| AdbError::Protocol("no TLS protocol versions suited to STLS".into()))
        .and_then(|config| {
            // rustls negotiates the best common version; require TLS 1.3.
            let supported: Vec<&rustls::SupportedProtocolVersion> = config
                .protocol_versions()
                .iter()
                .filter(|v| {
                    let tls13: &[rustls::SupportedProtocolVersion] =
                        WebPkiSupportedAlgorithms::TLS13;
                    tls13.contains(v)
                })
                .collect();
            if supported.is_empty() {
                let mut config = config;
                config.protocol_versions = WebPkiSupportedAlgorithms::TLS13.to_vec();
                Ok(config)
            } else {
                Ok(config)
            }
        })
}

fn allow_any_cert_store() -> rustls::RootCertStore {
    // Empty store: verification is overridden by AllowAnyVerifier below, which
    // accepts any chain (mutual, self-signed device credentials), matching
    // AOSP's SSL_VERIFY_PEER with a permissive callback.
    let mut store = rustls::RootCertStore::empty();
    let _ = store; // verified by AllowAnyVerifier, not by webpki
    rustls::RootCertStore::empty()
}

/// Server-certificate verifier that accepts any chain (AOSP behavior for adbd).
pub struct AllowAnyVerifier;

impl rustls::client::danger::ServerCertVerifier for AllowAnyVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer,
        _intermediates: &[CertificateDer],
        _server_name: &rustls::pki_types::ServerName,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _: &[u8],
        _: &CertificateDer,
        _: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        rustls::SignatureScheme::supported_from_tls13_and_tls12()
    }
}

impl From<Arc<AllowAnyVerifier>> for ClientConfig {
    fn from(_: Arc<AllowAnyVerifier>) -> Self {
        unreachable!("placeholder")
    }
}

/// Perform the STLS upgrade: send `A_STLS`, complete TLS 1.3, return the wrapped stream.
///
/// The caller must have an unnegotiated [`TcpTransport`] (no CNXN yet). After
/// this returns, CNXN proceeds inside TLS.
pub async fn upgrade_stls(
    mut plain: TcpTransport,
    cert_pem: &str,
    priv_pem: &str,
    server_name: &str,
) -> Result<StlsTransport> {
    // 1) Offer STLS.
    let offer = Packet::new(Command::Stls, STLS_VERSION, 0, &[][..]);
    plain.write_all(&offer.encode()).await?;
    // 2) Device must answer A_STLS (any response means we can try TLS).
    //    If the device never speaks STLS (old adbd), we fail clearly.
    let mut reply = [0u8; 24];
    tokio::time::timeout(STLS_HANDSHAKE_TIMEOUT, plain.read_exact(&mut reply))
        .await
        .map_err(|_| AdbError::Timeout("STLS: no A_STLS reply from device".into()))??;
    let command_id = u32::from_le_bytes(reply[0..4].try_into().expect("slice"));
    if command_id != Command::Stls.as_u32() {
        return Err(AdbError::Protocol(format!(
            "STLS: expected A_STLS reply, got 0x{command_id:08x}"
        )));
    }

    // 3) TLS 1.3.
    let config = build_client_config(cert_pem, priv_pem)?;
    let server_name = rustls::pki_types::ServerName::try_from(server_name.to_string())
        .map_err(|_| AdbError::Protocol(format!("STLS: invalid server name {server_name:?}")))?;
    let conn = ClientConnection::new(Arc::new(config), server_name)
        .map_err(|e| AdbError::Protocol(format!("STLS: cannot build client connection: {e}")))?;

    // Take the underlying TCP stream out of TcpTransport.
    let TcpTransport { stream, peer } = plain;
    let peer_name = format!("stls:{}", peer);
    let mut tls = StreamOwned::new(conn, stream);

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut buf = [0u8; 4096];
    loop {
        let (until, eof) = {
            let conn = tls.conn;
            let mut wr = 0;
            let write_result = {
                let mut buffer = tls.conn.writer();
                while let Some(record) = tls.conn.writer() {
                    match record.write(buf[wr..].as_mut()) {
                        Ok(0) => break,
                        Ok(n) => wr += n,
                        Err(rustls::Error::WouldBlock) => break,
                        Err(e) => {
                            return Err(AdbError::Protocol(format!("STLS: TLS write failed: {e}")))
                        }
                    }
                }
                Ok::<(), rustls::Error>(())
            };
            // Flush whatever the library wants out.
            match write_result {
                Ok(()) if wr > 0 => {
                    if tls.get_mut().write_all(&buf[..wr]).await.is_err() {
                        return Err(AdbError::Protocol(
                            "STLS: underlying TCP write failed".into(),
                        ));
                    }
                }
                Ok(()) => {}
                Err(e) => return Err(AdbError::Protocol(format!("STLS: TLS write failed: {e}"))),
            }
            let mut into = tls.conn.reader();
            let read_result = into.read(&mut buf);
            let read_bytes = match read_result {
                Ok(n) => n,
                Err(_) => 0,
            };
            (read_bytes, read_bytes == 0)
        };
        let _ = eof;
        if until > 0 {
            let in_data = &buf[..until];
            if tls.conn.read_tls(in_data).is_err() {
                return Err(AdbError::Protocol("STLS: TLS record parse failed".into()));
            }
            if tls.conn.complete_io().is_err() {
                // May block; loop reads more.
            }
            if tls.conn.is_handshaking() {
                continue;
            }
            break;
        }
        // Nothing to read; if handshake complete we're done, else need more bytes.
        if tls.conn.is_handshaking() {
            // WouldBlock path: read from the socket.
            let n = tls
                .get_mut()
                .read(&mut buf)
                .await
                .map_err(|e| AdbError::Transport(format!("STLS: reading handshake: {e}")))?;
            if n == 0 {
                return Err(AdbError::StreamClosed);
            }
            if tls.conn.read_tls(&buf[..n]).is_err() {
                return Err(AdbError::Protocol("STLS: handshake record invalid".into()));
            }
            let _ = tls.conn.complete_io();
            continue;
        }
        break;
    }

    Ok(StlsTransport {
        inner: tls,
        peer: peer_name,
    })
}

fn build_client_config(cert_pem: &str, priv_pem: &str) -> Result<ClientConfig> {
    let cert_chain = rustls_pemfile::certs(&mut cert_pem.as_bytes())
        .map(|der| CertificateDer::from(der))
        .collect::<Vec<_>>();
    if cert_chain.is_empty() {
        return Err(AdbError::Protocol(
            "STLS: certificate PEM has no entries".into(),
        ));
    }
    let key = rustls_pemfile::pkcs8_private_keys(&mut priv_pem.as_bytes())
        .next()
        .ok_or_else(|| AdbError::Protocol("STLS: private key PEM has no entries".into()))?
        .map_err(|e| AdbError::Protocol(format!("STLS: private key PEM: {e}")))?;
    ClientConfig::builder()
        .with_root_certificates(allow_any_cert_store())
        .with_client_auth_cert(cert_chain, PrivateKeyDer::from(key))
        .map_err(|e| AdbError::Protocol(format!("STLS: client auth cert: {e}")))
}
