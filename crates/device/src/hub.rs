//! Frame receiver and bounded broadcast hub (M3-T14).
//!
//! The hub owns one authenticated app session, reads `FRAME` messages, puts
//! them back in sequence order (dropping duplicates and counting gaps) and
//! publishes them to any number of subscribers through a bounded
//! [`tokio::sync::broadcast`] channel. Publishing never blocks: a slow
//! subscriber only lags and loses old frames, so the UI, MCP and recorder can
//! consume the stream independently.
//!
//! While reading, the hub sends `PING` once per second; `PONG` carries the
//! app's `CLOCK_BOOTTIME` timestamp, which gives a clock-offset estimate so
//! every frame can report an estimated glass-to-sensor latency.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use serde::Serialize;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

use watchdog_protocol::{FramePacket, Message, PingPong, Rotation};

use crate::session::{LiveSession, Polled};

/// Default number of frames kept for slow subscribers.
pub const DEFAULT_CHANNEL_CAPACITY: usize = 8;
/// Default number of out-of-order frames buffered before resynchronizing.
pub const DEFAULT_MAX_REORDER: usize = 16;
/// Default time an out-of-order frame may wait for its predecessors.
pub const DEFAULT_REORDER_HOLD: Duration = Duration::from_millis(250);
/// Default interval between latency probes.
pub const DEFAULT_PING_INTERVAL: Duration = Duration::from_secs(1);
/// Default window used to compute FPS and bitrate.
pub const DEFAULT_STATS_WINDOW: Duration = Duration::from_secs(2);

/// Poll granularity while waiting for app messages.
const POLL_INTERVAL: Duration = Duration::from_millis(50);
/// Oldest latency probes kept for `PONG` matching.
const MAX_PENDING_PINGS: usize = 4;
/// Latencies above this are treated as an unusable clock-offset sample.
const MAX_PLAUSIBLE_LATENCY_NS: i128 = 60_000_000_000;

/// Tunables for [`FrameHub`].
#[derive(Debug, Clone)]
pub struct HubConfig {
    /// Bounded broadcast capacity per subscriber.
    pub channel_capacity: usize,
    /// Out-of-order frames buffered before the hub gives up on a gap.
    pub max_reorder: usize,
    /// How long an out-of-order frame may wait for missing predecessors.
    pub reorder_hold: Duration,
    /// Interval between latency probes.
    pub ping_interval: Duration,
    /// Sliding window used to compute FPS and bitrate.
    pub stats_window: Duration,
}

impl Default for HubConfig {
    fn default() -> Self {
        Self {
            channel_capacity: DEFAULT_CHANNEL_CAPACITY,
            max_reorder: DEFAULT_MAX_REORDER,
            reorder_hold: DEFAULT_REORDER_HOLD,
            ping_interval: DEFAULT_PING_INTERVAL,
            stats_window: DEFAULT_STATS_WINDOW,
        }
    }
}

/// One JPEG frame published to subscribers.
#[derive(Debug, Clone)]
pub struct ReceivedFrame {
    /// App sequence number (gaps mean frames were dropped before the host).
    pub seq: u64,
    /// `CaptureResult.SENSOR_TIMESTAMP` (CLOCK_BOOTTIME, ns).
    pub sensor_timestamp_ns: u64,
    /// JPEG width in pixels.
    pub width: u32,
    /// JPEG height in pixels.
    pub height: u32,
    /// Display rotation hint.
    pub rotation: Rotation,
    /// Frame flags (bit 0: color transform present; bit 1: snapshot).
    pub flags: u8,
    /// Complete JPEG bytes.
    pub jpeg: Bytes,
    /// Host time at which the frame was received.
    pub received_at: Instant,
    /// Estimated sensor-to-host latency, when a clock offset is known.
    pub latency_ms: Option<f64>,
}

/// Rolling stream statistics.
#[derive(Debug, Clone, Default, Serialize)]
pub struct StreamStats {
    /// Frames published since the hub started.
    pub frames: u64,
    /// Sequence numbers never delivered (gaps before or after reordering).
    pub dropped: u64,
    /// Duplicate or stale frames discarded.
    pub duplicates: u64,
    /// Frames per second over the stats window.
    pub fps: f64,
    /// Kilobits per second over the stats window.
    pub bitrate_kbps: f64,
    /// Most recent estimated latency.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<f64>,
    /// Most recent probe round-trip time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rtt_ms: Option<f64>,
    /// Sequence number of the most recently published frame.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_seq: Option<u64>,
    /// Current number of broadcast subscribers.
    pub subscribers: usize,
    /// Whether the hub stopped reading (app closed or error).
    pub closed: bool,
    /// Last app or transport error, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

/// Bounded frame broadcaster with rolling stats.
///
/// Kept separate from [`FrameHub`] so publishing and the slow-subscriber
/// guarantee can be unit-tested without a device.
#[derive(Clone)]
pub struct FrameBroadcaster {
    sender: broadcast::Sender<Arc<ReceivedFrame>>,
    stats: Arc<Mutex<StatsState>>,
}

impl FrameBroadcaster {
    /// Create a broadcaster with `capacity` frames of backlog per subscriber.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self::with_window(capacity, DEFAULT_STATS_WINDOW)
    }

    /// Create a broadcaster with a custom stats window.
    #[must_use]
    pub fn with_window(capacity: usize, stats_window: Duration) -> Self {
        let (sender, _receiver) = broadcast::channel(capacity.max(1));
        Self {
            sender,
            stats: Arc::new(Mutex::new(StatsState::new(stats_window))),
        }
    }

    /// Subscribe to future frames.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<Arc<ReceivedFrame>> {
        self.sender.subscribe()
    }

    /// Publish one frame. Never blocks; lagging subscribers miss old frames.
    pub fn publish(&self, frame: ReceivedFrame) {
        {
            let mut stats = lock_stats(&self.stats);
            stats.record(&frame);
        }
        let _ = self.sender.send(Arc::new(frame));
    }

    /// Add frames skipped by the sequencer to the dropped counter.
    pub fn add_dropped(&self, count: u64) {
        lock_stats(&self.stats).dropped += count;
    }

    /// Add duplicate/stale frames to the duplicate counter.
    pub fn add_duplicates(&self, count: u64) {
        lock_stats(&self.stats).duplicates += count;
    }

    /// Record the latest probe round-trip time.
    pub fn set_rtt(&self, rtt_ms: f64) {
        lock_stats(&self.stats).rtt_ms = Some(rtt_ms);
    }

    /// Record a non-fatal app error.
    pub fn set_error(&self, message: String) {
        lock_stats(&self.stats).last_error = Some(message);
    }

    /// Mark the hub as no longer reading.
    pub fn mark_closed(&self, reason: Option<String>) {
        let mut stats = lock_stats(&self.stats);
        stats.closed = true;
        if reason.is_some() {
            stats.last_error = reason;
        }
    }

    /// Snapshot the current stats.
    #[must_use]
    pub fn stats(&self) -> StreamStats {
        let mut stats = lock_stats(&self.stats);
        stats.snapshot(self.sender.receiver_count())
    }

    /// Number of live subscribers.
    #[must_use]
    pub fn receiver_count(&self) -> usize {
        self.sender.receiver_count()
    }
}

/// A running frame hub; dropping it stops reading and forwarding.
pub struct FrameHub {
    broadcaster: FrameBroadcaster,
    task: JoinHandle<()>,
}

impl FrameHub {
    /// Spawn a hub that reads `live` until the session closes.
    ///
    /// Must be called from within a Tokio runtime.
    pub fn spawn(live: LiveSession, config: HubConfig) -> Self {
        let broadcaster =
            FrameBroadcaster::with_window(config.channel_capacity, config.stats_window);
        let task_broadcaster = broadcaster.clone();
        let task = tokio::spawn(async move {
            run(live, &config, &task_broadcaster).await;
        });
        Self { broadcaster, task }
    }

    /// Subscribe to frames from this hub.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<Arc<ReceivedFrame>> {
        self.broadcaster.subscribe()
    }

    /// Snapshot of the rolling stats.
    #[must_use]
    pub fn stats(&self) -> StreamStats {
        self.broadcaster.stats()
    }
}

impl Drop for FrameHub {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn run(mut live: LiveSession, config: &HubConfig, out: &FrameBroadcaster) {
    let epoch = Instant::now();
    let mut sequencer = Sequencer::new(config.max_reorder, config.reorder_hold);
    let mut pinger = Pinger::new(config.ping_interval);

    loop {
        match live.session().poll(POLL_INTERVAL).await {
            Ok(Polled::Message(message)) => match *message {
                Message::Frame(frame) => {
                    let now = Instant::now();
                    publish_due(out, &mut sequencer, &pinger, frame, now, epoch);
                    out.add_dropped(sequencer.take_dropped());
                    out.add_duplicates(sequencer.take_duplicates());
                }
                Message::Pong(pong) => {
                    if let Some(rtt_ms) = pinger.on_pong(&pong, Instant::now(), epoch) {
                        out.set_rtt(rtt_ms);
                    }
                }
                Message::Error(error) => {
                    out.set_error(format!("{} ({})", error.message, error.code));
                }
                Message::Bye(bye) => {
                    out.mark_closed(Some(format!("app closed the session: {}", bye.reason)));
                    break;
                }
                _ => {}
            },
            Ok(Polled::Timeout) => {
                let now = Instant::now();
                let expired = sequencer.flush_expired(now);
                for frame in expired {
                    let latency = pinger.latency_ms(frame.sensor_timestamp_ns, now, epoch);
                    out.publish(ReceivedFrame::from_packet(frame, now, latency));
                }
                out.add_dropped(sequencer.take_dropped());
            }
            Ok(Polled::Closed) => {
                out.mark_closed(None);
                break;
            }
            Err(error) => {
                out.mark_closed(Some(error.to_string()));
                break;
            }
        }

        if let Some(ping) = pinger.due(Instant::now()) {
            if let Err(error) = live.session().send(&ping).await {
                out.mark_closed(Some(error.to_string()));
                break;
            }
        }
    }
}

fn publish_due(
    out: &FrameBroadcaster,
    sequencer: &mut Sequencer,
    pinger: &Pinger,
    frame: FramePacket,
    now: Instant,
    epoch: Instant,
) {
    for frame in sequencer.push(frame, now) {
        let latency = pinger.latency_ms(frame.sensor_timestamp_ns, now, epoch);
        out.publish(ReceivedFrame::from_packet(frame, now, latency));
    }
}

impl ReceivedFrame {
    fn from_packet(frame: FramePacket, received_at: Instant, latency_ms: Option<f64>) -> Self {
        Self {
            seq: frame.seq,
            sensor_timestamp_ns: frame.sensor_timestamp_ns,
            width: frame.width,
            height: frame.height,
            rotation: frame.rotation,
            flags: frame.flags,
            jpeg: frame.jpeg,
            received_at,
            latency_ms,
        }
    }
}

struct StatsState {
    frames: u64,
    dropped: u64,
    duplicates: u64,
    last_seq: Option<u64>,
    window: VecDeque<(Instant, usize)>,
    window_span: Duration,
    latency_ms: Option<f64>,
    rtt_ms: Option<f64>,
    closed: bool,
    last_error: Option<String>,
}

impl StatsState {
    fn new(window_span: Duration) -> Self {
        Self {
            frames: 0,
            dropped: 0,
            duplicates: 0,
            last_seq: None,
            window: VecDeque::new(),
            window_span,
            latency_ms: None,
            rtt_ms: None,
            closed: false,
            last_error: None,
        }
    }

    fn record(&mut self, frame: &ReceivedFrame) {
        self.frames += 1;
        self.last_seq = Some(frame.seq);
        if let Some(latency) = frame.latency_ms {
            self.latency_ms = Some(latency);
        }
        self.window.push_back((frame.received_at, frame.jpeg.len()));
        self.prune(frame.received_at);
    }

    fn prune(&mut self, now: Instant) {
        while let Some((at, _)) = self.window.front() {
            if now.saturating_duration_since(*at) > self.window_span {
                self.window.pop_front();
            } else {
                break;
            }
        }
    }

    fn snapshot(&mut self, subscribers: usize) -> StreamStats {
        // Prune against the wall clock so FPS falls to zero when frames stop.
        let now = Instant::now();
        self.prune(now);
        let seconds = self.window_span.as_secs_f64().max(f64::EPSILON);
        let bytes: usize = self.window.iter().map(|(_, len)| len).sum();
        StreamStats {
            frames: self.frames,
            dropped: self.dropped,
            duplicates: self.duplicates,
            fps: self.window.len() as f64 / seconds,
            bitrate_kbps: bytes as f64 * 8.0 / seconds / 1000.0,
            latency_ms: self.latency_ms,
            rtt_ms: self.rtt_ms,
            last_seq: self.last_seq,
            subscribers,
            closed: self.closed,
            last_error: self.last_error.clone(),
        }
    }
}

/// Reorders frames by sequence number and counts unrecoverable gaps.
struct Sequencer {
    next: Option<u64>,
    buffer: BTreeMap<u64, (Instant, FramePacket)>,
    max_reorder: usize,
    hold: Duration,
    dropped: u64,
    duplicates: u64,
}

impl Sequencer {
    fn new(max_reorder: usize, hold: Duration) -> Self {
        Self {
            next: None,
            buffer: BTreeMap::new(),
            max_reorder: max_reorder.max(1),
            hold,
            dropped: 0,
            duplicates: 0,
        }
    }

    fn push(&mut self, frame: FramePacket, now: Instant) -> Vec<FramePacket> {
        let mut out = Vec::new();
        match self.next {
            None => {
                self.next = Some(frame.seq.saturating_add(1));
                out.push(frame);
            }
            Some(next) if frame.seq < next => self.duplicates += 1,
            Some(next) if frame.seq == next => {
                self.next = Some(next.saturating_add(1));
                out.push(frame);
                self.drain(&mut out);
            }
            Some(_) => {
                self.buffer.insert(frame.seq, (now, frame));
                if self.buffer.len() > self.max_reorder {
                    self.resync_to_smallest();
                    self.drain(&mut out);
                }
            }
        }
        out
    }

    fn flush_expired(&mut self, now: Instant) -> Vec<FramePacket> {
        let mut out = Vec::new();
        while let Some(&seq) = self.buffer.keys().next() {
            let expired = self
                .buffer
                .get(&seq)
                .is_some_and(|(at, _)| now.saturating_duration_since(*at) >= self.hold);
            if !expired {
                break;
            }
            self.resync_to_smallest();
            self.drain(&mut out);
        }
        out
    }

    fn resync_to_smallest(&mut self) {
        let Some(&smallest) = self.buffer.keys().next() else {
            return;
        };
        if let Some(next) = self.next {
            if smallest > next {
                self.dropped += smallest - next;
            }
        }
        self.next = Some(smallest);
    }

    fn drain(&mut self, out: &mut Vec<FramePacket>) {
        while let Some(next) = self.next {
            let Some((_, frame)) = self.buffer.remove(&next) else {
                break;
            };
            out.push(frame);
            self.next = Some(next.saturating_add(1));
        }
    }

    fn take_dropped(&mut self) -> u64 {
        std::mem::take(&mut self.dropped)
    }

    fn take_duplicates(&mut self) -> u64 {
        std::mem::take(&mut self.duplicates)
    }
}

/// Latency probes and clock-offset estimation.
struct Pinger {
    interval: Duration,
    next_ping: Instant,
    pending: VecDeque<(String, Instant)>,
    best_rtt: Option<Duration>,
    offset_ns: Option<i128>,
    rtt_ms: Option<f64>,
    nonce: u64,
}

impl Pinger {
    fn new(interval: Duration) -> Self {
        Self {
            interval,
            next_ping: Instant::now(),
            pending: VecDeque::new(),
            best_rtt: None,
            offset_ns: None,
            rtt_ms: None,
            nonce: 0,
        }
    }

    fn due(&mut self, now: Instant) -> Option<Message> {
        if now < self.next_ping {
            return None;
        }
        self.next_ping = now + self.interval;
        self.nonce = self.nonce.saturating_add(1);
        let nonce = format!("p{}", self.nonce);
        self.pending.push_back((nonce.clone(), now));
        while self.pending.len() > MAX_PENDING_PINGS {
            self.pending.pop_front();
        }
        Some(Message::Ping(PingPong {
            ts_ms: now_ms(),
            nonce: Some(nonce),
            app_ts_ns: None,
        }))
    }

    /// Feed a `PONG`; returns the measured round-trip time in milliseconds.
    fn on_pong(&mut self, pong: &PingPong, now: Instant, epoch: Instant) -> Option<f64> {
        let nonce = pong.nonce.as_deref()?;
        let sent = self
            .pending
            .iter()
            .find(|(pending, _)| pending == nonce)
            .map(|(_, sent)| *sent)?;
        let rtt = now.saturating_duration_since(sent);
        let app_ns = i128::from(pong.app_ts_ns?);
        let host_ns = now.saturating_duration_since(epoch).as_nanos() as i128;
        let offset = app_ns + (rtt.as_nanos() as i128) / 2 - host_ns;
        // Keep the least-delayed sample: it has the smallest asymmetry error.
        if self.best_rtt.is_none_or(|best| rtt < best) {
            self.best_rtt = Some(rtt);
            self.offset_ns = Some(offset);
        }
        self.rtt_ms = Some(rtt.as_secs_f64() * 1000.0);
        self.rtt_ms
    }

    /// Estimate sensor-to-host latency in milliseconds.
    fn latency_ms(&self, sensor_timestamp_ns: u64, now: Instant, epoch: Instant) -> Option<f64> {
        let offset = self.offset_ns?;
        let host_ns = now.saturating_duration_since(epoch).as_nanos() as i128;
        let latency_ns = host_ns + offset - i128::from(sensor_timestamp_ns);
        if !(0..=MAX_PLAUSIBLE_LATENCY_NS).contains(&latency_ns) {
            return None;
        }
        Some(latency_ns as f64 / 1_000_000.0)
    }
}

fn lock_stats(stats: &Mutex<StatsState>) -> std::sync::MutexGuard<'_, StatsState> {
    stats.lock().unwrap_or_else(PoisonError::into_inner)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::broadcast::error::TryRecvError;

    fn packet(seq: u64) -> FramePacket {
        FramePacket {
            seq,
            sensor_timestamp_ns: seq * 1_000_000,
            width: 1280,
            height: 720,
            rotation: Rotation::R0,
            flags: 0,
            jpeg: Bytes::from_static(&[0xFF, 0xD8, 0xFF, 0xD9]),
        }
    }

    fn sequence(frames: Vec<FramePacket>) -> Vec<u64> {
        frames.into_iter().map(|frame| frame.seq).collect()
    }

    #[test]
    fn sequencer_delivers_in_order() {
        let mut sequencer = Sequencer::new(4, Duration::from_millis(100));
        let now = Instant::now();
        let mut out = Vec::new();
        for seq in 0..4 {
            out.extend(sequencer.push(packet(seq), now));
        }
        assert_eq!(sequence(out), vec![0, 1, 2, 3]);
        assert_eq!(sequencer.take_dropped(), 0);
        assert_eq!(sequencer.take_duplicates(), 0);
    }

    #[test]
    fn sequencer_drops_duplicates_and_stale_frames() {
        let mut sequencer = Sequencer::new(4, Duration::from_millis(100));
        let now = Instant::now();
        assert_eq!(sequence(sequencer.push(packet(5), now)), vec![5]);
        assert!(sequencer.push(packet(5), now).is_empty());
        assert!(sequencer.push(packet(2), now).is_empty());
        assert_eq!(sequencer.take_duplicates(), 2);
    }

    #[test]
    fn sequencer_buffers_out_of_order_frames() {
        let mut sequencer = Sequencer::new(4, Duration::from_millis(100));
        let now = Instant::now();
        assert_eq!(sequence(sequencer.push(packet(0), now)), vec![0]);
        assert!(sequencer.push(packet(2), now).is_empty());
        assert!(sequencer.push(packet(3), now).is_empty());
        assert_eq!(sequence(sequencer.push(packet(1), now)), vec![1, 2, 3]);
        assert_eq!(sequencer.take_dropped(), 0);
    }

    #[test]
    fn sequencer_resynchronizes_when_the_buffer_is_full() {
        let mut sequencer = Sequencer::new(2, Duration::from_secs(10));
        let now = Instant::now();
        assert_eq!(sequence(sequencer.push(packet(0), now)), vec![0]);
        assert!(sequencer.push(packet(2), now).is_empty());
        assert!(sequencer.push(packet(3), now).is_empty());
        // A third out-of-order frame exceeds max_reorder: seq 1 is given up.
        assert_eq!(sequence(sequencer.push(packet(4), now)), vec![2, 3, 4]);
        assert_eq!(sequencer.take_dropped(), 1);
    }

    #[test]
    fn sequencer_flushes_a_stalled_gap_after_the_hold() {
        let mut sequencer = Sequencer::new(4, Duration::from_millis(100));
        let start = Instant::now();
        assert_eq!(sequence(sequencer.push(packet(0), start)), vec![0]);
        assert!(sequencer.push(packet(2), start).is_empty());
        assert!(sequencer
            .flush_expired(start + Duration::from_millis(50))
            .is_empty());
        assert_eq!(
            sequence(sequencer.flush_expired(start + Duration::from_millis(150))),
            vec![2]
        );
        assert_eq!(sequencer.take_dropped(), 1);
    }

    #[test]
    fn slow_subscriber_lags_without_blocking_the_publisher() {
        let broadcaster = FrameBroadcaster::new(1);
        let mut slow = broadcaster.subscribe();
        for seq in 0..5 {
            broadcaster.publish(ReceivedFrame::from_packet(
                packet(seq),
                Instant::now(),
                None,
            ));
        }
        match slow.try_recv() {
            Err(TryRecvError::Lagged(missed)) => assert_eq!(missed, 4),
            other => panic!("expected a lagged subscriber, got {other:?}"),
        }
        let stats = broadcaster.stats();
        assert_eq!(stats.frames, 5);
        assert_eq!(stats.last_seq, Some(4));
    }

    #[test]
    fn stats_count_frames_and_bytes() {
        let broadcaster = FrameBroadcaster::with_window(4, Duration::from_secs(2));
        let _receiver = broadcaster.subscribe();
        for seq in 0..3 {
            broadcaster.publish(ReceivedFrame::from_packet(
                packet(seq),
                Instant::now(),
                Some(12.0),
            ));
        }
        let stats = broadcaster.stats();
        assert_eq!(stats.frames, 3);
        assert_eq!(stats.subscribers, 1);
        assert_eq!(stats.latency_ms, Some(12.0));
        assert!(stats.fps > 0.0);
        assert!(stats.bitrate_kbps > 0.0);
    }
}
