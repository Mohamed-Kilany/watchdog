// Capture page controller (M6-T06).
//
// Still, burst and recording commands go through the same backend contract as
// the control panels: the demo FakeDevice simulates the phone, while HostClient
// forwards the protocol commands (`capture_still`, `capture_burst`,
// `record_start`, `record_stop`) over `POST /api/command`. Recording lifecycle
// events and host transfer progress arrive on the real-time channel.

import type {
  ControlState,
  DeviceBackend,
  DeviceEvent,
  TransferProgress,
} from "./types";

type StatusState = "idle" | "busy" | "ok" | "error";

function mustQuery<T extends Element>(selector: string): T {
  const element = document.querySelector<T>(selector);
  if (!element) throw new Error(`missing element: ${selector}`);
  return element;
}

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** `mm:ss`, or `h:mm:ss` past an hour. */
function formatDuration(milliseconds: number): string {
  const total = Math.max(0, Math.floor(milliseconds / 1000));
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const seconds = total % 60;
  const pad = (value: number): string => String(value).padStart(2, "0");
  return hours > 0
    ? `${hours}:${pad(minutes)}:${pad(seconds)}`
    : `${pad(minutes)}:${pad(seconds)}`;
}

function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const units = ["B", "KiB", "MiB", "GiB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  const digits = value >= 10 || unit === 0 ? 0 : 1;
  return `${value.toFixed(digits)} ${units[unit] ?? "B"}`;
}

function readNumber(
  input: HTMLInputElement,
  fallback: number,
  min: number,
  max: number,
): number {
  const value = Number(input.value);
  if (!Number.isFinite(value)) return fallback;
  return Math.min(max, Math.max(min, Math.round(value)));
}

function eventData(event: DeviceEvent): Record<string, unknown> {
  return event.data && typeof event.data === "object"
    ? (event.data as Record<string, unknown>)
    : {};
}

function numberField(
  record: Record<string, unknown>,
  ...keys: string[]
): number | undefined {
  for (const key of keys) {
    const value = Number(record[key]);
    if (Number.isFinite(value)) return value;
  }
  return undefined;
}

/** Capture page: still, burst, recording and transfer progress. */
export class CapturePage {
  private readonly stillButton =
    mustQuery<HTMLButtonElement>("#capture-still");
  private readonly stillReadout =
    mustQuery<HTMLParagraphElement>("#capture-last-still");
  private readonly burstCount =
    mustQuery<HTMLInputElement>("#capture-burst-count");
  private readonly burstInterval =
    mustQuery<HTMLInputElement>("#capture-burst-interval");
  private readonly burstFromStream =
    mustQuery<HTMLInputElement>("#capture-from-stream");
  private readonly burstButton =
    mustQuery<HTMLButtonElement>("#capture-burst");
  private readonly burstBar =
    mustQuery<HTMLProgressElement>("#capture-burst-bar");
  private readonly burstProgress =
    mustQuery<HTMLSpanElement>("#capture-burst-progress");
  private readonly recordMax =
    mustQuery<HTMLInputElement>("#capture-record-max");
  private readonly recordStartButton =
    mustQuery<HTMLButtonElement>("#capture-record-start");
  private readonly recordStopButton =
    mustQuery<HTMLButtonElement>("#capture-record-stop");
  private readonly recordingBadge =
    mustQuery<HTMLSpanElement>("#recording-state");
  private readonly recordingTimer =
    mustQuery<HTMLParagraphElement>("#recording-timer");
  private readonly modeBadge = mustQuery<HTMLSpanElement>("#capture-mode");
  private readonly status = mustQuery<HTMLParagraphElement>("#capture-status");
  private readonly transferList =
    mustQuery<HTMLOListElement>("#transfer-list");
  private readonly transferEmpty =
    mustQuery<HTMLParagraphElement>("#transfer-empty");

  private backend: DeviceBackend | null = null;
  private unsubscribeEvents: (() => void) | null = null;
  private unsubscribeTransfers: (() => void) | null = null;
  private readonly transfers = new Map<string, TransferProgress>();
  private recordingActive = false;
  private recordBusy = false;
  private recordingStartedMs: number | null = null;
  private recordingTimerId: number | null = null;
  private burstTotal = 0;

  constructor() {
    this.stillButton.addEventListener("click", () => {
      void this.captureStill();
    });
    this.burstButton.addEventListener("click", () => {
      void this.captureBurst();
    });
    this.recordStartButton.addEventListener("click", () => {
      void this.startRecording();
    });
    this.recordStopButton.addEventListener("click", () => {
      void this.stopRecording();
    });
    this.burstCount.addEventListener("change", () => {
      this.burstCount.value = String(
        readNumber(this.burstCount, 5, 1, 50),
      );
    });
    this.burstInterval.addEventListener("change", () => {
      this.burstInterval.value = String(
        readNumber(this.burstInterval, 200, 0, 5000),
      );
    });
    this.renderTimer(0);
    this.renderTransfers();
    this.syncButtons();
  }

  /** Adopt the current demo/host backend and subscribe to its events. */
  setBackend(backend: DeviceBackend): void {
    this.unsubscribeEvents?.();
    this.unsubscribeTransfers?.();
    this.backend = backend;
    this.unsubscribeEvents = backend.subscribeEvents((event) => {
      this.onEvent(event);
    });
    this.unsubscribeTransfers = backend.subscribeTransfers((transfer) => {
      this.onTransfer(transfer);
    });
    this.modeBadge.textContent =
      backend.mode === "demo" ? "demo device" : "host";
    this.modeBadge.dataset.state = backend.mode === "demo" ? "demo" : "host";
    this.transfers.clear();
    this.renderTransfers();
    this.recordingActive = false;
    this.recordingStartedMs = null;
    this.stopTimer();
    this.renderTimer(0);
    this.renderState(backend.snapshot);
    this.setStatus("Ready.", "idle");
  }

  /** Capture commands keep running while another route is shown. */
  setVisible(_visible: boolean): void {
    // The timer and transfers are event-driven, so there is nothing to pause.
  }

  /** Mirror the recording flag from the merged control state. */
  renderState(state: ControlState): void {
    const recording = state.recording?.actual === true;
    if (recording && !this.recordingActive) {
      this.recordingActive = true;
      if (this.recordingStartedMs === null) {
        this.recordingStartedMs = Date.now();
      }
      this.startTimer();
    } else if (!recording && this.recordingActive) {
      this.recordingActive = false;
      this.recordingStartedMs = null;
      this.stopTimer();
      this.renderTimer(0);
    }
    this.renderRecordingBadge();
    this.syncButtons();
  }

  private async captureStill(): Promise<void> {
    const backend = this.backend;
    if (!backend) return;
    this.stillButton.disabled = true;
    this.setStatus("Capturing still…", "busy");
    try {
      const outcome = await backend.captureStill();
      const details = [
        outcome.mediaId ? `media ${outcome.mediaId}` : null,
        outcome.width && outcome.height
          ? `${outcome.width}×${outcome.height}`
          : null,
        typeof outcome.bytes === "number" ? formatBytes(outcome.bytes) : null,
      ].filter((part): part is string => part !== null);
      this.stillReadout.textContent =
        details.length > 0 ? details.join(" · ") : "Still captured.";
      this.setStatus(
        `Still captured${details.length > 0 ? `: ${details.join(" · ")}` : ""}.`,
        "ok",
      );
    } catch (error) {
      this.setStatus(`Still capture failed: ${messageOf(error)}`, "error");
    } finally {
      this.stillButton.disabled = false;
    }
  }

  private async captureBurst(): Promise<void> {
    const backend = this.backend;
    if (!backend) return;
    const count = readNumber(this.burstCount, 5, 1, 50);
    const interval = readNumber(this.burstInterval, 200, 0, 5000);
    this.burstCount.value = String(count);
    this.burstInterval.value = String(interval);
    this.burstTotal = count;
    this.burstBar.max = count;
    this.burstBar.value = 0;
    this.burstProgress.textContent = `0 / ${count}`;
    this.burstButton.disabled = true;
    this.setStatus(
      `Capturing ${count} frame${count === 1 ? "" : "s"}…`,
      "busy",
    );
    try {
      const outcome = await backend.captureBurst({
        count,
        intervalMs: interval,
        fromStream: this.burstFromStream.checked,
      });
      const total = outcome.mediaIds?.length ?? count;
      this.burstBar.max = total;
      this.burstBar.value = total;
      this.burstProgress.textContent = `${total} / ${total}`;
      this.setStatus(
        `Burst complete: ${total} frame${total === 1 ? "" : "s"}${
          outcome.sequenceId ? ` (${outcome.sequenceId})` : ""
        }.`,
        "ok",
      );
    } catch (error) {
      this.setStatus(`Burst failed: ${messageOf(error)}`, "error");
    } finally {
      this.burstButton.disabled = false;
    }
  }

  private async startRecording(): Promise<void> {
    const backend = this.backend;
    if (!backend || this.recordBusy) return;
    const maxDurationSecs = readNumber(this.recordMax, 0, 0, 3600);
    this.recordMax.value = String(maxDurationSecs);
    this.recordBusy = true;
    this.syncButtons();
    try {
      const outcome = await backend.recordStart(
        maxDurationSecs > 0 ? { maxDurationSecs } : {},
      );
      this.recordingActive = true;
      this.recordingStartedMs = outcome.startedMs ?? Date.now();
      this.startTimer();
      this.renderRecordingBadge();
      this.setStatus(
        `Recording${outcome.mediaId ? ` ${outcome.mediaId}` : ""}.`,
        "ok",
      );
    } catch (error) {
      this.setStatus(`Recording failed: ${messageOf(error)}`, "error");
    } finally {
      this.recordBusy = false;
      this.syncButtons();
    }
  }

  private async stopRecording(): Promise<void> {
    const backend = this.backend;
    if (!backend || this.recordBusy) return;
    this.recordBusy = true;
    this.syncButtons();
    try {
      const outcome = await backend.recordStop();
      this.recordingActive = false;
      this.recordingStartedMs = null;
      this.stopTimer();
      this.renderTimer(outcome.durationMs ?? 0);
      this.renderRecordingBadge();
      const duration =
        typeof outcome.durationMs === "number"
          ? ` after ${formatDuration(outcome.durationMs)}`
          : "";
      this.setStatus(`Recording stopped${duration}.`, "ok");
    } catch (error) {
      this.setStatus(`Stop failed: ${messageOf(error)}`, "error");
    } finally {
      this.recordBusy = false;
      this.syncButtons();
    }
  }

  private onEvent(event: DeviceEvent): void {
    const data = eventData(event);
    switch (event.kind) {
      case "capture_progress": {
        const done = numberField(data, "done") ?? 0;
        const total = numberField(data, "total") ?? this.burstTotal;
        if (total > 0) {
          this.burstTotal = total;
          this.burstBar.max = total;
          this.burstBar.value = Math.min(done, total);
          this.burstProgress.textContent = `${done} / ${total}`;
        }
        break;
      }
      case "recording_started": {
        this.recordingActive = true;
        this.recordingStartedMs =
          numberField(data, "started_ms", "startedMs") ??
          event.timestamp_ms ??
          Date.now();
        this.startTimer();
        this.renderRecordingBadge();
        this.syncButtons();
        break;
      }
      case "recording_stopped": {
        const duration =
          numberField(data, "duration_ms", "durationMs") ?? 0;
        this.recordingActive = false;
        this.recordingStartedMs = null;
        this.stopTimer();
        this.renderTimer(duration);
        this.renderRecordingBadge();
        this.syncButtons();
        break;
      }
      default:
        break;
    }
  }

  private onTransfer(transfer: TransferProgress): void {
    this.transfers.set(transfer.transferId, transfer);
    this.renderTransfers();
  }

  private renderTransfers(): void {
    this.transferList.replaceChildren();
    const items = Array.from(this.transfers.values());
    this.transferEmpty.hidden = items.length > 0;
    for (const transfer of items) {
      const item = document.createElement("li");
      item.className = "transfer";
      item.dataset.state = transfer.state;
      item.dataset.testid = "transfer-item";

      const head = document.createElement("div");
      head.className = "transfer-head";
      const name = document.createElement("code");
      name.textContent = transfer.mediaId;
      const badge = document.createElement("span");
      badge.className = "badge";
      badge.dataset.state = transfer.state;
      badge.textContent = transfer.state;
      head.append(name, badge);

      const progress = document.createElement("progress");
      progress.dataset.testid = "transfer-progress";
      progress.max = Math.max(1, transfer.bytesTotal);
      progress.value = Math.min(transfer.bytesDone, progress.max);
      progress.setAttribute(
        "aria-label",
        `Transfer progress for ${transfer.mediaId}`,
      );

      const detail = document.createElement("span");
      detail.className = "muted";
      detail.textContent = `${formatBytes(transfer.bytesDone)} / ${formatBytes(
        transfer.bytesTotal,
      )}${transfer.error ? ` · ${transfer.error}` : ""}`;

      item.append(head, progress, detail);
      this.transferList.append(item);
    }
  }

  private renderRecordingBadge(): void {
    this.recordingBadge.textContent = this.recordingActive
      ? "Recording"
      : "Idle";
    this.recordingBadge.dataset.state = this.recordingActive
      ? "recording"
      : "idle";
  }

  private syncButtons(): void {
    this.recordStartButton.disabled = this.recordingActive || this.recordBusy;
    this.recordStopButton.disabled = !this.recordingActive || this.recordBusy;
  }

  private startTimer(): void {
    if (this.recordingTimerId !== null) return;
    this.renderTimer();
    this.recordingTimerId = window.setInterval(() => {
      this.renderTimer();
    }, 250);
  }

  private stopTimer(): void {
    if (this.recordingTimerId !== null) {
      window.clearInterval(this.recordingTimerId);
      this.recordingTimerId = null;
    }
  }

  private renderTimer(durationMs?: number): void {
    const elapsed =
      durationMs ??
      (this.recordingStartedMs === null
        ? 0
        : Date.now() - this.recordingStartedMs);
    this.recordingTimer.textContent = formatDuration(elapsed);
  }

  private setStatus(message: string, state: StatusState): void {
    this.status.textContent = message;
    this.status.dataset.state = state;
    this.status.setAttribute("aria-busy", String(state === "busy"));
  }
}
