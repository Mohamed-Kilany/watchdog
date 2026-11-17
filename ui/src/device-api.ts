// Typed client for the host's device-management API (M6-T03).
//
// Every request reuses the same bearer token as the live channel. The devices
// page polls provisioning jobs here; the WebSocket carries the session switch
// separately so the controls page follows along without a reload.

/** One app session as reported by the host. */
export interface SessionInfo {
  active: boolean;
  closed: boolean;
  serial?: string;
  device?: ActiveDevice;
}

/** Handshake details of the active (or last) session. */
export interface ActiveDevice {
  serial: string;
  kind: "usb" | "lan";
  manufacturer: string;
  model: string;
  device: string;
  sdk: number;
  release: string;
  abi: string;
  app_package: string;
  app_version_name: string;
  app_version_code: number;
  protocol_version: number;
  camera_count: number;
  battery_percent?: number;
  storage_free_mb?: number;
}

/** One row on the devices page. */
export interface DeviceEntry {
  serial: string;
  kind: "usb" | "lan";
  state: "active" | "closed" | "available" | "discovered";
  model?: string;
  product?: string;
  connect?: string;
  pairing?: string;
  guid?: string;
  last_seen_ms?: number;
}

/** `GET /api/devices` response. */
export interface DeviceList {
  session: SessionInfo;
  devices: DeviceEntry[];
  discovered_at_ms?: number;
}

/** One provisioning step. */
export interface StepReport {
  name: string;
  status: "passed" | "failed" | "skipped";
  duration_ms: number;
  detail: string;
  hint?: string;
}

/** Error details attached to a failed job or request. */
export interface JobError {
  code: string;
  message: string;
  hint?: string;
}

/** One background provisioning job. */
export interface ProvisionJob {
  id: string;
  serial: string;
  status: "running" | "passed" | "failed";
  steps: StepReport[];
  started_ms: number;
  finished_ms?: number;
  error?: JobError;
  session?: SessionInfo;
  session_error?: JobError;
}

/** App/permission status from `GET /api/devices/status`. */
export interface StatusReport {
  serial: string;
  props: {
    sdk: number;
    abi: string;
    model: string;
    manufacturer: string;
    release: string;
    device: string;
  };
  installed: boolean;
  version_code?: number;
  expected_version_name?: string;
  expected_version_code?: number;
  up_to_date?: boolean;
  service_pid?: number;
  token?: string;
  restore?: { originals?: Record<string, string> };
  appops_camera?: string;
  appops_record_audio?: string;
}

/** One restore step. */
export interface RestoreStep {
  setting: string;
  value: string;
  applied: boolean;
}

/** Raised for non-2xx API responses. */
export class DeviceApiError extends Error {
  readonly code: string;
  readonly hint?: string;

  constructor(message: string, code = "HTTP_ERROR", hint?: string) {
    super(message);
    this.name = "DeviceApiError";
    this.code = code;
    this.hint = hint;
  }
}

function errorFromBody(body: unknown): DeviceApiError {
  if (body && typeof body === "object") {
    const record = body as Record<string, unknown>;
    const error = record["error"];
    if (error && typeof error === "object") {
      const detail = error as Record<string, unknown>;
      return new DeviceApiError(
        String(detail["message"] ?? "request failed"),
        String(detail["code"] ?? "HTTP_ERROR"),
        typeof detail["hint"] === "string" ? detail["hint"] : undefined,
      );
    }
    if (typeof error === "string") return new DeviceApiError(error);
  }
  return new DeviceApiError("request failed");
}

/** Host-backed device API using the page's bearer token. */
export class DeviceApi {
  private readonly token: string | null;

  constructor(token: string | null) {
    this.token = token;
  }

  private async request<T>(
    method: "GET" | "POST",
    path: string,
    body?: unknown,
  ): Promise<T> {
    const headers: Record<string, string> = {};
    if (this.token) headers["Authorization"] = `Bearer ${this.token}`;
    if (body !== undefined) headers["Content-Type"] = "application/json";
    let response: Response;
    try {
      response = await fetch(path, {
        method,
        headers,
        body: body === undefined ? undefined : JSON.stringify(body),
        cache: "no-store",
      });
    } catch (error) {
      throw new DeviceApiError(
        `host unreachable: ${error instanceof Error ? error.message : String(error)}`,
      );
    }
    let parsed: unknown = null;
    try {
      parsed = await response.json();
    } catch {
      parsed = null;
    }
    if (!response.ok) throw errorFromBody(parsed);
    return parsed as T;
  }

  list(discover = false): Promise<DeviceList> {
    return discover
      ? this.request("POST", "/api/devices/discover")
      : this.request("GET", "/api/devices");
  }

  pair(target: string, code: string): Promise<{ ok: boolean; guid: string }> {
    return this.request("POST", "/api/devices/pair", { target, code });
  }

  activate(serial: string): Promise<{ ok: boolean; session: SessionInfo }> {
    return this.request("POST", "/api/devices/activate", { serial });
  }

  disconnect(): Promise<{ ok: boolean; session: SessionInfo }> {
    return this.request("POST", "/api/devices/disconnect", {});
  }

  restore(serial: string): Promise<{
    ok: boolean;
    serial: string;
    steps: RestoreStep[];
  }> {
    return this.request("POST", "/api/devices/restore", { serial });
  }

  provision(
    serial: string,
    options: { verify?: boolean; screen?: boolean } = {},
  ): Promise<{ id: string; job: ProvisionJob | null }> {
    return this.request("POST", "/api/devices/provision", {
      serial,
      verify: options.verify ?? true,
      screen: options.screen ?? true,
    });
  }

  provisionJob(id: string): Promise<ProvisionJob> {
    return this.request("GET", `/api/devices/provision/${encodeURIComponent(id)}`);
  }

  status(serial: string): Promise<StatusReport> {
    return this.request(
      "GET",
      `/api/devices/status?serial=${encodeURIComponent(serial)}`,
    );
  }
}
