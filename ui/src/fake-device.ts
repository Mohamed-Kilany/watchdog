import type {
  ControlChange,
  ControlEntry,
  ControlRegion,
  ControlState,
  ControlValue,
} from "./types";

export interface ApplyResult {
  control: string;
  ok: boolean;
  applied?: unknown;
  error?: string;
}

type Listener = (state: ControlState, changes: ControlChange[]) => void;

const AUTO = {
  iso: 100,
  exposure_time_ns: 16_666_666,
  frame_duration_ns: 33_333_000,
  focus_distance: 2.5,
} as const;

const ALL_IDS = [
  "ae_mode",
  "ae_lock",
  "iso",
  "exposure_time_ns",
  "frame_duration_ns",
  "ae_compensation",
  "anti_banding",
  "af_mode",
  "af_lock",
  "focus_distance",
  "af_state",
  "af_regions",
  "af_trigger",
  "ae_regions",
  "awb_mode",
  "awb_lock",
  "awb_state",
  "zoom_ratio",
  "zoom_actual",
] as const;

function initial(): ControlState {
  return {
    ae_mode: { supported: true, requested: 1, actual: 1, source: "host" },
    ae_lock: {
      supported: true,
      requested: false,
      actual: false,
      source: "host",
      locked: false,
    },
    iso: { supported: true, actual: AUTO.iso, source: "auto" },
    exposure_time_ns: {
      supported: true,
      actual: AUTO.exposure_time_ns,
      source: "auto",
    },
    frame_duration_ns: {
      supported: true,
      actual: AUTO.frame_duration_ns,
      source: "auto",
    },
    ae_compensation: {
      supported: true,
      requested: 0,
      actual: 0,
      source: "host",
    },
    anti_banding: {
      supported: true,
      requested: 3,
      actual: 3,
      source: "host",
    },
    af_mode: { supported: true, requested: 4, actual: 4, source: "host" },
    af_lock: {
      supported: true,
      requested: false,
      actual: false,
      source: "host",
      locked: false,
    },
    focus_distance: {
      supported: true,
      actual: AUTO.focus_distance,
      source: "auto",
    },
    af_state: { supported: true, actual: "passive_focused", source: "device" },
    af_regions: { supported: true },
    af_trigger: { supported: true },
    ae_regions: { supported: true },
    awb_mode: { supported: true, requested: 1, actual: 1, source: "host" },
    awb_lock: {
      supported: true,
      requested: false,
      actual: false,
      source: "host",
      locked: false,
    },
    awb_state: { supported: true, actual: "converged", source: "device" },
    zoom_ratio: { supported: true, requested: 1, actual: 1, source: "host" },
    zoom_actual: { supported: true, actual: 1, source: "device" },
  };
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
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
      : 1,
  };
}

/**
 * In-memory stand-in for `watchdog device` used by the demo and Playwright.
 *
 * It mirrors the host's `state` map and the implications the Android service
 * applies (manual exposure turns AE off, focus distance turns AF off, locking
 * freezes the current actual, ...). It never talks to a real device.
 */
export class FakeDevice {
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

    if ((has("iso") || has("exposure_time_ns")) && !has("ae_mode")) {
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
    const ids = targets.includes("all") ? [...ALL_IDS] : targets;
    const results = ids.map((id) => this.resetOne(id));
    this.emit([]);
    return results;
  }

  clearRegions(): ApplyResult[] {
    const results = ["ae_regions", "af_regions"].map((id) => {
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

    switch (change.id) {
      case "ae_mode": {
        const mode = Number(change.value) === 1 ? 1 : 0;
        entry.requested = mode;
        entry.actual = mode;
        entry.source = "host";
        if (mode === 1) this.enterAutoExposure();
        else this.enterManualExposure();
        return { control: change.id, ok: true, applied: mode };
      }
      case "ae_lock": {
        const locked = Boolean(change.value);
        entry.requested = locked;
        entry.actual = locked;
        entry.locked = locked;
        entry.source = "host";
        return { control: change.id, ok: true, applied: locked };
      }
      case "iso": {
        const value = round(clamp(Number(change.value), 50, 3200), 50);
        entry.requested = value;
        entry.actual = value;
        entry.source = "host";
        this.forceManualExposure();
        return { control: change.id, ok: true, applied: value };
      }
      case "exposure_time_ns": {
        const value = round(
          clamp(Number(change.value), 1_000_000, 100_000_000),
          1_000_000,
        );
        entry.requested = value;
        entry.actual = value;
        entry.source = "host";
        this.forceManualExposure();
        return { control: change.id, ok: true, applied: value };
      }
      case "frame_duration_ns": {
        const value = round(
          clamp(Number(change.value), 1_000_000, 100_000_000),
          1000,
        );
        entry.requested = value;
        entry.actual = value;
        entry.source = "host";
        return { control: change.id, ok: true, applied: value };
      }
      case "ae_compensation": {
        const value = round(clamp(Number(change.value), -2, 2), 0.5);
        entry.requested = value;
        entry.actual = value;
        entry.source = "host";
        return { control: change.id, ok: true, applied: value };
      }
      case "anti_banding": {
        const value = Number(change.value);
        entry.requested = value;
        entry.actual = value;
        entry.source = "host";
        return { control: change.id, ok: true, applied: value };
      }
      case "af_mode": {
        const value = Number(change.value);
        entry.requested = value;
        entry.actual = value;
        entry.source = "host";
        this.setAfState(value === 0 ? "inactive" : "passive_focused");
        return { control: change.id, ok: true, applied: value };
      }
      case "af_lock": {
        const locked = Boolean(change.value);
        entry.requested = locked;
        entry.actual = locked;
        entry.locked = locked;
        entry.source = "host";
        return { control: change.id, ok: true, applied: locked };
      }
      case "focus_distance": {
        const value = round(clamp(Number(change.value), 0, 10), 0.1);
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
        return { control: change.id, ok: true, applied: value };
      }
      case "af_regions": {
        const region = normalizeRegion(change.value);
        entry.requested = region ?? undefined;
        entry.actual = region ?? undefined;
        entry.source = "host";
        this.setAfState("active_scan");
        return { control: change.id, ok: true, applied: region };
      }
      case "af_trigger": {
        entry.requested = change.value;
        entry.actual = change.value;
        entry.source = "host";
        this.setAfState(
          change.value === "start" ? "focused_locked" : "passive_focused",
        );
        return { control: change.id, ok: true, applied: change.value };
      }
      case "ae_regions": {
        const region = normalizeRegion(change.value);
        entry.requested = region ?? undefined;
        entry.actual = region ?? undefined;
        entry.source = "host";
        return { control: change.id, ok: true, applied: region };
      }
      case "awb_mode": {
        const value = Number(change.value);
        entry.requested = value;
        entry.actual = value;
        entry.source = "host";
        this.setAwbState(value === 1 ? "converged" : "locked");
        return { control: change.id, ok: true, applied: value };
      }
      case "awb_lock": {
        const locked = Boolean(change.value);
        entry.requested = locked;
        entry.actual = locked;
        entry.locked = locked;
        entry.source = "host";
        this.setAwbState(locked ? "locked" : "converged");
        return { control: change.id, ok: true, applied: locked };
      }
      case "zoom_ratio": {
        const value = round(clamp(Number(change.value), 1, 8), 0.1);
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
      default:
        entry.requested = change.value as ControlValue;
        entry.actual = change.value as ControlValue;
        entry.source = "host";
        return { control: change.id, ok: true, applied: change.value };
    }
  }

  private resetOne(id: string): ApplyResult {
    const entry = this.state[id];
    if (!entry) {
      return { control: id, ok: false, error: "unknown control" };
    }
    switch (id) {
      case "ae_mode":
        entry.requested = 1;
        entry.actual = 1;
        entry.source = "host";
        this.enterAutoExposure();
        break;
      case "ae_lock":
        entry.requested = false;
        entry.actual = false;
        entry.locked = false;
        entry.source = "host";
        break;
      case "iso":
        if (this.isAutoExposure()) this.clearManualExposure("iso");
        else {
          entry.requested = AUTO.iso;
          entry.actual = AUTO.iso;
          entry.source = "host";
        }
        break;
      case "exposure_time_ns":
        if (this.isAutoExposure()) this.clearManualExposure("exposure_time_ns");
        else {
          entry.requested = AUTO.exposure_time_ns;
          entry.actual = AUTO.exposure_time_ns;
          entry.source = "host";
        }
        break;
      case "frame_duration_ns":
        if (this.isAutoExposure()) this.clearManualExposure("frame_duration_ns");
        else {
          entry.requested = AUTO.frame_duration_ns;
          entry.actual = AUTO.frame_duration_ns;
          entry.source = "host";
        }
        break;
      case "ae_compensation":
        entry.requested = 0;
        entry.actual = 0;
        entry.source = "host";
        break;
      case "anti_banding":
        entry.requested = 3;
        entry.actual = 3;
        entry.source = "host";
        break;
      case "af_mode":
        entry.requested = 4;
        entry.actual = 4;
        entry.source = "host";
        this.setAfState("passive_focused");
        break;
      case "af_lock":
        entry.requested = false;
        entry.actual = false;
        entry.locked = false;
        entry.source = "host";
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
      case "af_regions":
      case "ae_regions":
        delete entry.requested;
        entry.actual = undefined;
        entry.source = "host";
        break;
      case "af_trigger":
        delete entry.requested;
        entry.actual = undefined;
        entry.source = "host";
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
      case "af_state":
        entry.actual = "passive_focused";
        entry.source = "device";
        break;
      case "awb_state":
        entry.actual = "converged";
        entry.source = "device";
        break;
      case "zoom_actual":
        entry.actual = 1;
        entry.source = "device";
        break;
      default:
        return { control: id, ok: false, error: "not resettable" };
    }
    return { control: id, ok: true, applied: entry.actual };
  }

  private isAutoExposure(): boolean {
    return this.state.ae_mode?.actual === 1;
  }

  private enterAutoExposure(): void {
    this.clearManualExposure("iso");
    this.clearManualExposure("exposure_time_ns");
    this.clearManualExposure("frame_duration_ns");
  }

  private clearManualExposure(id: "iso" | "exposure_time_ns" | "frame_duration_ns"): void {
    const entry = this.state[id];
    if (!entry) return;
    delete entry.requested;
    entry.actual = AUTO[id];
    entry.source = "auto";
  }

  private enterManualExposure(): void {
    for (const id of ["iso", "exposure_time_ns", "frame_duration_ns"] as const) {
      const entry = this.state[id];
      if (!entry) continue;
      if (typeof entry.actual !== "number") entry.actual = AUTO[id];
      if (typeof entry.requested !== "number") entry.requested = entry.actual;
      entry.source = "host";
    }
  }

  private forceManualExposure(): void {
    const mode = this.state.ae_mode;
    if (!mode || mode.actual === 0) return;
    mode.requested = 0;
    mode.actual = 0;
    mode.source = "host";
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

export type { ControlEntry };
