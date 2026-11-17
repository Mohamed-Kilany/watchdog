// Typed client for the host settings, logs and diagnostics (M6-T08).
//
// The host configuration (M1), log ring buffer (M1-T04) and ffmpeg bundle
// (M7) will back these routes. This module defines the UI-facing contract
// documented in docs/api.md and docs/ui.md so the page works as soon as the
// host serves it. Without a host the demo `FakeDevice` fills the same shape.

import { DeviceApiError } from "./device-api";
import type {
  FfmpegInfo,
  HostAbout,
  HostLogLine,
  HostRetention,
  HostSettings,
  HostStorage,
  ListenAddresses,
} from "./types";

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
    if (typeof record["message"] === "string") {
      return new DeviceApiError(
        record["message"],
        String(record["code"] ?? "HTTP_ERROR"),
      );
    }
  }
  return new DeviceApiError("request failed");
}

function recordOf(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};
}

function asNumber(value: unknown, fallback = 0): number {
  const number = Number(value);
  return Number.isFinite(number) ? number : fallback;
}

function asString(value: unknown): string | undefined {
  return typeof value === "string" && value.length > 0 ? value : undefined;
}

function optionalNumber(value: unknown): number | undefined {
  if (value === undefined || value === null) return undefined;
  const number = Number(value);
  return Number.isFinite(number) ? number : undefined;
}

function normalizeLevel(value: unknown): HostLogLine["level"] {
  switch (value) {
    case "trace":
    case "debug":
    case "warn":
    case "error":
      return value;
    default:
      return "info";
  }
}

/** Normalize one host log line (`ts_ms`, `level`, `target`, `message`). */
export function normalizeLogLine(value: unknown): HostLogLine | null {
  if (!value || typeof value !== "object") return null;
  const record = value as Record<string, unknown>;
  const message = asString(record["message"]) ?? asString(record["msg"]);
  if (!message) return null;
  const line: HostLogLine = {
    ts_ms: asNumber(record["ts_ms"] ?? record["timestamp_ms"] ?? Date.now()),
    level: normalizeLevel(record["level"]),
    message,
  };
  const target = asString(record["target"]) ?? asString(record["module"]);
  if (target) line.target = target;
  return line;
}

function normalizeRetention(value: unknown): HostRetention {
  const record = recordOf(value);
  return {
    max_age_days: Math.max(
      0,
      Math.round(asNumber(record["max_age_days"] ?? record["maxAgeDays"], 30)),
    ),
    max_size_mb: Math.max(
      0,
      Math.round(asNumber(record["max_size_mb"] ?? record["maxSizeMb"], 10_240)),
    ),
  };
}

/** Normalize the settings payload, tolerating missing optional groups. */
export function normalizeSettings(value: unknown): HostSettings {
  const record = recordOf(value);
  const ports = recordOf(record["ports"]);
  const listen = recordOf(record["listen"]);
  const addresses = recordOf(listen["addresses"]);
  const token = recordOf(record["token"]);
  const storage = recordOf(record["storage"]);
  const ffmpeg = recordOf(record["ffmpeg"]);
  const about = recordOf(record["about"]);

  const ui = asString(addresses["ui"]) ?? asString(listen["ui"]);
  const mcp = asString(addresses["mcp"]) ?? asString(listen["mcp"]);
  const api = asString(addresses["api"]) ?? asString(listen["api"]);
  const listenAddresses: ListenAddresses = {
    ui: ui ?? "127.0.0.1",
    mcp: mcp ?? "127.0.0.1",
    api: api ?? "127.0.0.1",
  };
  const exposed = [ui, mcp, api].filter(
    (address) => address && address !== "127.0.0.1" && address !== "::1",
  ).length;
  const total = [ui, mcp, api].filter(Boolean).length;
  const rawMode = listen["mode"];
  const mode: HostSettings["listen"]["mode"] =
    rawMode === "lan" || rawMode === "mixed"
      ? rawMode
      : exposed > 0 && total > 0
        ? exposed === total
          ? "lan"
          : "mixed"
        : "localhost";

  const settings: HostSettings = {
    ports: {
      ui: asNumber(ports["ui"], 7700),
      mcp: asNumber(ports["mcp"], 7701),
      api: asNumber(ports["api"], 7702),
    },
    listen: { mode, addresses: listenAddresses },
    token: {
      configured: token["configured"] === true || Boolean(asString(token["hint"])),
    },
    storage: normalizeStorage(storage),
    retention: normalizeRetention(record["retention"]),
    ffmpeg: normalizeFfmpeg(ffmpeg),
    about: normalizeAbout(about),
  };
  const hint = asString(token["hint"]);
  if (hint) settings.token.hint = hint;
  return settings;
}

function normalizeStorage(value: Record<string, unknown>): HostStorage {
  const storage: HostStorage = {
    data_dir: asString(value["data_dir"]) ?? "unknown",
    media_dir: asString(value["media_dir"]) ?? "unknown",
    logs_dir: asString(value["logs_dir"]) ?? "unknown",
    media_items: Math.max(0, asNumber(value["media_items"])),
    media_bytes: Math.max(0, asNumber(value["media_bytes"])),
  };
  const free = optionalNumber(value["free_bytes"]);
  const total = optionalNumber(value["total_bytes"]);
  if (free !== undefined) storage.free_bytes = free;
  if (total !== undefined) storage.total_bytes = total;
  return storage;
}

function normalizeFfmpeg(value: Record<string, unknown>): FfmpegInfo {
  const ffmpeg: FfmpegInfo = { available: value["available"] === true };
  const version = asString(value["ffmpeg_version"]);
  const probe = asString(value["ffprobe_version"]);
  const path = asString(value["path"]);
  const license = asString(value["license"]);
  if (version) ffmpeg.ffmpeg_version = version;
  if (probe) ffmpeg.ffprobe_version = probe;
  if (path) ffmpeg.path = path;
  if (license) ffmpeg.license = license;
  const filters = optionalNumber(value["filters"]);
  if (filters !== undefined) ffmpeg.filters = filters;
  return ffmpeg;
}

function normalizeAbout(value: Record<string, unknown>): HostAbout {
  const about: HostAbout = {
    version: asString(value["version"]) ?? "unknown",
    os: asString(value["os"]) ?? "unknown",
    arch: asString(value["arch"]) ?? "unknown",
  };
  const commit = asString(value["commit"]);
  const build = asString(value["build"]);
  const protocol = optionalNumber(value["protocol"]);
  if (commit) about.commit = commit;
  if (build) about.build = build;
  if (protocol !== undefined) about.protocol = protocol;
  return about;
}

/** Host-backed settings, log and diagnostics client. */
export class SettingsApi {
  private readonly token: string | null;

  constructor(token: string | null) {
    this.token = token;
  }

  async get(): Promise<HostSettings> {
    const body = await this.request("GET", "/api/settings");
    const record =
      body && typeof body === "object" && "settings" in body
        ? (body as Record<string, unknown>)["settings"]
        : body;
    return normalizeSettings(record);
  }

  async saveRetention(retention: HostRetention): Promise<HostRetention> {
    const body = await this.request("PATCH", "/api/settings", {
      retention: {
        max_age_days: retention.max_age_days,
        max_size_mb: retention.max_size_mb,
      },
    });
    if (!body || typeof body !== "object") return retention;
    const record = body as Record<string, unknown>;
    const updated =
      record["retention"] !== undefined ? record["retention"] : record;
    return normalizeRetention(updated);
  }

  async logs(options: { level?: string; limit?: number } = {}): Promise<
    HostLogLine[]
  > {
    const url = new URL("/api/logs", window.location.href);
    if (options.level && options.level !== "all") {
      url.searchParams.set("level", options.level);
    }
    if (options.limit) url.searchParams.set("limit", String(options.limit));
    const body = await this.request("GET", url.pathname + url.search);
    const raw = Array.isArray(body)
      ? body
      : body && typeof body === "object"
        ? ((body as Record<string, unknown>)["lines"] ??
          (body as Record<string, unknown>)["items"])
        : [];
    if (!Array.isArray(raw)) return [];
    return raw
      .map(normalizeLogLine)
      .filter((line): line is HostLogLine => line !== null);
  }

  /** Diagnostics bundle URL; the download element cannot send bearer headers. */
  diagnosticsUrl(): string {
    const url = new URL("/api/diagnostics", window.location.href);
    if (this.token) url.searchParams.set("token", this.token);
    return url.toString();
  }

  private async request(
    method: "GET" | "PATCH",
    path: string,
    payload?: unknown,
  ): Promise<unknown> {
    const headers: Record<string, string> = {};
    if (this.token) headers["Authorization"] = `Bearer ${this.token}`;
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
    return parsed;
  }
}
