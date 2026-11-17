import type {
  ApplyResult,
  ControlChange,
  ControlEntry,
  ControlRegion,
  ControlState,
  ControlValue,
} from "./types";

type Listener = (state: ControlState, changes: ControlChange[]) => void;

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

  get snapshot(): ControlState {
    return structuredClone(this.state);
  }

  subscribe(listener: Listener): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
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

  private emit(changes: ControlChange[]): void {
    const snapshot = this.snapshot;
    for (const listener of this.listeners) listener(snapshot, changes);
  }
}

export type { ApplyResult, ControlEntry };
