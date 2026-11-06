//! ADB `sync:` service — STAT, LIST, SEND, RECV.
//!
//! Wire format (all integers little-endian):
//! - requests: 4 ASCII bytes id + u32 (+ payload / cstring)
//! - LIST entries (`DENT`) and STAT replies carry fields inline (no separate length):
//!   `id | u32 mode | u32 size | u32 mtime | [u32 namelen | name]`
//! - `DATA`/`DONE`/`OKAY`/`FAIL` carry a u32 after the id; `DATA`'s u32 is the
//!   chunk size (≤ 64 KiB), `DONE`'s is ignored for RECV/LIST and is the mtime
//!   for SEND, `OKAY`'s is ignored, `FAIL`'s is the message length.
//!
//! Resume: the protocol has none. Pull (`RECV`) restarts from zero to a
//! `<file>.watchdog-part` temp, then renames after completion. Interrupted
//! transfers never produce a silently-truncated file. See `docs/device-setup.md`.

use std::io::Write;
use std::path::Path;

use bytes::Bytes;
use sha2::{Digest, Sha256};

use crate::connection::{Connection, Stream};
use crate::error::{AdbError, Result};
use crate::transport::Transport;

/// Maximum DATA chunk size (SYNC_DATA_MAX in AOSP).
pub const SYNC_DATA_MAX: usize = 64 * 1024;

/// Request ids (and response ids, ASCII).
pub mod id {
    /// List directory.
    pub const LIST: &[u8; 4] = b"LIST";
    /// Retrieve a file.
    pub const RECV: &[u8; 4] = b"RECV";
    /// Upload a file.
    pub const SEND: &[u8; 4] = b"SEND";
    /// Stat a file.
    pub const STAT: &[u8; 4] = b"STAT";
    /// Data chunk.
    pub const DATA: &[u8; 4] = b"DATA";
    /// End of listing / upload / download.
    pub const DONE: &[u8; 4] = b"DONE";
    /// Success reply.
    pub const OKAY: &[u8; 4] = b"OKAY";
    /// Failure reply with message.
    pub const FAIL: &[u8; 4] = b"FAIL";
    /// Directory entry in LIST replies.
    pub const DENT: &[u8; 4] = b"DENT";
}

/// STAT result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyncStat {
    /// POSIX mode (includes file type bits).
    pub mode: u32,
    /// Size in bytes.
    pub size: u64,
    /// Last-modified time (seconds since epoch).
    pub mtime: u32,
}

impl SyncStat {
    /// True when the entry is a directory.
    #[must_use]
    pub fn is_dir(&self) -> bool {
        self.mode & 0o170000 == 0o040000
    }
}

/// LIST entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncEntry {
    /// File name (not full path).
    pub name: String,
    /// POSIX mode.
    pub mode: u32,
    /// Size in bytes.
    pub size: u64,
    /// Last-modified time (seconds since epoch).
    pub mtime: u32,
}

/// Progress callback (`bytes_done`, `total_bytes` if known).
pub type Progress<'a> = Option<&'a mut dyn FnMut(u64, u64)>;

fn req_cstring(id: &[u8; 4], s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + s.len());
    out.extend_from_slice(id);
    out.extend_from_slice(&(s.len() as u32).to_le_bytes());
    out.extend_from_slice(s.as_bytes());
    out
}

async fn read_exact_stream(stream: &mut Stream, buf: &mut [u8]) -> Result<()> {
    let mut offset = 0;
    while offset < buf.len() {
        match stream.inbound.recv().await {
            Some(chunk) => {
                let take = (buf.len() - offset).min(chunk.len());
                buf[offset..offset + take].copy_from_slice(&chunk[..take]);
                offset += take;
                if take < chunk.len() {
                    return Err(AdbError::Protocol(
                        "sync chunk larger than the buffer we asked to fill".into(),
                    ));
                }
            }
            None => return Err(AdbError::StreamClosed),
        }
    }
    Ok(())
}

async fn read_id(stream: &mut Stream) -> Result<[u8; 4]> {
    let mut buf = [0u8; 4];
    read_exact_stream(stream, &mut buf).await?;
    Ok(buf)
}

async fn read_u32(stream: &mut Stream) -> Result<u32> {
    let mut buf = [0u8; 4];
    read_exact_stream(stream, &mut buf).await?;
    Ok(u32::from_le_bytes(buf))
}

/// Read a FAIL body of `len` bytes and produce an error.
async fn read_fail(stream: &mut Stream, len: u32) -> AdbError {
    let mut msg = vec![0u8; len as usize];
    if read_exact_stream(stream, &mut msg).await.is_err() {
        return AdbError::Protocol("sync FAIL with unreadable message".into());
    }
    AdbError::Protocol(format!("sync FAIL: {}", String::from_utf8_lossy(&msg)))
}

/// `STAT <path>` — metadata for one file.
pub async fn stat<T: Transport>(connection: &Connection<T>, path: &str) -> Result<SyncStat> {
    let mut stream = connection.open_stream("sync:").await?;
    stream.write(Bytes::from(req_cstring(id::STAT, path))).await?;
    let rid = read_id(&mut stream).await?;
    match &rid {
        x if x == id::STAT => {
            let mode = read_u32(&mut stream).await?;
            let size = u64::from(read_u32(&mut stream).await?);
            let mtime = read_u32(&mut stream).await?;
            Ok(SyncStat { mode, size, mtime })
        }
        x if x == id::FAIL => {
            let len = read_u32(&mut stream).await?;
            Err(read_fail(&mut stream, len).await)
        }
        other => Err(AdbError::Protocol(format!(
            "expected STAT reply, got {:?}",
            String::from_utf8_lossy(other)
        ))),
    }
}

/// `LIST <path>` — directory entries.
pub async fn list<T: Transport>(connection: &Connection<T>, path: &str) -> Result<Vec<SyncEntry>> {
    let mut stream = connection.open_stream("sync:").await?;
    stream.write(Bytes::from(req_cstring(id::LIST, path))).await?;
    let mut out = Vec::new();
    loop {
        let rid = read_id(&mut stream).await?;
        match &rid {
            x if x == id::DENT => {
                let mode = read_u32(&mut stream).await?;
                let size = u64::from(read_u32(&mut stream).await?);
                let mtime = read_u32(&mut stream).await?;
                let name_len = read_u32(&mut stream).await? as usize;
                if name_len > 8192 {
                    return Err(AdbError::Protocol(format!("LIST entry name too long ({name_len})")));
                }
                let mut name = vec![0u8; name_len];
                read_exact_stream(&mut stream, &mut name).await?;
                out.push(SyncEntry {
                    name: String::from_utf8_lossy(&name).into_owned(),
                    mode,
                    size,
                    mtime,
                });
            }
            x if x == id::DONE => break,
            x if x == id::FAIL => {
                let len = read_u32(&mut stream).await?;
                return Err(read_fail(&mut stream, len).await);
            }
            other => {
                return Err(AdbError::Protocol(format!(
                    "unexpected LIST reply {:?}",
                    String::from_utf8_lossy(other)
                )));
            }
        }
    }
    Ok(out)
}

/// Result of a pull operation.
#[derive(Debug, Clone)]
pub struct PullResult {
    /// Local path of the file.
    pub path: std::path::PathBuf,
    /// Bytes written.
    pub bytes: u64,
    /// Lowercase hex SHA-256 of the file.
    pub sha256: String,
}

/// `RECV <path>` — download a file with SHA-256 verification.
///
/// Streams to `<local>.watchdog-part` and renames on completion; an
/// interrupted pull leaves the `.part` file, which is discarded on retry.
pub async fn pull<T: Transport>(
    connection: &Connection<T>,
    remote_path: &str,
    local_path: &Path,
    mut progress: Progress<'_>,
) -> Result<PullResult> {
    let part_path = match local_path.extension().and_then(|e| e.to_str()) {
        Some(ext) => local_path.with_extension(format!("{ext}.watchdog-part")),
        None => {
            let mut name = local_path.file_name().unwrap_or_default().to_os_string();
            name.push(".watchdog-part");
            local_path.with_file_name(name)
        }
    };

    let mut stream = connection.open_stream("sync:").await?;
    stream.write(Bytes::from(req_cstring(id::RECV, remote_path))).await?;

    let mut file = std::fs::File::create(&part_path)?;
    let mut hasher = Sha256::new();
    let mut total: u64 = 0;

    loop {
        let rid = read_id(&mut stream).await?;
        match &rid {
            x if x == id::DATA => {
                let len = read_u32(&mut stream).await? as usize;
                if len > SYNC_DATA_MAX {
                    return Err(AdbError::Protocol(format!("DATA chunk too large ({len})")));
                }
                let mut chunk = vec![0u8; len];
                read_exact_stream(&mut stream, &mut chunk).await?;
                file.write_all(&chunk)?;
                hasher.update(&chunk);
                total += len as u64;
                if let Some(cb) = progress.as_deref_mut() {
                    cb(total, 0);
                }
            }
            x if x == id::DONE => {
                let _ = read_u32(&mut stream).await?; // ignored per spec
                break;
            }
            x if x == id::FAIL => {
                let len = read_u32(&mut stream).await?;
                drop(file);
                let _ = std::fs::remove_file(&part_path);
                return Err(read_fail(&mut stream, len).await);
            }
            other => {
                return Err(AdbError::Protocol(format!(
                    "unexpected RECV reply {:?}",
                    String::from_utf8_lossy(other)
                )));
            }
        }
    }
    drop(file);
    std::fs::rename(&part_path, local_path)?;
    Ok(PullResult {
        path: local_path.to_path_buf(),
        bytes: total,
        sha256: hex::encode(hasher.finalize()),
    })
}

/// `SEND <path>,<mode>` — upload a file.
///
/// The remote file is truncated before the first byte arrives; on failure the
/// remote copy is incomplete and must be re-sent.
pub async fn push<T: Transport>(
    connection: &Connection<T>,
    local_path: &Path,
    remote_path: &str,
    mode: u32,
    mtime: u32,
    mut progress: Progress<'_>,
) -> Result<u64> {
    let file_bytes = std::fs::metadata(local_path)?.len();
    let mut stream = connection.open_stream("sync:").await?;

    let spec = format!("{remote_path},{mode}");
    stream.write(Bytes::from(req_cstring(id::SEND, &spec))).await?;

    let mut file = std::fs::File::open(local_path)?;
    let mut buf = vec![0u8; SYNC_DATA_MAX];
    let mut sent: u64 = 0;
    loop {
        use std::io::Read;
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        let mut packet = Vec::with_capacity(8 + n);
        packet.extend_from_slice(id::DATA);
        packet.extend_from_slice(&(n as u32).to_le_bytes());
        packet.extend_from_slice(&buf[..n]);
        stream.write(Bytes::from(packet)).await?;
        sent += n as u64;
        if let Some(cb) = progress.as_deref_mut() {
            cb(sent, file_bytes);
        }
    }

    let mut done = Vec::with_capacity(8);
    done.extend_from_slice(id::DONE);
    done.extend_from_slice(&mtime.to_le_bytes());
    stream.write(Bytes::from(done)).await?;

    let rid = read_id(&mut stream).await?;
    match &rid {
        x if x == id::OKAY => {
            let _ = read_u32(&mut stream).await?; // ignored
            Ok(sent)
        }
        x if x == id::FAIL => {
            let len = read_u32(&mut stream).await?;
            Err(read_fail(&mut stream, len).await)
        }
        other => Err(AdbError::Protocol(format!(
            "unexpected SEND reply {:?}",
            String::from_utf8_lossy(other)
        ))),
    }
}
