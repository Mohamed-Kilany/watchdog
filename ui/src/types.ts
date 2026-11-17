/** Shared data model for the host control UI. */

/** A scalar value accepted by a control. */
export type ControlValue = string | number | boolean;

/** One normalized region, matching the host's `ae_regions` / `af_regions`. */
export interface ControlRegion {
  x: number;
  y: number;
  w: number;
  h: number;
  weight?: number;
}

/**
 * One entry from `watchdog device controls --json` (`state.<id>`).
 *
 * The host sends `requested` only when a value was requested; device-driven
 * values (auto exposure, reported focus distance, ...) have only `actual`.
 */
export interface ControlEntry {
  supported: boolean;
  requested?: unknown;
  actual?: unknown;
  source?: string;
  locked?: boolean;
}

export type ControlState = Record<string, ControlEntry>;

export interface ControlChange {
  id: string;
  value: unknown;
}

/** One per-control outcome, mirroring the app's `COMMAND_RESULT.results`. */
export interface ApplyResult {
  control: string;
  ok: boolean;
  applied?: unknown;
  error?: string;
}

/** Where the shell is getting state from. */
export type DeviceMode = "demo" | "host";

/** Options for a burst/sequence capture. */
export interface BurstOptions {
  count: number;
  intervalMs: number;
  fromStream: boolean;
}

/** Options for starting a video recording. */
export interface RecordOptions {
  maxDurationSecs?: number;
}

/** Normalized outcome of one capture or recording command. */
export interface CaptureOutcome {
  mediaId?: string;
  mediaIds?: string[];
  sequenceId?: string;
  type?: string;
  bytes?: number;
  width?: number;
  height?: number;
  durationMs?: number;
  startedMs?: number;
  /** Raw `applied` payload from the device, for diagnostics. */
  applied?: unknown;
}

/** One asynchronous device event (recording lifecycle, capture progress). */
export interface DeviceEvent {
  kind: string;
  timestamp_ms?: number;
  data?: unknown;
  message?: string;
}

/** Host-side media transfer progress. */
export interface TransferProgress {
  transferId: string;
  mediaId: string;
  state: "queued" | "transferring" | "verifying" | "done" | "failed";
  bytesTotal: number;
  bytesDone: number;
  error?: string;
}

/** Device information attached to one media item. */
export interface MediaDeviceInfo {
  model: string;
  serial?: string;
  sdk?: number;
}

/**
 * One item in the host media library, matching the protocol's
 * `MEDIA_MANIFEST` item plus the host's storage metadata.
 */
export interface MediaItem {
  id: string;
  type: "still" | "video";
  created_ms: number;
  duration_ms?: number;
  width: number;
  height: number;
  bytes: number;
  sha256: string;
  mime: string;
  device: MediaDeviceInfo;
  control_snapshot?: Record<string, unknown>;
  host_job_id?: string | null;
}

/** Result of submitting media to the analysis queue. */
export interface AnalysisSubmission {
  id: string;
  state: string;
}

/**
 * The shell's device contract. `FakeDevice` implements it for the offline demo
 * and `HostClient` implements it against the host's real-time channel.
 */
export interface DeviceBackend {
  readonly mode: DeviceMode;
  readonly snapshot: ControlState;
  subscribe(listener: (state: ControlState) => void): () => void;
  setControls(changes: ControlChange[]): ApplyResult[] | Promise<ApplyResult[]>;
  reset(targets: string[]): ApplyResult[] | Promise<ApplyResult[]>;
  clearRegions(): ApplyResult[] | Promise<ApplyResult[]>;
  captureStill(): Promise<CaptureOutcome>;
  captureBurst(options: BurstOptions): Promise<CaptureOutcome>;
  recordStart(options?: RecordOptions): Promise<CaptureOutcome>;
  recordStop(): Promise<CaptureOutcome>;
  subscribeEvents(listener: (event: DeviceEvent) => void): () => void;
  subscribeTransfers(listener: (transfer: TransferProgress) => void): () => void;
  listMedia(): Promise<MediaItem[]>;
  getMedia(id: string): Promise<MediaItem | null>;
  deleteMedia(id: string): Promise<void>;
  mediaFileUrl(id: string): string;
  analyzeMedia(ids: string[]): Promise<AnalysisSubmission>;
}

export interface SelectOption {
  value: ControlValue;
  label: string;
}

interface DescriptorBase {
  id: string;
  label: string;
  group: string;
  help?: string;
}

/** Auto/manual toggle. `autoValue` is the device enum for automatic mode. */
export interface AutoDescriptor extends DescriptorBase {
  kind: "auto";
  autoValue: ControlValue;
  manualValue: ControlValue;
  autoLabel?: string;
  manualLabel?: string;
}

/** Lock button backed by a live actual value. */
export interface LockDescriptor extends DescriptorBase {
  kind: "lock";
}

/** Range slider with a numeric entry. `scale` converts UI units to device units. */
export interface SliderDescriptor extends DescriptorBase {
  kind: "slider";
  min: number;
  max: number;
  step: number;
  unit?: string;
  precision?: number;
  scale?: number;
  /** When set, the slider is disabled while this auto control is automatic. */
  autoId?: string;
  disabledWhenAuto?: boolean;
}

/** Enum control. */
export interface SelectDescriptor extends DescriptorBase {
  kind: "select";
  options: SelectOption[];
  /**
   * Encode the option value before sending it. `size` turns `"1280x720"` into
   * `{width: 1280, height: 720}` and `fps-range` turns `"30-30"` into
   * `[30, 30]`, matching the device's structural and FPS-range controls.
   */
  format?: "size" | "fps-range";
}

/** Reset-to-auto button for a group of controls. */
export interface ResetDescriptor extends DescriptorBase {
  kind: "reset";
  targets: string[];
}

/** Read-only live state entry (no input), shown with its reported value. */
export interface ReadoutDescriptor extends DescriptorBase {
  kind: "readout";
  unit?: string;
  precision?: number;
  /** Optional enum labels for numeric or string states. */
  options?: SelectOption[];
}

/** Boolean switch backed by a `true`/`false` control such as torch or audio. */
export interface ToggleDescriptor extends DescriptorBase {
  kind: "toggle";
}

/** One-shot action button such as `af_trigger` or `ae_precapture`. */
export interface TriggerDescriptor extends DescriptorBase {
  kind: "trigger";
  value: ControlValue;
  actionLabel?: string;
}

export type ControlDescriptor =
  | AutoDescriptor
  | LockDescriptor
  | SliderDescriptor
  | SelectDescriptor
  | ResetDescriptor
  | ReadoutDescriptor
  | ToggleDescriptor
  | TriggerDescriptor;

export interface CommandLogEntry {
  batch: ControlChange[];
  results: ApplyResult[];
  at: string;
  /** Free-form note for entries that are not control batches (host events). */
  note?: string;
}
