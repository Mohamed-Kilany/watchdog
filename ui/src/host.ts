// Real-time host backend for the app shell (M6-T02).
//
// State and events arrive over `/ws`; commands go through `POST /api/command`.
// The channel reconnects with exponential backoff and the shell shows a global
// banner while it is down. The bearer token is kept in `sessionStorage` and is
// stripped from the URL after the first load.

import type {
  ApplyResult,
  BurstOptions,
  CaptureOutcome,
  ControlChange,
  ControlState,
  DeviceBackend,
  DeviceEvent,
  RecordOptions,
  TransferProgress,
} from "./types";
import type { SessionInfo } from "./device-api";

const TOKEN_KEY = "watchdog.token";
const RECONNECT_BASE_MS = 500;
const RECONNECT_MAX_MS = 15_000;

/** Connection state reported to the shell banner. */
export type ConnectionState = "connecting" | "live" | "reconnecting" | "idle";

/** One asynchronous app event forwarded by the host. */
export type HostEvent = DeviceEvent;

interface StateEnvelope {
  type?: string;
  state?: ControlState;
  waiting?: boolean;
  session?: SessionInfo;
}

interface WireControlResult {
  control?: string;
  ok?: boolean;
  applied?: unknown;
  error_code?: string;
  message?: string;
}

interface CommandEnvelope {
  type?: string;
  id?: string;
  results?: WireControlResult[];
  error?: string;
}

/** Outcome of the initial `/api/state` probe. */
export type ProbeResult =
  | { kind: "host"; state: ControlState; session: SessionInfo }
  | { kind: "unauthorized" }
  | { kind: "demo" };

/** Read the token kept for this browser tab, if any. */
export function readStoredToken(): string | null {
  try {
    return sessionStorage.getItem(TOKEN_KEY);
  } catch {
    return null;
  }
}

/** Remember the token for this browser tab. */
export function saveToken(token: string): void {
  try {
    sessionStorage.setItem(TOKEN_KEY, token);
  } catch {
    // Private modes can refuse storage; the URL probe still works.
  }
}

/**
 * Move `?token=` from the address bar into session storage.
 *
 * The host prints `http://<host>:7700/?token=...`; keeping it in the URL would
 * leak the token through history and screenshots, so it is consumed once.
 */
export function consumeTokenFromUrl(): string | null {
  const url = new URL(window.location.href);
  const token = url.searchParams.get("token");
  if (!token) return null;
  saveToken(token);
  url.searchParams.delete("token");
  history.replaceState(
    history.state,
    "",
    `${url.pathname}${url.search}${url.hash}`,
  );
  return token;
}

export function authHeaders(token: string | null): Record<string, string> {
  return token ? { Authorization: `Bearer ${token}` } : {};
}

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export function errorMessageFromBody(body: unknown): string | null {
  if (!body || typeof body !== "object") return null;
  const record = body as Record<string, unknown>;
  const error = record["error"];
  if (typeof error === "string") return error;
  if (error && typeof error === "object") {
    const message = (error as Record<string, unknown>)["message"];
    if (typeof message === "string") return message;
  }
  const message = record["message"];
  return typeof message === "string" ? message : null;
}

/**
 * Ask the same origin whether a watchdog host is serving this page.
 *
 * A plain `vite preview` answers `/api/state` with the SPA shell, so anything
 * that is not a `state` JSON envelope falls back to the offline demo.
 */
export async function probeHost(token: string | null): Promise<ProbeResult> {
  let response: Response;
  try {
    response = await fetch("/api/state", {
      headers: authHeaders(token),
      cache: "no-store",
    });
  } catch {
    return { kind: "demo" };
  }
  if (response.status === 401) return { kind: "unauthorized" };
  if (!response.ok) return { kind: "demo" };
  const contentType = response.headers.get("content-type") ?? "";
  if (!contentType.toLowerCase().includes("application/json")) {
    return { kind: "demo" };
  }
  let body: StateEnvelope | null = null;
  try {
    body = (await response.json()) as StateEnvelope;
  } catch {
    return { kind: "demo" };
  }
  if (
    !body ||
    body.type !== "state" ||
    !body.state ||
    typeof body.state !== "object"
  ) {
    return { kind: "demo" };
  }
  const session = body.session ?? { active: true, closed: false };
  return { kind: "host", state: body.state, session };
}

/** New correlation id for one command round trip. */
function commandId(): string {
  try {
    if (typeof crypto.randomUUID === "function") return crypto.randomUUID();
  } catch {
    // Fall through for non-secure origins.
  }
  return `ui-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}

function asRegionArray(value: unknown): unknown {
  return Array.isArray(value) ? value : [value];
}

/**
 * Reset batches for the descriptor groups.
 *
 * Device-driven values (`iso`, `focus_distance`, readouts) are deliberately
 * absent: switching the mode back to auto is what makes the app clear them.
 * Regions are cleared with a full-frame rectangle of weight 0 because the app
 * rejects an empty region array.
 */
const RESET_VALUES: Record<string, unknown> = {
  control_mode: 1,
  ae_mode: 1,
  ae_lock: false,
  ae_compensation: 0,
  anti_banding: 3,
  ae_fps_range: [30, 30],
  ae_precapture: "cancel",
  af_mode: 4,
  af_lock: false,
  af_trigger: "cancel",
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

const CLEARED_REGION = { x: 0, y: 0, w: 1, h: 1, weight: 0 };

function resetChanges(targets: string[]): ControlChange[] {
  const ids = targets.includes("all") ? Object.keys(RESET_VALUES) : targets;
  const changes: ControlChange[] = [];
  for (const id of ids) {
    if (!Object.hasOwn(RESET_VALUES, id)) continue;
    changes.push({ id, value: structuredClone(RESET_VALUES[id]) });
  }
  const all = targets.includes("all");
  if (all || targets.includes("ae_regions")) {
    changes.push({ id: "ae_regions", value: [CLEARED_REGION] });
  }
  if (all || targets.includes("af_regions")) {
    changes.push({ id: "af_regions", value: [CLEARED_REGION] });
  }
  if (all || targets.includes("awb_regions")) {
    changes.push({ id: "awb_regions", value: [CLEARED_REGION] });
  }
  return changes;
}

function mapResults(body: CommandEnvelope | null): ApplyResult[] {
  const results: ApplyResult[] = [];
  for (const item of body?.results ?? []) {
    const result: ApplyResult = {
      control: item.control ?? "command",
      ok: item.ok === true,
    };
    if (item.applied !== undefined) result.applied = item.applied;
    if (item.ok !== true) {
      result.error = item.message ?? item.error_code ?? "rejected";
    }
    results.push(result);
  }
  if (body?.error) {
    results.push({ control: "command", ok: false, error: body.error });
  }
  if (results.length === 0) {
    results.push({ control: "command", ok: true });
  }
  return results;
}

/** Normalize the `applied` payload of a capture/record command. */
function captureOutcome(applied: unknown): CaptureOutcome {
  const outcome: CaptureOutcome = { applied };
  if (!applied || typeof applied !== "object") return outcome;
  const record = applied as Record<string, unknown>;
  const text = (key: string): string | undefined =>
    typeof record[key] === "string" ? (record[key] as string) : undefined;
  const number = (key: string): number | undefined =>
    Number.isFinite(Number(record[key])) ? Number(record[key]) : undefined;
  outcome.mediaId = text("media_id") ?? text("mediaId");
  outcome.sequenceId = text("sequence_id") ?? text("sequenceId");
  outcome.type = text("type");
  outcome.bytes = number("bytes");
  outcome.width = number("width");
  outcome.height = number("height");
  outcome.durationMs = number("duration_ms") ?? number("durationMs");
  outcome.startedMs = number("started_ms") ?? number("startedMs");
  const ids = record["media_ids"] ?? record["mediaIds"];
  if (Array.isArray(ids)) {
    outcome.mediaIds = ids.filter((id): id is string => typeof id === "string");
  }
  return outcome;
}

/** Normalize a host `transfer` WebSocket envelope. */
function transferFrom(message: Record<string, unknown>): TransferProgress | null {
  const transfer = message["transfer"];
  const record =
    transfer && typeof transfer === "object"
      ? (transfer as Record<string, unknown>)
      : message;
  const mediaId = record["media_id"] ?? record["mediaId"];
  const transferId = record["transfer_id"] ?? record["transferId"];
  const state = record["state"];
  if (
    typeof mediaId !== "string" ||
    typeof transferId !== "string" ||
    typeof state !== "string"
  ) {
    return null;
  }
  const bytesTotal = Number(record["bytes_total"] ?? record["bytesTotal"] ?? 0);
  const bytesDone = Number(record["bytes_done"] ?? record["bytesDone"] ?? 0);
  const progress: TransferProgress = {
    transferId,
    mediaId,
    state: state as TransferProgress["state"],
    bytesTotal: Number.isFinite(bytesTotal) ? bytesTotal : 0,
    bytesDone: Number.isFinite(bytesDone) ? bytesDone : 0,
  };
  const error = record["error"];
  if (typeof error === "string") progress.error = error;
  return progress;
}

/**
 * Host-backed device: merged state over WebSocket, commands over REST.
 *
 * One instance owns one reconnecting channel. `start()` opens it; state
 * snapshots replace the local map because the host already merges deltas.
 */
export class HostClient implements DeviceBackend {
  readonly mode = "host" as const;

  private readonly token: string | null;
  private state: ControlState;
  private readonly listeners = new Set<(state: ControlState) => void>();
  private readonly eventListeners = new Set<(event: HostEvent) => void>();
  private readonly connectionListeners = new Set<
    (state: ConnectionState) => void
  >();
  private readonly sessionListeners = new Set<(session: SessionInfo) => void>();
  private readonly transferListeners = new Set<
    (transfer: TransferProgress) => void
  >();
  private connection: ConnectionState = "connecting";
  private session: SessionInfo;
  private socket: WebSocket | null = null;
  private reconnectTimer: number | null = null;
  private attempt = 0;
  private started = false;
  private disposed = false;

  constructor(
    token: string | null,
    initialState: ControlState = {},
    initialSession: SessionInfo = { active: true, closed: false },
  ) {
    this.token = token;
    this.state = structuredClone(initialState);
    this.session = initialSession;
  }

  get snapshot(): ControlState {
    return structuredClone(this.state);
  }

  /** The active app session as the host last reported it. */
  get currentSession(): SessionInfo {
    return this.session;
  }

  /** Open the channel and keep it open until `stop()` is called. */
  start(): void {
    if (this.started) return;
    this.started = true;
    this.openSocket();
  }

  /** Close the channel and cancel pending reconnects. */
  stop(): void {
    this.disposed = true;
    if (this.reconnectTimer !== null) {
      window.clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
    const socket = this.socket;
    this.socket = null;
    socket?.close();
  }

  subscribe(listener: (state: ControlState) => void): () => void {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  subscribeEvents(listener: (event: HostEvent) => void): () => void {
    this.eventListeners.add(listener);
    return () => {
      this.eventListeners.delete(listener);
    };
  }

  subscribeTransfers(listener: (transfer: TransferProgress) => void): () => void {
    this.transferListeners.add(listener);
    return () => {
      this.transferListeners.delete(listener);
    };
  }

  subscribeConnection(
    listener: (state: ConnectionState) => void,
  ): () => void {
    this.connectionListeners.add(listener);
    listener(this.connection);
    return () => {
      this.connectionListeners.delete(listener);
    };
  }

  subscribeSession(listener: (session: SessionInfo) => void): () => void {
    this.sessionListeners.add(listener);
    listener(this.session);
    return () => {
      this.sessionListeners.delete(listener);
    };
  }

  /**
   * Adopt a session reported by the devices page.
   *
   * The WebSocket sends the same envelope; this path keeps the banner and the
   * controls page in sync when the API call finishes first.
   */
  applySession(session: SessionInfo): void {
    this.setSession(session);
    if (this.socket?.readyState === WebSocket.OPEN) {
      this.setConnection(session.active ? "live" : "idle");
    }
  }

  /** Drop the current socket and connect again without waiting for backoff. */
  reconnectNow(): void {
    if (this.disposed || !this.started) return;
    if (this.reconnectTimer !== null) {
      window.clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
    this.attempt = 0;
    const socket = this.socket;
    this.socket = null;
    socket?.close();
    this.openSocket();
  }

  async setControls(changes: ControlChange[]): Promise<ApplyResult[]> {
    const controls = changes.map((change) => ({
      id: change.id,
      value: change.id.endsWith("_regions")
        ? asRegionArray(change.value)
        : change.value,
    }));
    return this.sendCommand("set_controls", { controls });
  }

  async reset(targets: string[]): Promise<ApplyResult[]> {
    const changes = resetChanges(targets);
    if (changes.length === 0) {
      return [{ control: "reset", ok: true }];
    }
    return this.setControls(changes);
  }

  async clearRegions(): Promise<ApplyResult[]> {
    return this.setControls([
      { id: "ae_regions", value: [CLEARED_REGION] },
      { id: "af_regions", value: [CLEARED_REGION] },
      { id: "awb_regions", value: [CLEARED_REGION] },
      { id: "af_trigger", value: "cancel" },
    ]);
  }

  captureStill(): Promise<CaptureOutcome> {
    return this.sendCapture("capture_still", {});
  }

  captureBurst(options: BurstOptions): Promise<CaptureOutcome> {
    return this.sendCapture("capture_burst", {
      count: options.count,
      interval_ms: options.intervalMs,
      from_stream: options.fromStream,
    });
  }

  recordStart(options: RecordOptions = {}): Promise<CaptureOutcome> {
    const params: Record<string, unknown> = {};
    if (options.maxDurationSecs && options.maxDurationSecs > 0) {
      params["max_duration_secs"] = options.maxDurationSecs;
    }
    return this.sendCapture("record_start", params);
  }

  recordStop(): Promise<CaptureOutcome> {
    return this.sendCapture("record_stop", {});
  }

  private async sendCapture(
    command: string,
    params: unknown,
  ): Promise<CaptureOutcome> {
    const results = await this.sendCommand(command, params);
    const result =
      results.find((item) => item.control === "command") ?? results[0];
    if (!result) throw new Error(`${command} returned no result`);
    if (!result.ok) {
      throw new Error(result.error ?? `${command} was rejected`);
    }
    return captureOutcome(result.applied);
  }

  private async sendCommand(
    command: string,
    params: unknown,
  ): Promise<ApplyResult[]> {
    let response: Response;
    try {
      response = await fetch("/api/command", {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          ...authHeaders(this.token),
        },
        body: JSON.stringify({ id: commandId(), command, params }),
      });
    } catch (error) {
      throw new Error(`host unreachable: ${messageOf(error)}`);
    }
    let body: unknown = null;
    try {
      body = await response.json();
    } catch {
      body = null;
    }
    if (!response.ok) {
      throw new Error(
        errorMessageFromBody(body) ?? `command failed with HTTP ${response.status}`,
      );
    }
    return mapResults(body as CommandEnvelope | null);
  }

  private openSocket(): void {
    if (this.disposed) return;
    const url = new URL("/ws", window.location.href);
    url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
    if (this.token) url.searchParams.set("token", this.token);
    let socket: WebSocket;
    try {
      socket = new WebSocket(url.toString());
    } catch {
      this.scheduleReconnect();
      return;
    }
    this.socket = socket;

    socket.addEventListener("open", () => {
      if (this.socket !== socket) return;
      this.attempt = 0;
      this.setConnection(this.session.active ? "live" : "idle");
    });
    socket.addEventListener("message", (event) => {
      if (this.socket !== socket) return;
      if (typeof event.data === "string") this.handleMessage(event.data);
    });
    socket.addEventListener("close", () => {
      if (this.socket !== socket) return;
      this.socket = null;
      this.scheduleReconnect();
    });
    socket.addEventListener("error", () => {
      // The browser always follows `error` with `close`; reconnect there.
    });
  }

  private scheduleReconnect(): void {
    if (this.disposed) return;
    this.setConnection("reconnecting");
    const base = Math.min(
      RECONNECT_MAX_MS,
      RECONNECT_BASE_MS * 2 ** Math.min(this.attempt, 5),
    );
    const delay = Math.round(base * (0.8 + Math.random() * 0.4));
    this.attempt = Math.min(this.attempt + 1, 10);
    this.reconnectTimer = window.setTimeout(() => {
      this.reconnectTimer = null;
      this.openSocket();
    }, delay);
  }

  private setConnection(state: ConnectionState): void {
    if (this.connection === state) return;
    this.connection = state;
    for (const listener of this.connectionListeners) listener(state);
  }

  private setSession(session: SessionInfo): void {
    this.session = session;
    for (const listener of this.sessionListeners) listener(session);
  }

  private sessionFrom(message: Record<string, unknown>): SessionInfo | null {
    const session = message["session"];
    if (!session || typeof session !== "object") return null;
    const record = session as Record<string, unknown>;
    return {
      active: record["active"] === true,
      closed: record["closed"] === true,
      ...(typeof record["serial"] === "string"
        ? { serial: record["serial"] }
        : {}),
      ...(record["device"] && typeof record["device"] === "object"
        ? { device: record["device"] as SessionInfo["device"] }
        : {}),
    };
  }

  private handleMessage(text: string): void {
    let message: Record<string, unknown>;
    try {
      message = JSON.parse(text) as Record<string, unknown>;
    } catch {
      return;
    }
    switch (message["type"]) {
      case "hello":
      case "session": {
        const session = this.sessionFrom(message);
        if (session) {
          this.setSession(session);
          this.setConnection(session.active ? "live" : "idle");
        }
        break;
      }
      case "state": {
        const session = this.sessionFrom(message);
        if (session) this.setSession(session);
        const state = message["state"];
        if (!state || typeof state !== "object") return;
        this.state = state as ControlState;
        for (const listener of this.listeners) listener(this.snapshot);
        break;
      }
      case "event": {
        const event: HostEvent = {
          kind: String(message["kind"] ?? "event"),
        };
        if (typeof message["timestamp_ms"] === "number") {
          event.timestamp_ms = message["timestamp_ms"];
        }
        if (message["data"] !== undefined) event.data = message["data"];
        for (const listener of this.eventListeners) listener(event);
        break;
      }
      case "transfer": {
        const transfer = transferFrom(message);
        if (transfer) {
          for (const listener of this.transferListeners) listener(transfer);
        }
        break;
      }
      case "closed": {
        this.setSession({
          ...this.session,
          active: false,
          closed: true,
        });
        this.setConnection("idle");
        break;
      }
      case "error": {
        const event: HostEvent = {
          kind: "error",
          message: String(message["message"] ?? "host error"),
        };
        for (const listener of this.eventListeners) listener(event);
        break;
      }
      default:
        break;
    }
  }
}
