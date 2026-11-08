//! Scripted fake device: queued byte stream for reads, recording for writes.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use watchdog_adb::transport::{Transport, TransportReader, TransportWriter};
use watchdog_adb::Result;

/// What the fake should do for one request/response round.
#[derive(Debug, Clone)]
pub enum ScriptStep {
    /// Serve these bytes to the reader (in order).
    Serve(Vec<u8>),
    /// Close the connection after these reads are consumed.
    CloseAfter(Vec<u8>),
}

/// Cloneable inspection handle for [`MockTransport`] writes.
#[derive(Debug, Clone)]
pub struct MockTap {
    written: Arc<Mutex<Vec<u8>>>,
}

impl MockTap {
    /// Snapshot of all bytes written so far.
    #[must_use]
    pub fn written(&self) -> Vec<u8> {
        self.written.lock().expect("tap lock").clone()
    }
}

/// Read half of [`MockTransport`].
#[derive(Debug)]
pub struct MockReader {
    incoming: Vec<u8>,
    read_pos: usize,
    close_after_reads: bool,
    block_after_reads: bool,
}

/// Write half of [`MockTransport`].
#[derive(Debug)]
pub struct MockWriter {
    written: Arc<Mutex<Vec<u8>>>,
}

/// In-memory scripted transport implementing [`Transport`].
///
/// Reads serve from the concatenated script; when it is exhausted, reads fail
/// with `StreamClosed`. Writes are recorded and inspectable via [`MockTap`].
#[derive(Debug)]
pub struct MockTransport {
    name: String,
    incoming: Vec<u8>,
    read_pos: usize,
    close_after_reads: bool,
    block_after_reads: bool,
    written: Arc<Mutex<Vec<u8>>>,
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
            block_after_reads: false,
            written: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Clone a tap that observes everything written.
    #[must_use]
    pub fn tap(&self) -> MockTap {
        MockTap {
            written: Arc::clone(&self.written),
        }
    }

    /// Append bytes to the readable script.
    #[must_use]
    pub fn script_bytes(mut self, bytes: &[u8]) -> Self {
        self.incoming.extend_from_slice(bytes);
        self
    }

    /// Append a packet to the readable script.
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

    /// Block forever once the script is exhausted (simulates an idle device
    /// that has nothing to say while the host writes).
    #[must_use]
    pub fn block_after_script(mut self) -> Self {
        self.block_after_reads = true;
        self
    }
}

#[async_trait]
impl TransportReader for MockTransport {
    async fn read_exact(&mut self, buf: &mut [u8]) -> Result<()> {
        for slot in buf.iter_mut() {
            if self.read_pos < self.incoming.len() {
                *slot = self.incoming[self.read_pos];
                self.read_pos += 1;
            } else if self.block_after_reads {
                std::future::pending::<()>().await;
            } else if self.close_after_reads {
                return Err(watchdog_adb::AdbError::StreamClosed);
            } else {
                *slot = 0;
            }
        }
        Ok(())
    }
}

#[async_trait]
impl TransportWriter for MockTransport {
    async fn write_all(&mut self, buf: &[u8]) -> Result<()> {
        self.written
            .lock()
            .expect("tap lock")
            .extend_from_slice(buf);
        Ok(())
    }
}

#[async_trait]
impl Transport for MockTransport {
    fn name(&self) -> String {
        format!("mock:{}", self.name)
    }

    type Reader = MockReader;
    type Writer = MockWriter;

    fn split(self) -> (Self::Reader, Self::Writer) {
        (
            MockReader {
                incoming: self.incoming,
                read_pos: self.read_pos,
                close_after_reads: self.close_after_reads,
                block_after_reads: self.block_after_reads,
            },
            MockWriter {
                written: self.written,
            },
        )
    }

    async fn close(&mut self) -> Result<()> {
        self.read_pos = self.incoming.len();
        Ok(())
    }
}

#[async_trait]
impl TransportReader for MockReader {
    async fn read_exact(&mut self, buf: &mut [u8]) -> Result<()> {
        for slot in buf.iter_mut() {
            if self.read_pos < self.incoming.len() {
                *slot = self.incoming[self.read_pos];
                self.read_pos += 1;
            } else if self.block_after_reads {
                std::future::pending::<()>().await;
            } else if self.close_after_reads {
                return Err(watchdog_adb::AdbError::StreamClosed);
            } else {
                *slot = 0;
            }
        }
        Ok(())
    }
}

#[async_trait]
impl TransportWriter for MockWriter {
    async fn write_all(&mut self, buf: &[u8]) -> Result<()> {
        self.written
            .lock()
            .expect("tap lock")
            .extend_from_slice(buf);
        Ok(())
    }
}
