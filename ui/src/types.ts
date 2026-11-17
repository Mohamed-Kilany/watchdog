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
}

/** Reset-to-auto button for a group of controls. */
export interface ResetDescriptor extends DescriptorBase {
  kind: "reset";
  targets: string[];
}

export type ControlDescriptor =
  | AutoDescriptor
  | LockDescriptor
  | SliderDescriptor
  | SelectDescriptor
  | ResetDescriptor;

export interface CommandLogEntry {
  batch: ControlChange[];
  results: ApplyResult[];
  at: string;
  /** Free-form note for entries that are not control batches (host events). */
  note?: string;
}
