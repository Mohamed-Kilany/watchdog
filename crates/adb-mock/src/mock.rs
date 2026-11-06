//! Scripted fake device: queued byte stream for reads, recording for writes.

use std::collections::VecDeque;

use async_trait::async_trait;

use watchdog_adb::transport::Transport;
use watchdog_adb::Result;

/// What the fake should do for one request/response round.
#[derive(Debug, Clone)]
pub enum ScriptStep {
    /// Serve these bytes to the reader (in order).
    Serve(Vec<u8>),
    /// Close the connection after these reads are consumed.
    CloseAfter(Vec<u8>),
}

/// In-memory scripted transport implementing [`Transport`].
///
/// Reads serve from the concatenated [`ScriptStep::Serve`] payloads; when the
/// script is exhausted, reads fail with `StreamClosed` — tests must script
/// every expected byte. Writes are recorded for later assertions.
#[derive(Debug)]
pub struct MockTransport {
    name: String,
    incoming: Vec<u8>,
    read_pos: usize,
    close_after_reads: bool,
    /// Everything written so far.
    pub written: Vec<u8>,
}

impl Default for MockTransport {
    fn default() -> Self {
        Self::new("mock")
    }
}

impl MockTransport {
    /// Empty script.
    #[must_use]
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            incoming: Vec::new(),
            read_pos: 0,
            close_after_reads: true,
            written: Vec::new(),
        }
    }

    /// Append bytes to the readable script.
    #[must_use]
    pub fn script_bytes(mut self, bytes: &[u8]) -> Self {
        self.incoming.extend_from_slice(bytes);
        self
    }

    /// Append a raw packet to the readable script.
    #[must_use]
    pub fn script_packet(mut self, packet: &watchdog_adb::packet::Packet) -> Self {
        self.incoming.extend_from_slice(&packet.encode());
        self
    }

    /// Keep serving reads forever (return zeros) instead of EOF'ing.
    #[must_use]
    pub fn keep_open(mut self) -> Self {
        self.close_after_reads = false;
        self
    }
}

#[async_trait]
impl Transport for MockTransport {
    fn name(&self) -> String {
        format!("mock:{}", self.name)
    }

    async fn read_exact(&mut self, buf: &mut [u8]) -> Result<()> {
        for slot in buf.iter_mut() {
            if self.read_pos < self.incoming.len() {
                *slot = self.incoming[self.read_pos];
                self.read_pos += 1;
            } else if self.close_after_reads {
                return Err(watchdog_adb::AdbError::StreamClosed);
            } else {
                *slot = 0;
            }
        }
        Ok(())
    }

    async fn write_all(&mut self, buf: &[u8]) -> Result<()> {
        self.written.extend_from_slice(buf);
        Ok(())
    }

    async fn close(&mut self) -> Result<()> {
        self.read_pos = self.incoming.len();
        Ok(())
    }
}

/// A fake device with a name and scripted transport (extensible harness).
#[derive(Debug)]
pub struct MockDevice {
    /// Device banner, e.g. `device::1a2b3c4d:test device`.
    pub banner: String,
    /// Scripted transport returned by [`MockDevice::open`].
    pub transport: MockTransport,
    /// Recorded peer-info packets once protocol-level helpers land (M2-T14).
    pub seen_packets: VecDeque<String>,
}

impl Default for MockDevice {
    fn default() -> Self {
        Self::new("device::mock:001122334455:mock device")
    }
}

impl MockDevice {
    /// New device with the given CNXN banner.
    #[must_use]
    pub fn new(banner: &str) -> Self {
        Self {
            banner: banner.to_string(),
            transport: MockTransport::new("adb"),
            seen_packets: VecDeque::new(),
        }
    }

    /// Consume the scripted transport.
    #[must_use]
    pub fn open(self) -> MockTransport {
        self.transport
    }
}
