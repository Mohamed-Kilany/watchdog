// Live view page controller (M6-T04).
//
// The host viewfinder is a plain `<img>` pointed at `/stream.mjpg`: Chromium
// decodes the multipart JPEG stream natively and the TCP window applies
// backpressure, so no frame queue is kept in JavaScript (ADR-0008). This
// controller only opens/closes that stream with the session and the route,
// polls `/api/stats` once a second, and draws face boxes, the grid and the
// rotation hint in the DOM.

import { DeviceApiError } from "./device-api";
import type { SessionInfo } from "./device-api";
import type { WdFaceOverlay, FaceBox } from "./components/wd-face-overlay";
import type { ConnectionState, HostClient } from "./host";
import { LiveApi } from "./live-api";
import type { StreamStats } from "./live-api";
import type { ControlState } from "./types";

const STATS_POLL_MS = 1_000;
const STREAM_RETRY_MS = 2_000;
const NOTICE_MS = 4_000;

function mustQuery<T extends Element>(selector: string): T {
  const element = document.querySelector<T>(selector);
  if (!element) throw new Error(`missing element: ${selector}`);
  return element;
}

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function parseFaces(value: unknown): FaceBox[] {
  let list: unknown = value;
  if (typeof list === "string") {
    try {
      list = JSON.parse(list);
    } catch {
      return [];
    }
  }
  if (!Array.isArray(list)) return [];
  const faces: FaceBox[] = [];
  for (const item of list) {
    if (!item || typeof item !== "object") continue;
    const record = item as Record<string, unknown>;
    const face: FaceBox = {
      x: Number(record["x"]),
      y: Number(record["y"]),
      w: Number(record["w"]),
      h: Number(record["h"]),
    };
    if (![face.x, face.y, face.w, face.h].every(Number.isFinite)) continue;
    if (typeof record["id"] === "number") face.id = record["id"];
    if (typeof record["score"] === "number") face.score = record["score"];
    faces.push(face);
  }
  return faces;
}

function rotationOf(state: ControlState): 0 | 90 | 180 | 270 {
  const value = Number(state["stream_rotation"]?.actual);
  return value === 90 || value === 180 || value === 270 ? value : 0;
}

function deviceLabel(session: SessionInfo): string {
  const device = session.device;
  if (!device) return session.serial ?? "device";
  const name = `${device.manufacturer} ${device.model}`.trim();
  return name.length > 0 ? name : device.serial;
}

function formatBitrate(kbps: number): string {
  if (!Number.isFinite(kbps) || kbps <= 0) return "—";
  return kbps >= 1_000
    ? `${(kbps / 1_000).toFixed(2)} Mbit/s`
    : `${Math.round(kbps)} kbit/s`;
}

/** Live view controller: viewfinder stream, stats and browser overlays. */
export class LivePage {
  private readonly viewfinder = mustQuery<HTMLDivElement>("#viewfinder");
  private readonly media = mustQuery<HTMLDivElement>("#vf-media");
  private readonly stream = mustQuery<HTMLImageElement>("#live-stream");
  private readonly scene = mustQuery<HTMLDivElement>("#viewfinder .scene");
  private readonly grid = mustQuery<HTMLDivElement>("#grid-overlay");
  private readonly faces = mustQuery<WdFaceOverlay>("#face-overlay");
  private readonly status = mustQuery<HTMLDivElement>("#live-status");
  private readonly corner = mustQuery<HTMLDivElement>("#vf-corner");
  private readonly snapshotButton = mustQuery<HTMLButtonElement>("#snapshot");
  private readonly fullscreenButton = mustQuery<HTMLButtonElement>("#fullscreen");
  private readonly exitFullscreenButton =
    mustQuery<HTMLButtonElement>("#exit-fullscreen");
  private readonly gridToggle = mustQuery<HTMLInputElement>("#show-grid");
  private readonly facesToggle = mustQuery<HTMLInputElement>("#show-faces");
  private readonly stats = mustQuery<HTMLDListElement>("#live-stats");
  private readonly fps = mustQuery<HTMLElement>("#stat-fps");
  private readonly bitrate = mustQuery<HTMLElement>("#stat-bitrate");
  private readonly latency = mustQuery<HTMLElement>("#stat-latency");
  private readonly drops = mustQuery<HTMLElement>("#stat-drops");
  private readonly frames = mustQuery<HTMLElement>("#stat-frames");

  private api: LiveApi | null = null;
  private session: SessionInfo = { active: false, closed: false };
  private connection: ConnectionState = "idle";
  private visible = true;
  private revision = 0;
  private statsTimer: number | null = null;
  private statsBusy = false;
  private retryTimer: number | null = null;
  private noticeTimer: number | null = null;
  private streamLoaded = false;
  private facesVisible = true;
  private lastFaces: FaceBox[] = [];
  private rotation: 0 | 90 | 180 | 270 = 0;

  constructor() {
    this.stream.addEventListener("load", () => {
      this.streamLoaded = true;
      if (this.noticeTimer === null) this.updateStatus();
    });
    this.stream.addEventListener("error", () => this.onStreamError());
    this.snapshotButton.addEventListener("click", () => {
      void this.takeSnapshot();
    });
    this.fullscreenButton.addEventListener("click", () => {
      void this.toggleFullscreen();
    });
    this.exitFullscreenButton.addEventListener("click", () => {
      void this.exitFullscreen();
    });
    document.addEventListener("fullscreenchange", () => this.updateFullscreen());
    this.gridToggle.addEventListener("change", () => {
      this.grid.hidden = !this.gridToggle.checked;
    });
    this.facesToggle.addEventListener("change", () => {
      this.facesVisible = this.facesToggle.checked;
      this.renderFaces();
    });
    new ResizeObserver(() => this.applyRotation()).observe(this.viewfinder);
    this.demo();
  }

  /** Attach to a host; the stream follows its session and connection state. */
  connectHost(token: string | null, host: HostClient): void {
    this.api = new LiveApi(token);
    host.subscribeSession((session) => {
      this.session = session;
      this.syncStream();
    });
    host.subscribeConnection((connection) => {
      this.connection = connection;
      this.syncStream();
    });
    this.syncStream();
  }

  /** Offline demo: keep the synthetic viewfinder and clear host-only data. */
  demo(): void {
    this.api = null;
    this.session = { active: false, closed: false };
    this.connection = "idle";
    this.stopPolling();
    this.stopStream();
    this.setStats(null);
    this.updateStatus();
  }

  /** Pause the stream and polling while another page is shown. */
  setVisible(visible: boolean): void {
    this.visible = visible;
    this.syncStream();
  }

  /** Update overlays and the rotation hint from the merged control state. */
  renderState(state: ControlState): void {
    this.lastFaces = parseFaces(state["faces"]?.actual);
    this.renderFaces();
    const rotation = rotationOf(state);
    if (rotation !== this.rotation) {
      this.rotation = rotation;
      this.viewfinder.dataset.rotation = String(rotation);
      this.applyRotation();
    }
  }

  private isLive(): boolean {
    return this.api !== null && this.session.active && this.connection === "live";
  }

  private syncStream(): void {
    const shouldStream = this.isLive() && this.visible;
    if (!shouldStream) {
      this.stopStream();
      this.stopPolling();
      this.setStats(null);
      this.updateStatus();
      return;
    }
    if (this.stream.getAttribute("src") === null) {
      this.openStream();
    }
    this.startPolling();
    this.updateStatus();
  }

  private openStream(): void {
    if (!this.api) return;
    this.clearRetry();
    this.streamLoaded = false;
    this.stream.hidden = false;
    this.scene.hidden = true;
    this.stream.src = this.api.streamUrl(++this.revision);
  }

  private stopStream(): void {
    this.clearRetry();
    this.stream.removeAttribute("src");
    this.stream.hidden = true;
    this.streamLoaded = false;
    this.scene.hidden = false;
  }

  private onStreamError(): void {
    this.streamLoaded = false;
    if (!this.isLive() || !this.visible) {
      this.updateStatus();
      return;
    }
    this.stream.hidden = true;
    this.scene.hidden = false;
    this.updateStatus();
    if (this.retryTimer === null) {
      this.retryTimer = window.setTimeout(() => {
        this.retryTimer = null;
        if (this.isLive() && this.visible) this.openStream();
      }, STREAM_RETRY_MS);
    }
  }

  private clearRetry(): void {
    if (this.retryTimer !== null) {
      window.clearTimeout(this.retryTimer);
      this.retryTimer = null;
    }
  }

  private startPolling(): void {
    if (this.statsTimer !== null) return;
    void this.pollStats();
    this.statsTimer = window.setInterval(() => {
      if (!document.hidden) void this.pollStats();
    }, STATS_POLL_MS);
  }

  private stopPolling(): void {
    if (this.statsTimer !== null) {
      window.clearInterval(this.statsTimer);
      this.statsTimer = null;
    }
  }

  private async pollStats(): Promise<void> {
    if (!this.api || this.statsBusy) return;
    this.statsBusy = true;
    try {
      const stats = await this.api.stats();
      // The session or route can end while the request is in flight; do not
      // resurrect stale stats after the live view has been cleared.
      if (!this.isLive() || !this.visible) return;
      this.setStats(stats);
      // `<img>` load timing for multipart streams varies between browsers; a
      // frame count from the hub proves the open stream is delivering.
      if (
        !this.streamLoaded &&
        !this.stream.hidden &&
        this.stream.getAttribute("src") !== null &&
        stats.frames > 0
      ) {
        this.streamLoaded = true;
        if (this.noticeTimer === null) this.updateStatus();
      }
    } catch {
      // The connection banner already explains host/session failures.
    } finally {
      this.statsBusy = false;
    }
  }

  private setStats(stats: StreamStats | null): void {
    if (!stats) {
      this.stats.dataset.state = "idle";
      for (const element of [
        this.fps,
        this.bitrate,
        this.latency,
        this.drops,
        this.frames,
      ]) {
        element.textContent = "—";
      }
      return;
    }
    this.stats.dataset.state = stats.closed ? "closed" : "live";
    this.fps.textContent = Number.isFinite(stats.fps) ? stats.fps.toFixed(1) : "—";
    this.bitrate.textContent = formatBitrate(stats.bitrate_kbps);
    this.latency.textContent =
      typeof stats.latency_ms === "number"
        ? `${Math.round(stats.latency_ms)} ms`
        : "—";
    this.drops.textContent = String(stats.dropped ?? 0);
    this.frames.textContent = String(stats.frames ?? 0);
  }

  private renderFaces(): void {
    this.faces.faces = this.facesVisible ? this.lastFaces : [];
  }

  private applyRotation(): void {
    const width = this.viewfinder.clientWidth;
    const height = this.viewfinder.clientHeight;
    const scale =
      this.rotation % 180 === 0 || width === 0 || height === 0
        ? 1
        : Math.min(width / height, height / width);
    this.media.style.setProperty("--vf-rotation", `${this.rotation}deg`);
    this.media.style.setProperty("--vf-scale", String(scale));
  }

  private updateStatus(): void {
    if (!this.api) {
      this.showStatus("Demo viewfinder — connect a host to stream a device.");
      this.viewfinder.dataset.state = "demo";
      this.corner.textContent = "DEMO · 1280×720";
      this.snapshotButton.disabled = true;
      return;
    }
    this.snapshotButton.disabled = !this.isLive();
    if (!this.session.active) {
      this.showStatus(
        this.session.closed
          ? "The device session closed — open Devices to activate it again."
          : "No device session — open Devices to activate a device.",
      );
      this.viewfinder.dataset.state = "offline";
      this.corner.textContent = "NO SESSION";
      return;
    }
    if (this.connection !== "live") {
      this.showStatus(
        this.connection === "reconnecting"
          ? "Connection lost — reconnecting…"
          : "Connecting to the host…",
      );
      this.viewfinder.dataset.state = "connecting";
      this.corner.textContent = "CONNECTING";
      return;
    }
    if (!this.streamLoaded) {
      this.showStatus("Waiting for the first frame…");
      this.viewfinder.dataset.state = "waiting";
      this.corner.textContent = "WAITING FOR FRAMES";
      return;
    }
    this.status.hidden = true;
    this.status.textContent = "";
    this.viewfinder.dataset.state = "live";
    this.corner.textContent = `LIVE · ${deviceLabel(this.session)}`;
  }

  private showStatus(message: string): void {
    if (this.noticeTimer !== null) {
      window.clearTimeout(this.noticeTimer);
      this.noticeTimer = null;
    }
    this.status.textContent = message;
    this.status.hidden = false;
  }

  private showNotice(message: string): void {
    this.showStatus(message);
    if (this.noticeTimer !== null) window.clearTimeout(this.noticeTimer);
    this.noticeTimer = window.setTimeout(() => {
      this.noticeTimer = null;
      this.updateStatus();
    }, NOTICE_MS);
  }

  private async takeSnapshot(): Promise<void> {
    if (!this.api || !this.isLive()) return;
    this.snapshotButton.disabled = true;
    try {
      const blob = await this.api.snapshot();
      const url = URL.createObjectURL(blob);
      const link = document.createElement("a");
      link.href = url;
      link.download = "watchdog-snapshot.jpg";
      document.body.append(link);
      link.click();
      link.remove();
      window.setTimeout(() => URL.revokeObjectURL(url), 10_000);
      this.showNotice("Snapshot saved.");
    } catch (error) {
      this.showNotice(
        error instanceof DeviceApiError
          ? error.message
          : `snapshot failed: ${messageOf(error)}`,
      );
    } finally {
      this.snapshotButton.disabled = !this.isLive();
    }
  }

  private async toggleFullscreen(): Promise<void> {
    try {
      if (document.fullscreenElement === this.viewfinder) {
        await document.exitFullscreen();
      } else {
        await this.viewfinder.requestFullscreen();
      }
    } catch (error) {
      this.showNotice(`fullscreen unavailable: ${messageOf(error)}`);
    }
    this.updateFullscreen();
  }

  private async exitFullscreen(): Promise<void> {
    if (document.fullscreenElement !== this.viewfinder) return;
    try {
      await document.exitFullscreen();
    } catch (error) {
      this.showNotice(`fullscreen unavailable: ${messageOf(error)}`);
    }
    this.updateFullscreen();
  }

  private updateFullscreen(): void {
    const active = document.fullscreenElement === this.viewfinder;
    this.fullscreenButton.setAttribute("aria-pressed", String(active));
    this.fullscreenButton.textContent = active ? "Exit fullscreen" : "Fullscreen";
    this.exitFullscreenButton.hidden = !active;
    this.applyRotation();
  }
}
