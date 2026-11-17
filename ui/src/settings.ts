// Settings page (M6-T08).
//
// The page is backend-agnostic: the offline `FakeDevice` reports demo values,
// seeded logs and a generated diagnostics file, while `HostClient` reads
// `/api/settings`, `/api/logs` and `/api/diagnostics`. The host configuration,
// ffmpeg bundle and system API land with M1/M7/M9, so host mode shows a clear
// notice until those routes exist.

import { DeviceApiError } from "./device-api";
import type {
  DeviceBackend,
  HostLogLine,
  HostRetention,
  HostSettings,
} from "./types";

const UI_VERSION = "0.1.0";

function mustQuery<T extends Element>(selector: string): T {
  const element = document.querySelector<T>(selector);
  if (!element) throw new Error(`missing element: ${selector}`);
  return element;
}

function messageOf(error: unknown): string {
  if (error instanceof DeviceApiError) {
    if (error.code === "NOT_FOUND" && error.message === "request failed") {
      return "the host has no settings route yet";
    }
    return error.message;
  }
  return error instanceof Error ? error.message : String(error);
}

/** Human-readable byte count with one decimal for larger units. */
function formatBytes(bytes: number | undefined): string {
  if (bytes === undefined || !Number.isFinite(bytes)) return "unknown";
  if (bytes < 1024) return `${Math.round(bytes)} B`;
  const units = ["KiB", "MiB", "GiB", "TiB"];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  const digits = value >= 100 ? 0 : 1;
  return `${value.toFixed(digits)} ${units[unit]}`;
}

/** Clock time for one log line. */
function formatClock(tsMs: number): string {
  const date = new Date(tsMs);
  if (Number.isNaN(date.getTime())) return "--:--:--";
  return date.toLocaleTimeString(undefined, {
    hour12: false,
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

function integerFrom(input: HTMLInputElement, fallback: number): number {
  const value = Number(input.value);
  return Number.isFinite(value) && value >= 0 ? Math.round(value) : fallback;
}

/** Host/demo-agnostic settings controller. */
export class SettingsPage {
  private readonly mode = mustQuery<HTMLElement>("#settings-mode");
  private readonly refreshButton =
    mustQuery<HTMLButtonElement>("#settings-refresh");
  private readonly status = mustQuery<HTMLElement>("#settings-status");
  private readonly notice = mustQuery<HTMLElement>("#settings-notice");

  private readonly uiPort = mustQuery<HTMLElement>("#settings-ui-port");
  private readonly mcpPort = mustQuery<HTMLElement>("#settings-mcp-port");
  private readonly apiPort = mustQuery<HTMLElement>("#settings-api-port");
  private readonly listenMode = mustQuery<HTMLElement>("#settings-listen-mode");
  private readonly uiListen = mustQuery<HTMLElement>("#settings-ui-listen");
  private readonly mcpListen = mustQuery<HTMLElement>("#settings-mcp-listen");
  private readonly apiListen = mustQuery<HTMLElement>("#settings-api-listen");
  private readonly baseUrl = mustQuery<HTMLElement>("#settings-base-url");
  private readonly copyUrl =
    mustQuery<HTMLButtonElement>("#settings-copy-url");
  private readonly tokenState = mustQuery<HTMLElement>("#settings-token-state");
  private readonly tokenHint = mustQuery<HTMLElement>("#settings-token-hint");
  private readonly tokenWarning =
    mustQuery<HTMLElement>("#settings-token-warning");

  private readonly dataDir = mustQuery<HTMLElement>("#settings-data-dir");
  private readonly mediaDir = mustQuery<HTMLElement>("#settings-media-dir");
  private readonly logsDir = mustQuery<HTMLElement>("#settings-logs-dir");
  private readonly mediaItems = mustQuery<HTMLElement>("#settings-media-items");
  private readonly mediaSize = mustQuery<HTMLElement>("#settings-media-size");
  private readonly diskFree = mustQuery<HTMLElement>("#settings-disk-free");
  private readonly diskTotal = mustQuery<HTMLElement>("#settings-disk-total");
  private readonly diskMeter = mustQuery<HTMLElement>("#settings-disk-meter");
  private readonly diskBar = mustQuery<HTMLElement>("#settings-disk-bar");

  private readonly retentionForm =
    mustQuery<HTMLFormElement>("#settings-retention-form");
  private readonly retentionAge =
    mustQuery<HTMLInputElement>("#settings-retention-age");
  private readonly retentionSize =
    mustQuery<HTMLInputElement>("#settings-retention-size");
  private readonly retentionSave =
    mustQuery<HTMLButtonElement>("#settings-retention-save");

  private readonly ffmpegState = mustQuery<HTMLElement>("#settings-ffmpeg-state");
  private readonly ffmpegVersion =
    mustQuery<HTMLElement>("#settings-ffmpeg-version");
  private readonly ffprobeVersion =
    mustQuery<HTMLElement>("#settings-ffprobe-version");
  private readonly ffmpegPath = mustQuery<HTMLElement>("#settings-ffmpeg-path");
  private readonly ffmpegLicense =
    mustQuery<HTMLElement>("#settings-ffmpeg-license");
  private readonly ffmpegFilters =
    mustQuery<HTMLElement>("#settings-ffmpeg-filters");

  private readonly logLevel = mustQuery<HTMLSelectElement>("#settings-log-level");
  private readonly logRefresh =
    mustQuery<HTMLButtonElement>("#settings-log-refresh");
  private readonly logCount = mustQuery<HTMLElement>("#settings-log-count");
  private readonly logList = mustQuery<HTMLOListElement>("#settings-log-list");
  private readonly logEmpty = mustQuery<HTMLElement>("#settings-log-empty");

  private readonly version = mustQuery<HTMLElement>("#settings-version");
  private readonly commit = mustQuery<HTMLElement>("#settings-commit");
  private readonly build = mustQuery<HTMLElement>("#settings-build");
  private readonly platform = mustQuery<HTMLElement>("#settings-platform");
  private readonly protocol = mustQuery<HTMLElement>("#settings-protocol");
  private readonly uiVersion = mustQuery<HTMLElement>("#settings-ui-version");
  private readonly diagnostics =
    mustQuery<HTMLAnchorElement>("#settings-diagnostics");

  private backend: DeviceBackend | null = null;
  private current: HostSettings | null = null;
  private visible = false;
  private requestSeq = 0;

  constructor() {
    this.refreshButton.addEventListener("click", () => {
      void this.refresh();
    });
    this.copyUrl.addEventListener("click", () => {
      void this.copyBaseUrl();
    });
    this.retentionForm.addEventListener("submit", (event) => {
      event.preventDefault();
      void this.saveRetention();
    });
    this.logLevel.addEventListener("change", () => {
      void this.loadLogs();
    });
    this.logRefresh.addEventListener("click", () => {
      void this.loadLogs();
    });
    this.uiVersion.textContent = UI_VERSION;
    this.logList.setAttribute("aria-busy", "false");
  }

  /** Switch between the demo settings and the host settings API. */
  setBackend(backend: DeviceBackend): void {
    if (this.backend === backend) return;
    this.backend = backend;
    this.current = null;
    this.mode.textContent =
      backend.mode === "host" ? "host settings" : "demo settings";
    this.mode.dataset["state"] = backend.mode === "host" ? "host" : "demo";
    this.notice.hidden = true;
    this.notice.textContent = "";
    this.setStatus("", "ok");
    this.setDiagnostics();
    if (this.visible) void this.refresh();
  }

  setVisible(visible: boolean): void {
    this.visible = visible;
    if (visible) void this.refresh();
  }

  async refresh(): Promise<void> {
    const backend = this.backend;
    if (!backend) return;
    const seq = ++this.requestSeq;
    this.setStatus("Loading settings…", "busy");
    try {
      const settings = await backend.getSettings();
      if (seq !== this.requestSeq) return;
      this.current = settings;
      this.renderSettings(settings);
      this.notice.hidden = true;
      this.notice.textContent = "";
      this.setDiagnostics();
      this.setStatus(
        backend.mode === "host"
          ? "Loaded settings from the host."
          : "Loaded demo settings.",
        "ok",
      );
      await this.loadLogs(seq);
    } catch (error) {
      if (seq !== this.requestSeq) return;
      this.current = null;
      this.clearSettings();
      this.notice.hidden = false;
      this.notice.textContent =
        backend.mode === "host"
          ? `The host settings API is not available yet (M7/M9). ${messageOf(error)}`
          : `Could not load the demo settings: ${messageOf(error)}`;
      this.setStatus("Settings unavailable.", "error");
      this.renderLogs([]);
    }
  }

  private async loadLogs(parentSeq?: number): Promise<void> {
    const backend = this.backend;
    if (!backend) return;
    const seq = parentSeq ?? this.requestSeq;
    this.logList.setAttribute("aria-busy", "true");
    try {
      const lines = await backend.listLogs({
        level: this.logLevel.value,
        limit: 200,
      });
      if (seq !== this.requestSeq) return;
      this.renderLogs(lines);
    } catch (error) {
      if (seq !== this.requestSeq) return;
      this.renderLogs([]);
      this.logEmpty.hidden = false;
      this.logEmpty.textContent = `Could not load logs: ${messageOf(error)}`;
    } finally {
      if (seq === this.requestSeq) {
        this.logList.setAttribute("aria-busy", "false");
      }
    }
  }

  private async saveRetention(): Promise<void> {
    const backend = this.backend;
    if (!backend || !this.current) return;
    const retention: HostRetention = {
      max_age_days: integerFrom(this.retentionAge, this.current.retention.max_age_days),
      max_size_mb: integerFrom(this.retentionSize, this.current.retention.max_size_mb),
    };
    this.retentionSave.disabled = true;
    this.setStatus("Saving retention…", "busy");
    try {
      const saved = await backend.saveRetention(retention);
      this.current.retention = { ...saved };
      this.retentionAge.value = String(saved.max_age_days);
      this.retentionSize.value = String(saved.max_size_mb);
      this.setStatus(
        `Retention saved: ${saved.max_age_days} days, ${saved.max_size_mb} MiB.`,
        "ok",
      );
      await this.loadLogs();
    } catch (error) {
      this.setStatus(`Could not save retention: ${messageOf(error)}`, "error");
    } finally {
      this.retentionSave.disabled = false;
    }
  }

  private renderSettings(settings: HostSettings): void {
    const { ports, listen, token, storage, retention, ffmpeg, about } = settings;
    this.uiPort.textContent = String(ports.ui);
    this.mcpPort.textContent = String(ports.mcp);
    this.apiPort.textContent = String(ports.api);

    const lan = listen.mode !== "localhost";
    this.listenMode.textContent = lan ? "LAN" : "localhost";
    this.listenMode.dataset["state"] = lan ? "warn" : "ok";
    this.uiListen.textContent = listen.addresses.ui;
    this.mcpListen.textContent = listen.addresses.mcp;
    this.apiListen.textContent = listen.addresses.api;

    const url = `http://${listen.addresses.ui}:${ports.ui}/`;
    this.baseUrl.textContent = url;
    this.baseUrl.title = url;

    this.tokenState.textContent = token.configured ? "configured" : "not configured";
    this.tokenState.dataset["state"] = token.configured ? "ok" : "warn";
    this.tokenHint.textContent = token.configured
      ? (token.hint ?? "")
      : "open access on localhost";
    this.tokenWarning.hidden = !(lan && !token.configured);

    this.dataDir.textContent = storage.data_dir;
    this.dataDir.title = storage.data_dir;
    this.mediaDir.textContent = storage.media_dir;
    this.mediaDir.title = storage.media_dir;
    this.logsDir.textContent = storage.logs_dir;
    this.logsDir.title = storage.logs_dir;
    this.mediaItems.textContent = String(storage.media_items);
    this.mediaSize.textContent = formatBytes(storage.media_bytes);
    this.diskFree.textContent = formatBytes(storage.free_bytes);
    this.diskTotal.textContent = formatBytes(storage.total_bytes);
    if (
      storage.free_bytes !== undefined &&
      storage.total_bytes !== undefined &&
      storage.total_bytes > 0
    ) {
      const used = Math.max(0, storage.total_bytes - storage.free_bytes);
      const percent = Math.min(100, (used / storage.total_bytes) * 100);
      this.diskBar.style.width = `${percent.toFixed(1)}%`;
      this.diskMeter.setAttribute(
        "aria-label",
        `Disk usage: ${percent.toFixed(0)}% used, ${formatBytes(
          storage.free_bytes,
        )} free of ${formatBytes(storage.total_bytes)}`,
      );
    } else {
      this.diskBar.style.width = "0%";
      this.diskMeter.setAttribute("aria-label", "Disk usage unknown");
    }

    this.retentionAge.value = String(retention.max_age_days);
    this.retentionSize.value = String(retention.max_size_mb);

    this.ffmpegState.textContent = ffmpeg.available ? "available" : "missing";
    this.ffmpegState.dataset["state"] = ffmpeg.available ? "ok" : "error";
    this.ffmpegVersion.textContent = ffmpeg.ffmpeg_version ?? "unknown";
    this.ffprobeVersion.textContent = ffmpeg.ffprobe_version ?? "unknown";
    this.ffmpegPath.textContent = ffmpeg.path ?? "not extracted";
    this.ffmpegPath.title = ffmpeg.path ?? "";
    this.ffmpegLicense.textContent = ffmpeg.license ?? "unknown";
    this.ffmpegFilters.textContent =
      ffmpeg.filters === undefined ? "unknown" : String(ffmpeg.filters);

    this.version.textContent = about.version;
    this.commit.textContent = about.commit ?? "unknown";
    this.build.textContent = about.build ?? "unknown";
    this.platform.textContent = `${about.os} · ${about.arch}`;
    this.protocol.textContent =
      about.protocol === undefined ? "unknown" : `v${about.protocol}`;
    this.uiVersion.textContent = UI_VERSION;
  }

  private clearSettings(): void {
    for (const element of [
      this.uiPort,
      this.mcpPort,
      this.apiPort,
      this.uiListen,
      this.mcpListen,
      this.apiListen,
      this.baseUrl,
      this.tokenHint,
      this.dataDir,
      this.mediaDir,
      this.logsDir,
      this.mediaItems,
      this.mediaSize,
      this.diskFree,
      this.diskTotal,
      this.ffmpegVersion,
      this.ffprobeVersion,
      this.ffmpegPath,
      this.ffmpegLicense,
      this.ffmpegFilters,
      this.version,
      this.commit,
      this.build,
      this.platform,
      this.protocol,
    ]) {
      element.textContent = "—";
    }
    this.listenMode.textContent = "—";
    this.listenMode.removeAttribute("data-state");
    this.tokenState.textContent = "—";
    this.tokenState.removeAttribute("data-state");
    this.tokenWarning.hidden = true;
    this.diskBar.style.width = "0%";
    this.ffmpegState.textContent = "—";
    this.ffmpegState.removeAttribute("data-state");
    this.retentionAge.value = "";
    this.retentionSize.value = "";
    this.diagnostics.href = "#";
    this.diagnostics.removeAttribute("download");
  }

  private renderLogs(lines: HostLogLine[]): void {
    this.logList.replaceChildren();
    this.logCount.textContent = `${lines.length} ${
      lines.length === 1 ? "line" : "lines"
    }`;
    this.logEmpty.hidden = lines.length > 0;
    if (lines.length === 0) {
      this.logEmpty.textContent = "No log lines at this level.";
      return;
    }
    const fragment = document.createDocumentFragment();
    for (const line of lines) {
      const item = document.createElement("li");
      item.className = "settings-log";
      item.dataset["level"] = line.level;
      item.setAttribute("data-testid", "settings-log-line");

      const time = document.createElement("time");
      time.className = "settings-log-time";
      time.dateTime = new Date(line.ts_ms).toISOString();
      time.textContent = formatClock(line.ts_ms);

      const level = document.createElement("span");
      level.className = "badge settings-log-level";
      level.dataset["state"] = line.level;
      level.textContent = line.level;

      const target = document.createElement("span");
      target.className = "settings-log-target";
      target.textContent = line.target ?? "host";

      const message = document.createElement("span");
      message.className = "settings-log-message";
      message.textContent = line.message;

      item.append(time, level, target, message);
      fragment.append(item);
    }
    this.logList.append(fragment);
  }

  private setDiagnostics(): void {
    const backend = this.backend;
    if (!backend) {
      this.diagnostics.href = "#";
      return;
    }
    this.diagnostics.href = backend.diagnosticsUrl();
    this.diagnostics.download =
      backend.mode === "host"
        ? "watchdog-diagnostics.zip"
        : "watchdog-diagnostics.json";
  }

  private async copyBaseUrl(): Promise<void> {
    const value = this.baseUrl.textContent ?? "";
    if (!value || value === "—") return;
    try {
      await navigator.clipboard.writeText(value);
      this.setStatus(`Copied ${value}`, "ok");
    } catch {
      this.setStatus("Could not copy the URL; select it manually.", "error");
    }
  }

  private setStatus(
    message: string,
    state: "busy" | "ok" | "error",
  ): void {
    this.status.textContent = message;
    this.status.dataset["state"] = state;
  }
}
