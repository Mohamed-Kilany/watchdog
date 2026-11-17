// Typed client for the live view endpoints (M6-T04).
//
// The viewfinder itself is a plain `<img>` pointed at `/stream.mjpg`; this
// module only fetches the rolling stats and the snapshot JPEG. Both reuse the
// same bearer token as the rest of the UI API.

import { DeviceApiError } from "./device-api";

/** Rolling stream counters returned by `GET /api/stats`. */
export interface StreamStats {
  frames: number;
  dropped: number;
  duplicates: number;
  fps: number;
  bitrate_kbps: number;
  latency_ms?: number;
  rtt_ms?: number;
  last_seq?: number;
  subscribers: number;
  closed: boolean;
  last_error?: string;
}

function errorFromBody(body: unknown): DeviceApiError {
  if (body && typeof body === "object") {
    const error = (body as Record<string, unknown>)["error"];
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

export class LiveApi {
  private readonly token: string | null;

  constructor(token: string | null) {
    this.token = token;
  }

  /** URL for the MJPEG viewfinder, with a revision cache-buster. */
  streamUrl(revision: number): string {
    const url = new URL("/stream.mjpg", window.location.href);
    if (this.token) url.searchParams.set("token", this.token);
    url.searchParams.set("rev", String(revision));
    return url.toString();
  }

  async stats(): Promise<StreamStats> {
    let response: Response;
    try {
      response = await fetch("/api/stats", {
        headers: this.headers(),
        cache: "no-store",
      });
    } catch (error) {
      throw new DeviceApiError(
        `host unreachable: ${error instanceof Error ? error.message : String(error)}`,
      );
    }
    let body: unknown = null;
    try {
      body = await response.json();
    } catch {
      body = null;
    }
    if (!response.ok) throw errorFromBody(body);
    return body as StreamStats;
  }

  /** Latest viewfinder frame as a JPEG download. */
  async snapshot(): Promise<Blob> {
    let response: Response;
    try {
      response = await fetch("/api/snapshot", {
        headers: this.headers(),
        cache: "no-store",
      });
    } catch (error) {
      throw new DeviceApiError(
        `host unreachable: ${error instanceof Error ? error.message : String(error)}`,
      );
    }
    if (!response.ok) {
      let body: unknown = null;
      try {
        body = await response.json();
      } catch {
        body = null;
      }
      throw errorFromBody(body);
    }
    return response.blob();
  }

  private headers(): Record<string, string> {
    return this.token ? { Authorization: `Bearer ${this.token}` } : {};
  }
}
