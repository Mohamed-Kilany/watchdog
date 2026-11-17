// Typed client for the host media library (M6-T07).
//
// The media store itself lands with M5; this module defines the UI-facing
// contract documented in docs/api.md and docs/ui.md so the page can talk to a
// host as soon as the routes exist. Without a host the demo `FakeDevice`
// provides an in-memory library.

import { DeviceApiError } from "./device-api";
import type { AnalysisSubmission, MediaDeviceInfo, MediaItem } from "./types";

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

function asNumber(value: unknown, fallback = 0): number {
  const number = Number(value);
  return Number.isFinite(number) ? number : fallback;
}

function asString(value: unknown): string | undefined {
  return typeof value === "string" && value.length > 0 ? value : undefined;
}

/** Normalize one host item into the UI's `MediaItem` shape. */
export function normalizeMediaItem(value: unknown): MediaItem | null {
  if (!value || typeof value !== "object") return null;
  const record = value as Record<string, unknown>;
  const id = asString(record["id"]);
  if (!id) return null;
  const rawType = record["type"] ?? record["kind"];
  const type = rawType === "video" ? "video" : "still";
  const deviceRecord: Record<string, unknown> =
    record["device"] && typeof record["device"] === "object"
      ? (record["device"] as Record<string, unknown>)
      : {};
  const device: MediaDeviceInfo = {
    model: asString(deviceRecord["model"]) ?? "unknown device",
  };
  const serial = asString(deviceRecord["serial"]);
  if (serial) device.serial = serial;
  if (deviceRecord["sdk"] !== undefined) {
    device.sdk = asNumber(deviceRecord["sdk"]);
  }
  const snapshot = record["control_snapshot"];
  const item: MediaItem = {
    id,
    type,
    created_ms: asNumber(record["created_ms"] ?? record["created_at_ms"]),
    width: asNumber(record["width"]),
    height: asNumber(record["height"]),
    bytes: asNumber(record["bytes"]),
    sha256: asString(record["sha256"]) ?? "",
    mime:
      asString(record["mime"]) ??
      (type === "video" ? "video/mp4" : "image/jpeg"),
    device,
  };
  if (record["duration_ms"] !== undefined) {
    item.duration_ms = asNumber(record["duration_ms"]);
  }
  if (snapshot && typeof snapshot === "object" && !Array.isArray(snapshot)) {
    item.control_snapshot = snapshot as Record<string, unknown>;
  }
  if (record["host_job_id"] !== undefined) {
    item.host_job_id =
      record["host_job_id"] === null ? null : String(record["host_job_id"]);
  }
  return item;
}

/** Host-backed media library and analysis queue client. */
export class MediaApi {
  private readonly token: string | null;

  constructor(token: string | null) {
    this.token = token;
  }

  async list(): Promise<MediaItem[]> {
    const body = await this.request("GET", "/api/media");
    const rawItems = Array.isArray(body)
      ? body
      : body && typeof body === "object"
        ? ((body as Record<string, unknown>)["items"] as unknown[])
        : [];
    if (!Array.isArray(rawItems)) return [];
    return rawItems
      .map(normalizeMediaItem)
      .filter((item): item is MediaItem => item !== null);
  }

  async get(id: string): Promise<MediaItem | null> {
    const body = await this.request(
      "GET",
      `/api/media/${encodeURIComponent(id)}`,
    );
    const record =
      body && typeof body === "object" && "item" in body
        ? (body as Record<string, unknown>)["item"]
        : body;
    return normalizeMediaItem(record);
  }

  async remove(id: string): Promise<void> {
    await this.request("DELETE", `/api/media/${encodeURIComponent(id)}`);
  }

  /** URL for the original file; media elements cannot send bearer headers. */
  fileUrl(id: string): string {
    const url = new URL(
      `/api/media/${encodeURIComponent(id)}/file`,
      window.location.href,
    );
    if (this.token) url.searchParams.set("token", this.token);
    return url.toString();
  }

  async analyze(ids: string[]): Promise<AnalysisSubmission> {
    const body = await this.request("POST", "/api/analyses", {
      media_ids: ids,
    });
    if (!body || typeof body !== "object") {
      throw new DeviceApiError("analysis response was empty");
    }
    const record = body as Record<string, unknown>;
    const id = asString(record["analysis_id"]) ?? asString(record["id"]);
    if (!id) throw new DeviceApiError("analysis response has no id");
    return {
      id,
      state:
        asString(record["state"]) ??
        asString(record["status"]) ??
        "queued",
    };
  }

  private async request(
    method: "GET" | "POST" | "DELETE",
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
