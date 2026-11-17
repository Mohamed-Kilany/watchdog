// Host-stored control presets (M4-T11) exposed to the controls page.
//
// The schema and apply order mirror `docs/presets.md`: a preset stores the
// *requested* value of every writable control, skips read-only and one-shot
// entries, and is applied as one `set_controls` batch with the camera ids first.
// The demo keeps presets in `localStorage`; the host keeps one JSON file per
// preset under `<data_dir>/presets`.

import { authHeaders, errorMessageFromBody } from "./host";
import type { ControlChange, ControlState } from "./types";

/** Read-only `STATE` entries, mirroring `watchdog_presets::READ_ONLY_CONTROLS`. */
export const READ_ONLY_CONTROLS = new Set([
  "fps_actual",
  "recording",
  "battery_percent",
  "ae_state",
  "af_state",
  "awb_state",
  "focus_distance_diopters",
  "awb_gains",
  "color_transform",
  "zoom_actual",
  "lens_focal_length",
  "lens_aperture",
  "lens_filter_density",
  "lens_state",
  "face_count",
  "faces",
  "tonemap_curve",
  "tonemap_gamma",
]);

/** One-shot triggers that are never stored in a preset. */
export const ONE_SHOT_CONTROLS = new Set(["ae_precapture", "af_trigger"]);

/** Controls that rebuild the capture session or pipeline. */
export const STRUCTURAL_CONTROLS = new Set([
  "camera_id",
  "physical_camera_id",
  "stream_size",
  "jpeg_quality",
  "stream_rotation",
  "still_format",
  "still_size",
  "video_size",
  "video_bitrate",
  "video_codec",
  "video_container",
  "audio",
]);

/** Summary returned by `GET /api/presets` (matches `PresetSummary`). */
export interface PresetSummary {
  name: string;
  file: string;
  description?: string;
  controls: number;
  updated_at?: string;
}

/** Full preset document (matches `watchdog_presets::Preset`). */
export interface Preset {
  schema_version: number;
  name: string;
  description?: string;
  controls: Record<string, unknown>;
  created_at?: string;
  updated_at?: string;
}

/** Storage shared by the demo and the host UI. */
export interface PresetSource {
  readonly mode: "demo" | "host";
  list(): Promise<PresetSummary[]>;
  load(name: string): Promise<Preset>;
  save(name: string, controls: Record<string, unknown>): Promise<Preset>;
  remove(name: string): Promise<void>;
}

/** Capture the writable controls from a state snapshot. */
export function captureControls(
  state: ControlState,
  options: { includeStructural?: boolean } = {},
): Record<string, unknown> {
  const controls: Record<string, unknown> = {};
  for (const [id, entry] of Object.entries(state)) {
    if (!entry.supported) continue;
    if (READ_ONLY_CONTROLS.has(id) || ONE_SHOT_CONTROLS.has(id)) continue;
    if (!options.includeStructural && STRUCTURAL_CONTROLS.has(id)) continue;
    const value = entry.requested ?? entry.actual;
    if (value === undefined || value === null) continue;
    controls[id] = structuredClone(value);
  }
  return controls;
}

/** One `set_controls` batch in the documented apply order. */
export function orderedChanges(controls: Record<string, unknown>): ControlChange[] {
  const ids = Object.keys(controls);
  ids.sort((left, right) => {
    const rank = (id: string): number =>
      id === "camera_id" ? 0 : id === "physical_camera_id" ? 1 : 2;
    const delta = rank(left) - rank(right);
    return delta !== 0 ? delta : left.localeCompare(right);
  });
  return ids.map((id) => ({ id, value: structuredClone(controls[id]) }));
}

/** Lowercase a display name into a file stem, mirroring `slugify` in Rust. */
export function slugify(name: string): string {
  const trimmed = name.trim();
  let slug = "";
  for (const character of trimmed) {
    if (/[A-Za-z0-9]/.test(character)) {
      slug += character.toLowerCase();
    } else if (slug.length > 0 && !slug.endsWith("-")) {
      slug += "-";
    }
  }
  return slug.replace(/-+$/, "");
}

const STORAGE_KEY = "watchdog.presets";

function readStore(): Record<string, Preset> {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as unknown;
    if (!parsed || typeof parsed !== "object") return {};
    return parsed as Record<string, Preset>;
  } catch {
    return {};
  }
}

function writeStore(store: Record<string, Preset>): void {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(store));
}

/** Demo preset source: `localStorage`, no host required. */
export class LocalPresetSource implements PresetSource {
  readonly mode = "demo" as const;

  async list(): Promise<PresetSummary[]> {
    return Object.entries(readStore())
      .map(([file, preset]) => ({
        name: preset.name,
        file: `${file}.json`,
        description: preset.description,
        controls: Object.keys(preset.controls ?? {}).length,
        updated_at: preset.updated_at,
      }))
      .sort((left, right) => left.name.localeCompare(right.name));
  }

  async load(name: string): Promise<Preset> {
    const preset = readStore()[slugify(name)];
    if (!preset) throw new Error(`no preset named "${name}"`);
    return structuredClone(preset);
  }

  async save(
    name: string,
    controls: Record<string, unknown>,
  ): Promise<Preset> {
    const trimmed = name.trim();
    if (!trimmed) throw new Error("preset name is required");
    const slug = slugify(trimmed);
    if (!slug) throw new Error("preset name needs an ASCII letter or digit");
    if (Object.keys(controls).length === 0) {
      throw new Error("there are no writable controls to save");
    }
    const store = readStore();
    const now = new Date().toISOString();
    const existing = store[slug];
    const preset: Preset = {
      schema_version: 1,
      name: trimmed,
      controls: structuredClone(controls),
      created_at: existing?.created_at ?? now,
      updated_at: now,
    };
    store[slug] = preset;
    writeStore(store);
    return preset;
  }

  async remove(name: string): Promise<void> {
    const slug = slugify(name);
    const store = readStore();
    if (!store[slug]) throw new Error(`no preset named "${name}"`);
    delete store[slug];
    writeStore(store);
  }
}

/** Host preset source backed by `/api/presets`. */
export class HostPresetSource implements PresetSource {
  readonly mode = "host" as const;

  constructor(private readonly token: string | null) {}

  async list(): Promise<PresetSummary[]> {
    const body = await this.request("GET", "/api/presets");
    const presets = (body as { presets?: PresetSummary[] }).presets;
    return Array.isArray(presets) ? presets : [];
  }

  async load(name: string): Promise<Preset> {
    const body = await this.request(
      "GET",
      `/api/presets?name=${encodeURIComponent(name)}`,
    );
    return body as Preset;
  }

  async save(
    name: string,
    controls: Record<string, unknown>,
  ): Promise<Preset> {
    const body = await this.request("POST", "/api/presets", {
      name,
      controls,
    });
    return body as Preset;
  }

  async remove(name: string): Promise<void> {
    await this.request(
      "DELETE",
      `/api/presets?name=${encodeURIComponent(name)}`,
    );
  }

  private async request(
    method: string,
    path: string,
    payload?: unknown,
  ): Promise<unknown> {
    const headers: Record<string, string> = {
      ...authHeaders(this.token),
      Accept: "application/json",
    };
    if (payload !== undefined) headers["Content-Type"] = "application/json";
    let response: Response;
    try {
      response = await fetch(path, {
        method,
        headers,
        body: payload === undefined ? undefined : JSON.stringify(payload),
        cache: "no-store",
      });
    } catch (error) {
      throw new Error(`cannot reach the host: ${String(error)}`);
    }
    let body: unknown = null;
    try {
      body = await response.json();
    } catch {
      body = null;
    }
    if (!response.ok) {
      throw new Error(
        errorMessageFromBody(body) ?? `preset request failed with HTTP ${response.status}`,
      );
    }
    return body;
  }
}
