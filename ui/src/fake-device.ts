import type {
  AnalysisSubmission,
  ApplyResult,
  BurstOptions,
  CaptureOutcome,
  ControlChange,
  ControlEntry,
  ControlRegion,
  ControlState,
  ControlValue,
  DeviceEvent,
  HostLogLine,
  HostRetention,
  HostSettings,
  MediaItem,
  RecordOptions,
  TransferProgress,
} from "./types";

type Listener = (state: ControlState, changes: ControlChange[]) => void;

/** Tiny synthetic clip shipped with the demo (Vite hashes and serves it). */
const DEMO_CLIP_URL = new URL("./assets/demo-clip.webm", import.meta.url).href;

const AUTO = {
  iso: 100,
  exposure_time_ns: 16_666_666,
  frame_duration_ns: 33_333_000,
  focus_distance: 2.5,
  zoom_actual: 1,
  fps_actual: 30,
} as const;

const AUTO_EXPOSURE_IDS = [
  "iso",
  "exposure_time_ns",
  "frame_duration_ns",
] as const;

interface RangeSpec {
  min: number;
  max: number;
  step: number;
}

const RANGES: Record<string, RangeSpec> = {
  iso: { min: 50, max: 3200, step: 50 },
  exposure_time_ns: { min: 1_000_000, max: 100_000_000, step: 1_000_000 },
  frame_duration_ns: { min: 1_000_000, max: 100_000_000, step: 1_000 },
  ae_compensation: { min: -2, max: 2, step: 0.5 },
  focus_distance: { min: 0, max: 10, step: 0.1 },
  zoom_ratio: { min: 1, max: 8, step: 0.1 },
  filter_density: { min: 0, max: 1, step: 0.05 },
  jpeg_quality: { min: 1, max: 100, step: 1 },
  video_bitrate: { min: 100_000, max: 100_000_000, step: 100_000 },
};

const ENUMS: Record<string, number[]> = {
  control_mode: [0, 1, 2],
  ae_mode: [0, 1],
  anti_banding: [0, 1, 2, 3],
  af_mode: [0, 1, 2, 3, 4],
  awb_mode: [0, 1, 2, 3, 4, 5, 6, 7],
  effect: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
  aberration_correction: [0, 1, 2],
  optical_stabilization: [0, 1],
  video_stabilization: [0, 1, 2],
  lens_shading: [0, 1],
  distortion_correction: [0, 1, 2],
  flash_mode: [0, 1, 2],
  scene_mode: [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18,
  ],
  noise_reduction: [0, 1, 2, 3, 4],
  edge_enhancement: [0, 1, 2, 3],
  hot_pixel: [0, 1, 2],
  tonemap: [0, 1, 2, 3, 4],
  tonemap_preset_curve: [0, 1],
  test_pattern: [0, 1, 2, 3, 4],
  stream_rotation: [0, 90, 180, 270],
};

const BOOLEANS = new Set([
  "ae_lock",
  "af_lock",
  "awb_lock",
  "torch",
  "face_detect",
  "audio",
]);

const STRINGS: Record<string, string[]> = {
  camera_id: ["0", "1", "2"],
  physical_camera_id: ["none", "0", "1", "2"],
  still_format: ["JPEG", "RAW_SENSOR"],
  video_codec: ["h264", "h265"],
  video_container: ["mp4"],
};

const SIZES: Record<string, { width: number; height: number }[]> = {
  stream_size: [
    { width: 640, height: 480 },
    { width: 1280, height: 720 },
    { width: 1920, height: 1080 },
  ],
  still_size: [
    { width: 1920, height: 1080 },
    { width: 4000, height: 3000 },
  ],
  video_size: [
    { width: 1280, height: 720 },
    { width: 1920, height: 1080 },
    { width: 3840, height: 2160 },
  ],
};

const REGIONS = new Set(["ae_regions", "af_regions", "awb_regions"]);
const TRIGGERS = new Set(["ae_precapture", "af_trigger"]);

const FPS_RANGES: [number, number][] = [
  [15, 30],
  [30, 30],
  [60, 60],
];

const WRITABLE_DEFAULTS: Record<string, unknown> = {
  control_mode: 1,
  ae_mode: 1,
  iso: AUTO.iso,
  exposure_time_ns: AUTO.exposure_time_ns,
  frame_duration_ns: AUTO.frame_duration_ns,
  ae_compensation: 0,
  anti_banding: 3,
  ae_fps_range: [30, 30],
  ae_lock: false,
  af_mode: 4,
  focus_distance: AUTO.focus_distance,
  af_lock: false,
  awb_mode: 1,
  awb_lock: false,
  effect: 0,
  aberration_correction: 1,
  zoom_ratio: 1,
  filter_density: 0,
  optical_stabilization: 1,
  video_stabilization: 0,
  lens_shading: 1,
  distortion_correction: 1,
  torch: false,
  flash_mode: 0,
  scene_mode: 0,
  noise_reduction: 1,
  edge_enhancement: 1,
  hot_pixel: 1,
  tonemap: 1,
  tonemap_preset_curve: 0,
  face_detect: false,
  test_pattern: 0,
  camera_id: "0",
  physical_camera_id: "none",
  stream_size: { width: 1280, height: 720 },
  jpeg_quality: 80,
  stream_rotation: 0,
  still_format: "JPEG",
  still_size: { width: 4000, height: 3000 },
  video_size: { width: 1920, height: 1080 },
  video_bitrate: 8_000_000,
  video_codec: "h264",
  video_container: "mp4",
  audio: false,
};

const READOUT_DEFAULTS: Record<string, unknown> = {
  ae_state: "converged",
  af_state: "passive_focused",
  awb_state: "converged",
  focus_distance_diopters: AUTO.focus_distance,
  awb_gains: { r: 1.8, g: 1, b: 1.5 },
  color_transform: {
    matrix_size: 3,
    components: [1.2, 0, 0, 0, 1, 0, 0, 0, 1.1],
  },
  zoom_actual: AUTO.zoom_actual,
  lens_focal_length: 4.3,
  lens_aperture: 2,
  lens_filter_density: 0,
  lens_state: "stationary",
  tonemap_curve: {
    red: [0, 0.45, 1],
    green: [0, 0.5, 1],
    blue: [0, 0.4, 1],
  },
  tonemap_gamma: 2.2,
  face_count: 0,
  faces: [],
  fps_actual: AUTO.fps_actual,
  recording: false,
  battery_percent: 87,
  storage_free_mb: 24_576,
};

const READOUT_IDS = Object.keys(READOUT_DEFAULTS);
const WRITABLE_IDS = Object.keys(WRITABLE_DEFAULTS);
const ALL_IDS = [...WRITABLE_IDS, ...READOUT_IDS, ...REGIONS, ...TRIGGERS];

const FACE = {
  id: 1,
  score: 0.92,
  x: 0.32,
  y: 0.22,
  w: 0.18,
  h: 0.28,
};

function cloneValue<T>(value: T): T {
  return structuredClone(value);
}

function initial(): ControlState {
  const state: ControlState = {};
  for (const id of WRITABLE_IDS) {
    const value = cloneValue(WRITABLE_DEFAULTS[id]);
    if ((AUTO_EXPOSURE_IDS as readonly string[]).includes(id)) {
      state[id] = { supported: true, actual: value, source: "auto" };
    } else if (id === "focus_distance") {
      state[id] = { supported: true, actual: value, source: "auto" };
    } else {
      state[id] = {
        supported: true,
        requested: value,
        actual: value,
        source: "host",
      };
    }
  }
  for (const id of READOUT_IDS) {
    state[id] = {
      supported: true,
      actual: cloneValue(READOUT_DEFAULTS[id]),
      source: "device",
    };
  }
  for (const id of REGIONS) state[id] = { supported: true };
  for (const id of TRIGGERS) state[id] = { supported: true };
  for (const id of ["ae_lock", "af_lock", "awb_lock"]) {
    const entry = state[id];
    if (entry) entry.locked = false;
  }
  return state;
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

function rangeOf(id: string): RangeSpec {
  const spec = RANGES[id];
  if (!spec) throw new Error(`missing range spec for ${id}`);
  return spec;
}

function round(value: number, step: number): number {
  return Math.round(value / step) * step;
}

function normalizeRegion(value: unknown): ControlRegion | null {
  if (!value || typeof value !== "object") return null;
  const record = value as Record<string, unknown>;
  const x = Number(record["x"]);
  const y = Number(record["y"]);
  const w = Number(record["w"]);
  const h = Number(record["h"]);
  if (![x, y, w, h].every(Number.isFinite)) return null;
  const width = clamp(w, 0.02, 1);
  const height = clamp(h, 0.02, 1);
  return {
    x: clamp(x, 0, 1 - width),
    y: clamp(y, 0, 1 - height),
    w: width,
    h: height,
    weight: Number.isFinite(Number(record["weight"]))
      ? Number(record["weight"])
      : 1000,
  };
}

function sizeOf(value: unknown): { width: number; height: number } | null {
  if (typeof value === "string") {
    const match = /^(\d+)x(\d+)$/.exec(value.trim());
    if (!match) return null;
    const width = Number(match[1]);
    const height = Number(match[2]);
    return width > 0 && height > 0 ? { width, height } : null;
  }
  if (!value || typeof value !== "object") return null;
  const record = value as Record<string, unknown>;
  const width = Number(record["width"]);
  const height = Number(record["height"]);
  return Number.isFinite(width) && Number.isFinite(height) && width > 0 && height > 0
    ? { width, height }
    : null;
}

/** Deterministic placeholder digest for demo media items. */
function fakeSha256(seed: string): string {
  let hash = 2166136261;
  let out = "";
  for (let index = 0; index < 64; index += 1) {
    hash ^= seed.charCodeAt(index % Math.max(seed.length, 1));
    hash = Math.imul(hash, 16777619);
    out += ((hash >>> 28) & 0xf).toString(16);
  }
  return out;
}

/** Synthetic JPEG used by the demo media library and captured stills. */
function demoSceneDataUrl(label: string, subtitle: string): string {
  const canvas = document.createElement("canvas");
  canvas.width = 640;
  canvas.height = 360;
  const context = canvas.getContext("2d");
  if (!context) {
    return (
      "data:image/svg+xml;charset=utf-8," +
      encodeURIComponent(
        `<svg xmlns="http://www.w3.org/2000/svg" width="640" height="360">` +
          `<rect width="640" height="360" fill="#10202c"/>` +
          `<text x="320" y="180" fill="#7ce0b3" font-size="28" text-anchor="middle">${label}</text>` +
          `</svg>`,
      )
    );
  }
  const gradient = context.createLinearGradient(0, 0, 640, 360);
  gradient.addColorStop(0, "#0b1a26");
  gradient.addColorStop(0.55, "#123240");
  gradient.addColorStop(1, "#0a2a24");
  context.fillStyle = gradient;
  context.fillRect(0, 0, 640, 360);
  context.fillStyle = "#1c2a35";
  context.fillRect(48, 56, 544, 248);
  context.strokeStyle = "#3b5566";
  context.lineWidth = 3;
  context.strokeRect(48, 56, 544, 248);
  context.fillStyle = "#4cc2ff";
  context.fillRect(86, 96, 180, 8);
  context.fillStyle = "#7ce0b3";
  context.font = "700 46px system-ui, sans-serif";
  context.fillText("OTT DEMO", 86, 178);
  context.fillStyle = "#e8edf5";
  context.font = "600 22px system-ui, sans-serif";
  context.fillText(label, 86, 218);
  context.fillStyle = "#9aa7b8";
  context.font = "500 18px ui-monospace, monospace";
  context.fillText(subtitle, 86, 256);
  context.fillStyle = "#ffb84d";
  context.beginPath();
  context.arc(548, 84, 9, 0, Math.PI * 2);
  context.fill();
  return canvas.toDataURL("image/jpeg", 0.82);
}

/**
 * In-memory stand-in for `watchdog device` used by the demo and Playwright.
 *
 * It mirrors the host's `state` map and the implications the Android service
 * applies (manual exposure turns AE off, focus distance turns AF off, locking
 * freezes the current actual, ...). It never talks to a real device.
 */
export class FakeDevice {
  readonly mode = "demo" as const;
  private state: ControlState = initial();
  private listeners = new Set<Listener>();
  private eventListeners = new Set<(event: DeviceEvent) => void>();
  private transferListeners = new Set<(transfer: TransferProgress) => void>();
  private media: MediaItem[] = [];
  private mediaFiles = new Map<string, string>();
  private mediaSeeded = false;
  private analysisSeq = 0;
  private captureSeq = 0;
  private recordingStartedMs: number | null = null;
  private recordingMediaId: string | null = null;
  private retention: HostRetention = { max_age_days: 30, max_size_mb: 10_240 };
  private logLines: HostLogLine[] = [];
  private logsSeeded = false;

  get snapshot(): ControlState {
    return structuredClone(this.state);
  }

  subscribe(listener: Listener): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  subscribeEvents(listener: (event: DeviceEvent) => void): () => void {
    this.eventListeners.add(listener);
    return () => this.eventListeners.delete(listener);
  }

  subscribeTransfers(
    listener: (transfer: TransferProgress) => void,
  ): () => void {
    this.transferListeners.add(listener);
    return () => this.transferListeners.delete(listener);
  }

  listMedia(): Promise<MediaItem[]> {
    this.seedMedia();
    return Promise.resolve(this.media.map((item) => structuredClone(item)));
  }

  getMedia(id: string): Promise<MediaItem | null> {
    this.seedMedia();
    const item = this.media.find((entry) => entry.id === id);
    return Promise.resolve(item ? structuredClone(item) : null);
  }

  deleteMedia(id: string): Promise<void> {
    this.seedMedia();
    const index = this.media.findIndex((entry) => entry.id === id);
    if (index < 0) {
      return Promise.reject(new Error(`media ${id} was not found`));
    }
    this.media.splice(index, 1);
    this.mediaFiles.delete(id);
    return Promise.resolve();
  }

  mediaFileUrl(id: string): string {
    this.seedMedia();
    return this.mediaFiles.get(id) ?? "";
  }

  analyzeMedia(ids: string[]): Promise<AnalysisSubmission> {
    this.seedMedia();
    const missing = ids.find(
      (id) => !this.media.some((item) => item.id === id),
    );
    if (missing) {
      return Promise.reject(new Error(`media ${missing} was not found`));
    }
    this.analysisSeq += 1;
    return Promise.resolve({
      id: `an-demo-${Date.now().toString(36)}-${this.analysisSeq}`,
      state: "queued",
    });
  }

  getSettings(): Promise<HostSettings> {
    this.seedMedia();
    this.seedLogs();
    const mediaBytes = this.media.reduce((sum, item) => sum + item.bytes, 0);
    const settings: HostSettings = {
      ports: { ui: 7700, mcp: 7701, api: 7702 },
      listen: {
        mode: "localhost",
        addresses: { ui: "127.0.0.1", mcp: "127.0.0.1", api: "127.0.0.1" },
      },
      token: { configured: false, hint: null },
      storage: {
        data_dir: "demo/data",
        media_dir: "demo/data/media",
        logs_dir: "demo/data/logs",
        media_items: this.media.length,
        media_bytes: mediaBytes,
        free_bytes: 128 * 1024 ** 3,
        total_bytes: 512 * 1024 ** 3,
      },
      retention: { ...this.retention },
      ffmpeg: {
        available: true,
        ffmpeg_version: "7.1 (demo)",
        ffprobe_version: "7.1 (demo)",
        path: "embedded://ffmpeg",
        license: "LGPL-2.1-or-later",
        filters: 418,
      },
      about: {
        version: "0.1.0-demo",
        commit: "demo-build",
        build: "2026-11-17",
        os: "demo",
        arch: "wasm",
        protocol: 1,
      },
    };
    return Promise.resolve(settings);
  }

  saveRetention(retention: HostRetention): Promise<HostRetention> {
    this.retention = {
      max_age_days: Math.max(0, Math.round(retention.max_age_days)),
      max_size_mb: Math.max(0, Math.round(retention.max_size_mb)),
    };
    this.appendLog(
      "info",
      "settings",
      `retention updated: ${this.retention.max_age_days} days, ${this.retention.max_size_mb} MiB`,
    );
    return Promise.resolve({ ...this.retention });
  }

  listLogs(options: { level?: string; limit?: number } = {}): Promise<
    HostLogLine[]
  > {
    this.seedLogs();
    const level = options.level ?? "all";
    const limit = options.limit ?? 200;
    const lines = this.logLines
      .filter((line) => level === "all" || line.level === level)
      .slice(-limit)
      .map((line) => ({ ...line }));
    return Promise.resolve(lines);
  }

  diagnosticsUrl(): string {
    this.seedMedia();
    this.seedLogs();
    const payload = {
      generated_ms: Date.now(),
      about: {
        version: "0.1.0-demo",
        commit: "demo-build",
        build: "2026-11-17",
        os: "demo",
        arch: "wasm",
      },
      settings: {
        ports: { ui: 7700, mcp: 7701, api: 7702 },
        retention: { ...this.retention },
      },
      media: this.media.map((item) => ({
        id: item.id,
        type: item.type,
        bytes: item.bytes,
        sha256: item.sha256,
      })),
      logs: this.logLines,
    };
    return `data:application/json;charset=utf-8,${encodeURIComponent(
      JSON.stringify(payload, null, 2),
    )}`;
  }

  async captureStill(): Promise<CaptureOutcome> {
    const mediaId = this.nextMediaId("still");
    const { width, height } = sizeOf(this.state["still_size"]?.actual) ?? {
      width: 4000,
      height: 3000,
    };
    const bytes = Math.round(width * height * 0.35);
    this.addStillMedia(mediaId, width, height, bytes, Date.now(), "Captured still");
    this.emitEvent({
      kind: "capture_finished",
      timestamp_ms: Date.now(),
      data: { media_id: mediaId, type: "still", width, height, bytes },
    });
    this.startTransfer(mediaId, bytes);
    return {
      mediaId,
      type: "still",
      width,
      height,
      bytes,
      applied: { media_id: mediaId, type: "still", width, height, bytes },
    };
  }

  async captureBurst(options: BurstOptions): Promise<CaptureOutcome> {
    const count = clamp(Math.round(options.count), 1, 50);
    const interval = clamp(Math.round(options.intervalMs), 0, 5000);
    const sequenceId = this.nextMediaId("sequence");
    const mediaIds: string[] = [];
    for (let done = 1; done <= count; done += 1) {
      const mediaId = this.nextMediaId("burst");
      mediaIds.push(mediaId);
      this.emitEvent({
        kind: "capture_progress",
        timestamp_ms: Date.now(),
        data: {
          sequence_id: sequenceId,
          media_id: mediaId,
          done,
          total: count,
          from_stream: options.fromStream,
        },
      });
      if (done < count) {
        await new Promise((resolve) =>
          window.setTimeout(resolve, Math.max(interval, 20)),
        );
      }
    }
    const bytes = count * (options.fromStream ? 120_000 : 1_500_000);
    const stillSize = sizeOf(this.state["still_size"]?.actual) ?? {
      width: 4000,
      height: 3000,
    };
    const streamSize = sizeOf(this.state["stream_size"]?.actual) ?? {
      width: 1280,
      height: 720,
    };
    const frameSize = options.fromStream ? streamSize : stillSize;
    const perFrameBytes = Math.max(1, Math.round(bytes / count));
    mediaIds.forEach((mediaId, index) => {
      this.addStillMedia(
        mediaId,
        frameSize.width,
        frameSize.height,
        perFrameBytes,
        Date.now(),
        `Burst frame ${index + 1}/${count}`,
      );
    });
    this.startTransfer(sequenceId, bytes);
    return {
      sequenceId,
      mediaIds,
      type: "sequence",
      bytes,
      applied: {
        sequence_id: sequenceId,
        media_ids: mediaIds,
        count,
        interval_ms: interval,
        from_stream: options.fromStream,
      },
    };
  }

  async recordStart(options: RecordOptions = {}): Promise<CaptureOutcome> {
    if (this.state.recording?.actual === true) {
      throw new Error("already recording");
    }
    const startedMs = Date.now();
    const mediaId = this.nextMediaId("video");
    this.recordingStartedMs = startedMs;
    this.recordingMediaId = mediaId;
    this.setRecording(true);
    this.emitEvent({
      kind: "recording_started",
      timestamp_ms: startedMs,
      data: {
        media_id: mediaId,
        started_ms: startedMs,
        max_duration_secs: options.maxDurationSecs ?? 0,
      },
    });
    if (options.maxDurationSecs && options.maxDurationSecs > 0) {
      window.setTimeout(
        () => {
          if (this.state.recording?.actual === true) {
            void this.recordStop().catch(() => {
              // A manual stop won the race; nothing to report in the demo.
            });
          }
        },
        options.maxDurationSecs * 1000,
      );
    }
    return {
      mediaId,
      type: "video",
      startedMs,
      applied: {
        media_id: mediaId,
        started_ms: startedMs,
        max_duration_secs: options.maxDurationSecs ?? 0,
      },
    };
  }

  async recordStop(): Promise<CaptureOutcome> {
    if (this.state.recording?.actual !== true) {
      throw new Error("not recording");
    }
    const startedMs = this.recordingStartedMs ?? Date.now();
    const durationMs = Math.max(0, Date.now() - startedMs);
    const mediaId = this.recordingMediaId ?? this.nextMediaId("video");
    const bytes = Math.max(1, Math.round((durationMs / 1000) * 1_000_000));
    const videoSize = sizeOf(this.state["video_size"]?.actual) ?? {
      width: 1920,
      height: 1080,
    };
    this.addVideoMedia(
      mediaId,
      durationMs,
      bytes,
      videoSize.width,
      videoSize.height,
      Date.now(),
    );
    this.recordingStartedMs = null;
    this.recordingMediaId = null;
    this.setRecording(false);
    this.emitEvent({
      kind: "recording_stopped",
      timestamp_ms: Date.now(),
      data: { media_id: mediaId, duration_ms: durationMs, bytes },
    });
    this.startTransfer(mediaId, bytes);
    return {
      mediaId,
      type: "video",
      durationMs,
      bytes,
      applied: { media_id: mediaId, duration_ms: durationMs, bytes },
    };
  }

  setControls(changes: ControlChange[]): ApplyResult[] {
    const expanded = [...changes];
    const has = (id: string): boolean =>
      expanded.some((change) => change.id === id);

    if (
      (has("iso") ||
        has("exposure_time_ns") ||
        has("frame_duration_ns")) &&
      !has("ae_mode")
    ) {
      expanded.unshift({ id: "ae_mode", value: 0 });
    }
    if (has("focus_distance") && !has("af_mode")) {
      expanded.unshift({ id: "af_mode", value: 0 });
    }

    const results = expanded.map((change) => this.apply(change));
    this.emit(changes);
    return results;
  }

  reset(targets: string[]): ApplyResult[] {
    const ids = targets.includes("all")
      ? ALL_IDS.filter((id) => id !== "camera_id" && id !== "physical_camera_id")
      : targets;
    const results = ids.map((id) => this.resetOne(id));
    this.emit([]);
    return results;
  }

  clearRegions(): ApplyResult[] {
    const results = [...REGIONS].map((id) => {
      const entry = this.state[id];
      if (!entry) return { control: id, ok: false, error: "unknown control" };
      delete entry.requested;
      entry.actual = undefined;
      entry.source = "host";
      return { control: id, ok: true, applied: null };
    });
    this.emit([]);
    return results;
  }

  /** Make reported actuals drift from requested values, as real devices do. */
  drift(): void {
    const iso = this.state.iso;
    if (iso && typeof iso.requested === "number") {
      iso.actual = round(clamp(iso.requested * 1.6, 50, 3200), 10);
      iso.source = "device";
    }
    const exposure = this.state.exposure_time_ns;
    if (exposure && typeof exposure.requested === "number") {
      exposure.actual = round(
        clamp(exposure.requested * 1.5, 1_000_000, 100_000_000),
        1000,
      );
      exposure.source = "device";
    }
    const zoom = this.state.zoom_actual;
    const ratio = this.state.zoom_ratio;
    if (zoom && ratio && typeof ratio.requested === "number") {
      zoom.actual = round(clamp(ratio.requested * 1.2, 1, 8), 0.1);
      zoom.source = "device";
    }
    const gains = this.state.awb_gains;
    if (gains && typeof gains.actual === "object" && gains.actual !== null) {
      const record = gains.actual as Record<string, number>;
      gains.actual = {
        r: round((record.r ?? 1) * 1.1, 0.01),
        g: record.g ?? 1,
        b: round((record.b ?? 1) * 0.9, 0.01),
      };
      gains.source = "device";
    }
    const fps = this.state.fps_actual;
    if (fps && typeof fps.actual === "number") {
      fps.actual = round(clamp(fps.actual + 1.5, 1, 60), 0.1);
      fps.source = "device";
    }
    this.emit([]);
  }

  private apply(change: ControlChange): ApplyResult {
    const entry = this.state[change.id];
    if (!entry) {
      return { control: change.id, ok: false, error: "unknown control" };
    }
    if (!entry.supported) {
      return { control: change.id, ok: false, error: "unsupported" };
    }
    if (READOUT_IDS.includes(change.id)) {
      return { control: change.id, ok: false, error: "read-only control" };
    }

    if (TRIGGERS.has(change.id)) {
      const trigger = String(change.value);
      if (trigger !== "start" && trigger !== "cancel") {
        return {
          control: change.id,
          ok: false,
          error: 'trigger expects "start" or "cancel"',
        };
      }
      entry.requested = trigger;
      entry.actual = trigger;
      entry.source = "host";
      if (change.id === "af_trigger") {
        this.setAfState(trigger === "start" ? "focused_locked" : "passive_focused");
      } else {
        this.setAeState(trigger === "start" ? "precapture" : "converged");
      }
      return { control: change.id, ok: true, applied: trigger };
    }

    if (REGIONS.has(change.id)) {
      const region = normalizeRegion(change.value);
      if (!region) {
        return { control: change.id, ok: false, error: "invalid region" };
      }
      entry.requested = region;
      entry.actual = region;
      entry.source = "host";
      if (change.id === "af_regions") this.setAfState("active_scan");
      if (change.id === "ae_regions") this.setAeState("searching");
      if (change.id === "awb_regions") this.setAwbState("searching");
      return { control: change.id, ok: true, applied: region };
    }

    if (change.id === "ae_mode") {
      const mode = Number(change.value) === 1 ? 1 : 0;
      entry.requested = mode;
      entry.actual = mode;
      entry.source = "host";
      if (mode === 1) this.enterAutoExposure();
      else this.enterManualExposure();
      return { control: change.id, ok: true, applied: mode };
    }

    if (change.id === "af_mode") {
      const value = Number(change.value);
      if (!ENUMS["af_mode"]?.includes(value)) {
        return { control: change.id, ok: false, error: "invalid value" };
      }
      entry.requested = value;
      entry.actual = value;
      entry.source = "host";
      this.setAfState(value === 0 ? "inactive" : "passive_focused");
      return { control: change.id, ok: true, applied: value };
    }

    if (change.id === "focus_distance") {
      const spec = rangeOf("focus_distance");
      const value = round(
        clamp(Number(change.value), spec.min, spec.max),
        spec.step,
      );
      entry.requested = value;
      entry.actual = value;
      entry.source = "host";
      const afMode = this.state.af_mode;
      if (afMode) {
        afMode.requested = 0;
        afMode.actual = 0;
        afMode.source = "host";
      }
      this.setAfState("inactive");
      const diopters = this.state.focus_distance_diopters;
      if (diopters) {
        diopters.actual = value;
        diopters.source = "device";
      }
      return { control: change.id, ok: true, applied: value };
    }

    if (change.id === "awb_mode") {
      const value = Number(change.value);
      if (!ENUMS["awb_mode"]?.includes(value)) {
        return { control: change.id, ok: false, error: "invalid value" };
      }
      entry.requested = value;
      entry.actual = value;
      entry.source = "host";
      this.setAwbState(value === 1 ? "converged" : "locked");
      return { control: change.id, ok: true, applied: value };
    }

    if (change.id === "zoom_ratio") {
      const spec = rangeOf("zoom_ratio");
      const value = round(
        clamp(Number(change.value), spec.min, spec.max),
        spec.step,
      );
      entry.requested = value;
      entry.actual = value;
      entry.source = "host";
      const zoomActual = this.state.zoom_actual;
      if (zoomActual) {
        zoomActual.actual = value;
        zoomActual.source = "device";
      }
      return { control: change.id, ok: true, applied: value };
    }

    if (change.id === "face_detect") {
      const enabled = Boolean(change.value);
      entry.requested = enabled;
      entry.actual = enabled;
      entry.source = "host";
      this.setFaces(enabled ? [FACE] : []);
      return { control: change.id, ok: true, applied: enabled };
    }

    if (change.id === "torch") {
      const on = Boolean(change.value);
      entry.requested = on;
      entry.actual = on;
      entry.source = "host";
      return { control: change.id, ok: true, applied: on };
    }

    if (change.id === "camera_id" || change.id === "physical_camera_id") {
      const value = String(change.value);
      if (!STRINGS[change.id]?.includes(value)) {
        return { control: change.id, ok: false, error: "invalid value" };
      }
      entry.requested = value;
      entry.actual = value;
      entry.source = "host";
      this.resetPipelineDefaults();
      return { control: change.id, ok: true, applied: value };
    }

    if (change.id === "ae_fps_range") {
      if (!Array.isArray(change.value) || change.value.length !== 2) {
        return {
          control: change.id,
          ok: false,
          error: "expected [lower, upper]",
        };
      }
      const lower = Number(change.value[0]);
      const upper = Number(change.value[1]);
      if (
        !FPS_RANGES.some(
          (range) => range[0] === lower && range[1] === upper,
        )
      ) {
        return { control: change.id, ok: false, error: "unsupported range" };
      }
      entry.requested = [lower, upper];
      entry.actual = [lower, upper];
      entry.source = "host";
      return { control: change.id, ok: true, applied: [lower, upper] };
    }

    const sizeOptions = SIZES[change.id];
    if (sizeOptions) {
      const size = sizeOf(change.value);
      if (!size) {
        return {
          control: change.id,
          ok: false,
          error: "expected {width, height}",
        };
      }
      if (
        !sizeOptions.some(
          (item) => item.width === size.width && item.height === size.height,
        )
      ) {
        return { control: change.id, ok: false, error: "unsupported size" };
      }
      entry.requested = size;
      entry.actual = size;
      entry.source = "host";
      return { control: change.id, ok: true, applied: size };
    }

    const stringOptions = STRINGS[change.id];
    if (stringOptions) {
      const value = String(change.value);
      if (!stringOptions.includes(value)) {
        return { control: change.id, ok: false, error: "invalid value" };
      }
      entry.requested = value;
      entry.actual = value;
      entry.source = "host";
      return { control: change.id, ok: true, applied: value };
    }

    if (BOOLEANS.has(change.id)) {
      const value = Boolean(change.value);
      entry.requested = value;
      entry.actual = value;
      entry.source = "host";
      if (change.id === "ae_lock" || change.id === "af_lock" || change.id === "awb_lock") {
        entry.locked = value;
      }
      if (change.id === "awb_lock") {
        this.setAwbState(value ? "locked" : "converged");
      }
      return { control: change.id, ok: true, applied: value };
    }

    const enumOptions = ENUMS[change.id];
    if (enumOptions) {
      const value = Number(change.value);
      if (!enumOptions.includes(value)) {
        return { control: change.id, ok: false, error: "invalid value" };
      }
      entry.requested = value;
      entry.actual = value;
      entry.source = "host";
      return { control: change.id, ok: true, applied: value };
    }

    const spec = RANGES[change.id];
    if (spec) {
      const value = round(
        clamp(Number(change.value), spec.min, spec.max),
        spec.step,
      );
      entry.requested = value;
      entry.actual = value;
      entry.source = "host";
      return { control: change.id, ok: true, applied: value };
    }

    entry.requested = change.value as ControlValue;
    entry.actual = change.value as ControlValue;
    entry.source = "host";
    return { control: change.id, ok: true, applied: change.value };
  }

  private resetOne(id: string): ApplyResult {
    const entry = this.state[id];
    if (!entry) {
      return { control: id, ok: false, error: "unknown control" };
    }
    if (READOUT_IDS.includes(id)) {
      entry.actual = cloneValue(READOUT_DEFAULTS[id]);
      entry.source = "device";
      delete entry.requested;
      return { control: id, ok: true, applied: entry.actual };
    }
    if (REGIONS.has(id) || TRIGGERS.has(id)) {
      delete entry.requested;
      entry.actual = undefined;
      entry.source = "host";
      return { control: id, ok: true, applied: null };
    }
    if (id === "camera_id" || id === "physical_camera_id") {
      return { control: id, ok: false, error: "not resettable" };
    }

    switch (id) {
      case "ae_mode":
        entry.requested = 1;
        entry.actual = 1;
        entry.source = "host";
        this.enterAutoExposure();
        break;
      case "af_mode":
        entry.requested = 4;
        entry.actual = 4;
        entry.source = "host";
        this.setAfState("passive_focused");
        break;
      case "awb_mode":
        entry.requested = 1;
        entry.actual = 1;
        entry.source = "host";
        this.setAwbState("converged");
        break;
      case "awb_lock":
        entry.requested = false;
        entry.actual = false;
        entry.locked = false;
        entry.source = "host";
        this.setAwbState("converged");
        break;
      case "ae_lock":
      case "af_lock":
        entry.requested = false;
        entry.actual = false;
        entry.locked = false;
        entry.source = "host";
        break;
      case "iso":
      case "exposure_time_ns":
      case "frame_duration_ns":
        if (this.isAutoExposure()) {
          this.clearManualExposure(id);
        } else {
          entry.requested = AUTO[id];
          entry.actual = AUTO[id];
          entry.source = "host";
        }
        break;
      case "focus_distance":
        if (this.state.af_mode?.actual === 4) {
          delete entry.requested;
          entry.actual = AUTO.focus_distance;
          entry.source = "auto";
        } else {
          entry.requested = AUTO.focus_distance;
          entry.actual = AUTO.focus_distance;
          entry.source = "host";
        }
        break;
      case "zoom_ratio": {
        entry.requested = 1;
        entry.actual = 1;
        entry.source = "host";
        const zoomActual = this.state.zoom_actual;
        if (zoomActual) {
          zoomActual.actual = 1;
          zoomActual.source = "device";
        }
        break;
      }
      case "face_detect":
        entry.requested = false;
        entry.actual = false;
        entry.source = "host";
        this.setFaces([]);
        break;
      default: {
        if (!Object.hasOwn(WRITABLE_DEFAULTS, id)) {
          return { control: id, ok: false, error: "not resettable" };
        }
        const value = cloneValue(WRITABLE_DEFAULTS[id]);
        entry.requested = value;
        entry.actual = value;
        entry.source = "host";
        break;
      }
    }
    return { control: id, ok: true, applied: entry.actual };
  }

  private isAutoExposure(): boolean {
    return this.state.ae_mode?.actual === 1;
  }

  private enterAutoExposure(): void {
    for (const id of AUTO_EXPOSURE_IDS) this.clearManualExposure(id);
    this.setAeState("converged");
  }

  private clearManualExposure(id: (typeof AUTO_EXPOSURE_IDS)[number]): void {
    const entry = this.state[id];
    if (!entry) return;
    delete entry.requested;
    entry.actual = AUTO[id];
    entry.source = "auto";
  }

  private enterManualExposure(): void {
    for (const id of AUTO_EXPOSURE_IDS) {
      const entry = this.state[id];
      if (!entry) continue;
      if (typeof entry.actual !== "number") entry.actual = AUTO[id];
      if (typeof entry.requested !== "number") entry.requested = entry.actual;
      entry.source = "host";
    }
    this.setAeState("inactive");
  }

  /** Re-derive dependent state after a camera switch. */
  private resetPipelineDefaults(): void {
    const ae = this.state.ae_mode;
    if (ae) {
      ae.requested = 1;
      ae.actual = 1;
      ae.source = "host";
    }
    this.enterAutoExposure();
    const af = this.state.af_mode;
    if (af) {
      af.requested = 4;
      af.actual = 4;
      af.source = "host";
    }
    this.setAfState("passive_focused");
    const awb = this.state.awb_mode;
    if (awb) {
      awb.requested = 1;
      awb.actual = 1;
      awb.source = "host";
    }
    this.setAwbState("converged");
    for (const id of REGIONS) {
      const entry = this.state[id];
      if (!entry) continue;
      delete entry.requested;
      entry.actual = undefined;
    }
  }

  private setFaces(faces: unknown[]): void {
    const count = this.state.face_count;
    if (count) {
      count.actual = faces.length;
      count.source = "device";
    }
    const boxes = this.state.faces;
    if (boxes) {
      boxes.actual = faces;
      boxes.source = "device";
    }
  }

  private setAeState(value: string): void {
    const entry = this.state.ae_state;
    if (!entry) return;
    entry.actual = value;
    entry.source = "device";
  }

  private setAfState(value: string): void {
    const entry = this.state.af_state;
    if (!entry) return;
    entry.actual = value;
    entry.source = "device";
  }

  private setAwbState(value: string): void {
    const entry = this.state.awb_state;
    if (!entry) return;
    entry.actual = value;
    entry.source = "device";
  }

  private setRecording(recording: boolean): void {
    const entry = this.state.recording;
    if (entry) {
      entry.actual = recording;
      entry.source = "device";
    }
    this.emit([]);
  }

  private seedMedia(): void {
    if (this.mediaSeeded) return;
    this.mediaSeeded = true;
    const now = Date.now();
    const deviceOne = {
      model: "Pixel 7 (demo)",
      serial: "DEMO-0001",
      sdk: 34,
    };
    const deviceTwo = {
      model: "Pixel 6 (demo)",
      serial: "DEMO-0002",
      sdk: 33,
    };
    this.addMedia(
      {
        id: "m-demo-still-1",
        type: "still",
        created_ms: now - 90_000,
        width: 1920,
        height: 1080,
        bytes: 1_842_000,
        sha256: fakeSha256("m-demo-still-1"),
        mime: "image/jpeg",
        device: deviceOne,
        control_snapshot: {
          iso: 400,
          exposure_time_ns: 8_333_333,
          awb_mode: 1,
          zoom_actual: 1,
          fps_actual: 30,
        },
      },
      demoSceneDataUrl("Still · buffering", "1920×1080 · ISO 400"),
    );
    this.addMedia(
      {
        id: "m-demo-video-1",
        type: "video",
        created_ms: now - 2 * 60 * 60 * 1000,
        duration_ms: 12_400,
        width: 1920,
        height: 1080,
        bytes: 9_240_000,
        sha256: fakeSha256("m-demo-video-1"),
        mime: "video/webm",
        device: deviceOne,
        control_snapshot: {
          iso: 200,
          exposure_time_ns: 16_666_666,
          awb_mode: 1,
          zoom_actual: 1,
          fps_actual: 30,
        },
      },
      DEMO_CLIP_URL,
    );
    this.addMedia(
      {
        id: "m-demo-still-2",
        type: "still",
        created_ms: now - 26 * 60 * 60 * 1000,
        width: 3840,
        height: 2160,
        bytes: 3_208_000,
        sha256: fakeSha256("m-demo-still-2"),
        mime: "image/jpeg",
        device: deviceTwo,
        control_snapshot: {
          iso: 800,
          exposure_time_ns: 33_333_333,
          awb_mode: 5,
          zoom_actual: 2,
          fps_actual: 24,
        },
      },
      demoSceneDataUrl("Still · paused", "3840×2160 · ISO 800"),
    );
  }

  private addMedia(item: MediaItem, fileUrl: string): void {
    this.media.push(item);
    this.mediaFiles.set(item.id, fileUrl);
  }

  private addStillMedia(
    mediaId: string,
    width: number,
    height: number,
    bytes: number,
    createdMs: number,
    label: string,
  ): void {
    this.addMedia(
      {
        id: mediaId,
        type: "still",
        created_ms: createdMs,
        width,
        height,
        bytes,
        sha256: fakeSha256(mediaId),
        mime: "image/jpeg",
        device: {
          model: "Pixel 7 (demo)",
          serial: "DEMO-0001",
          sdk: 34,
        },
        control_snapshot: this.controlSnapshot(),
      },
      demoSceneDataUrl(label, `${width}×${height}`),
    );
  }

  private addVideoMedia(
    mediaId: string,
    durationMs: number,
    bytes: number,
    width: number,
    height: number,
    createdMs: number,
  ): void {
    this.addMedia(
      {
        id: mediaId,
        type: "video",
        created_ms: createdMs,
        duration_ms: durationMs,
        width,
        height,
        bytes,
        sha256: fakeSha256(mediaId),
        mime: "video/webm",
        device: {
          model: "Pixel 7 (demo)",
          serial: "DEMO-0001",
          sdk: 34,
        },
        control_snapshot: this.controlSnapshot(),
      },
      DEMO_CLIP_URL,
    );
  }

  private controlSnapshot(): Record<string, unknown> {
    const snapshot: Record<string, unknown> = {};
    for (const id of [
      "iso",
      "exposure_time_ns",
      "ae_mode",
      "af_mode",
      "focus_distance",
      "awb_mode",
      "awb_gains",
      "zoom_actual",
      "fps_actual",
    ]) {
      const entry = this.state[id];
      if (entry && entry.actual !== undefined) {
        snapshot[id] = structuredClone(entry.actual);
      }
    }
    return snapshot;
  }

  private nextMediaId(kind: string): string {
    this.captureSeq += 1;
    return `m-${kind}-${Date.now().toString(36)}-${this.captureSeq}`;
  }

  /** Seed a short, realistic log history once per demo session. */
  private seedLogs(): void {
    if (this.logsSeeded) return;
    this.logsSeeded = true;
    const now = Date.now();
    const seed: Array<[number, HostLogLine["level"], string, string]> = [
      [92_000, "info", "host", "watchdog 0.1.0 starting"],
      [91_000, "info", "config", "loaded defaults; no watchdog.toml found"],
      [90_000, "info", "ui", "web UI listening on http://127.0.0.1:7700"],
      [89_000, "info", "mcp", "MCP server listening on 127.0.0.1:7701"],
      [88_000, "info", "api", "integration API listening on 127.0.0.1:7702"],
      [74_000, "info", "device", "active session DEMO-0001 (Pixel 7 demo)"],
      [70_000, "debug", "stream", "first frame received: 1280x720 seq=1"],
      [58_000, "warn", "stream", "dropped 2 late frames while reconnecting"],
      [55_000, "info", "stream", "stream recovered after 1.4 s"],
      [40_000, "info", "media", "demo library seeded with 3 items"],
      [22_000, "debug", "presets", "preset 'OTT baseline' loaded"],
      [8_000, "error", "analysis", "no analyzer engine yet; queue disabled"],
      [3_000, "info", "health", "disk 128 GiB free; retention 30 days / 10240 MiB"],
    ];
    for (const [ago, level, target, message] of seed) {
      this.logLines.push({ ts_ms: now - ago, level, target, message });
    }
  }

  /** Append one demo log line, keeping the ring buffer bounded. */
  private appendLog(
    level: HostLogLine["level"],
    target: string,
    message: string,
  ): void {
    this.seedLogs();
    this.logLines.push({ ts_ms: Date.now(), level, target, message });
    if (this.logLines.length > 300) {
      this.logLines.splice(0, this.logLines.length - 300);
    }
  }

  private emitEvent(event: DeviceEvent): void {
    this.appendLog(
      event.kind.includes("failed") ? "error" : "info",
      "device",
      `event ${event.kind}${event.message ? `: ${event.message}` : ""}`,
    );
    for (const listener of this.eventListeners) listener(event);
  }

  private startTransfer(mediaId: string, bytesTotal: number): void {
    const transferId = `t-${Date.now().toString(36)}-${this.captureSeq}`;
    const send = (
      state: TransferProgress["state"],
      bytesDone: number,
    ): void => {
      const transfer: TransferProgress = {
        transferId,
        mediaId,
        state,
        bytesTotal,
        bytesDone,
      };
      for (const listener of this.transferListeners) listener(transfer);
    };
    send("queued", 0);
    const steps = 4;
    for (let step = 1; step <= steps; step += 1) {
      window.setTimeout(() => {
        send("transferring", Math.round((bytesTotal * step) / steps));
        if (step === steps) {
          window.setTimeout(() => {
            send("verifying", bytesTotal);
            window.setTimeout(() => send("done", bytesTotal), 80);
          }, 80);
        }
      }, step * 80);
    }
  }

  private emit(changes: ControlChange[]): void {
    if (changes.length > 0) {
      this.appendLog(
        "debug",
        "controls",
        `applied ${changes.length} change(s): ${changes
          .map((change) => change.id)
          .join(", ")}`,
      );
    }
    const snapshot = this.snapshot;
    for (const listener of this.listeners) listener(snapshot, changes);
  }
}

export type { ApplyResult, ControlEntry };
