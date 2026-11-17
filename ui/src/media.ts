// Media library page (M6-T07).
//
// The page is backend-agnostic: the offline `FakeDevice` keeps an in-memory
// library (seeded items plus anything captured in the demo) and `HostClient`
// talks to `/api/media*` and `/api/analyses`. The media store itself lands with
// M5/M7, so the host shows a clear notice until those routes exist.

import { DeviceApiError } from "./device-api";
import type { DeviceBackend, MediaItem } from "./types";

type ViewMode = "grid" | "list";
type StatusState = "busy" | "ok" | "error";

function mustQuery<T extends Element>(selector: string): T {
  const element = document.querySelector<T>(selector);
  if (!element) throw new Error(`missing element: ${selector}`);
  return element;
}

function messageOf(error: unknown): string {
  if (error instanceof DeviceApiError) {
    if (error.code === "NOT_IMPLEMENTED") {
      return "the host has no analysis queue yet";
    }
    if (error.code === "NOT_FOUND" && error.message === "request failed") {
      return "the host has no media route yet";
    }
    if (error.hint) return `${error.message} (${error.hint})`;
    return error.message;
  }
  return error instanceof Error ? error.message : String(error);
}

function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const units = ["B", "KiB", "MiB", "GiB", "TiB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value >= 10 || unit === 0 ? Math.round(value) : value.toFixed(1)} ${units[unit]}`;
}

function formatDuration(ms: number): string {
  if (!Number.isFinite(ms) || ms <= 0) return "00:00";
  const total = Math.round(ms / 1000);
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const seconds = total % 60;
  const mmss = `${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}`;
  return hours > 0 ? `${hours}:${mmss}` : mmss;
}

function formatDate(ms: number): string {
  if (!Number.isFinite(ms) || ms <= 0) return "unknown";
  return new Date(ms).toLocaleString();
}

function fileExtension(item: MediaItem): string {
  if (item.type === "video") {
    return item.mime.includes("webm") ? "webm" : "mp4";
  }
  if (item.mime.includes("png")) return "png";
  if (item.mime.includes("svg")) return "svg";
  return "jpg";
}

function deviceKey(item: MediaItem): string {
  return item.device.serial ?? item.device.model;
}

function itemLabel(item: MediaItem): string {
  return `${item.type === "video" ? "video" : "still"} ${item.id}`;
}

/** Simple generated poster for video cards and the video element. */
function posterDataUrl(item: MediaItem): string {
  const duration = formatDuration(item.duration_ms ?? 0);
  const dimensions = `${item.width}×${item.height}`;
  const svg =
    `<svg xmlns="http://www.w3.org/2000/svg" width="640" height="360" viewBox="0 0 640 360">` +
    `<defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1">` +
    `<stop offset="0" stop-color="#0b1a26"/><stop offset="1" stop-color="#123a34"/>` +
    `</linearGradient></defs>` +
    `<rect width="640" height="360" fill="url(#g)"/>` +
    `<rect x="48" y="56" width="544" height="248" fill="#16242e" stroke="#3b5566" stroke-width="3"/>` +
    `<circle cx="320" cy="166" r="46" fill="#4cc2ff" fill-opacity="0.16"/>` +
    `<path d="M306 140 L350 166 L306 192 Z" fill="#4cc2ff"/>` +
    `<text x="320" y="252" fill="#e8edf5" font-family="system-ui,sans-serif" font-size="24" font-weight="600" text-anchor="middle">VIDEO · ${duration}</text>` +
    `<text x="320" y="282" fill="#9aa7b8" font-family="ui-monospace,monospace" font-size="16" text-anchor="middle">${dimensions}</text>` +
    `</svg>`;
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
}

/** Host/demo-agnostic media library controller. */
export class MediaPage {
  private readonly mode = mustQuery<HTMLElement>("#media-mode");
  private readonly refreshButton =
    mustQuery<HTMLButtonElement>("#media-refresh");
  private readonly status = mustQuery<HTMLElement>("#media-status");
  private readonly notice = mustQuery<HTMLElement>("#media-notice");
  private readonly search = mustQuery<HTMLInputElement>("#media-search");
  private readonly typeFilter =
    mustQuery<HTMLSelectElement>("#media-filter-type");
  private readonly deviceFilter =
    mustQuery<HTMLSelectElement>("#media-filter-device");
  private readonly sort = mustQuery<HTMLSelectElement>("#media-sort");
  private readonly gridButton =
    mustQuery<HTMLButtonElement>("#media-view-grid");
  private readonly listButton =
    mustQuery<HTMLButtonElement>("#media-view-list");
  private readonly selection = mustQuery<HTMLElement>("#media-selection");
  private readonly selectedCount =
    mustQuery<HTMLElement>("#media-selected-count");
  private readonly selectAllButton =
    mustQuery<HTMLButtonElement>("#media-select-all");
  private readonly clearButton =
    mustQuery<HTMLButtonElement>("#media-select-clear");
  private readonly downloadSelected =
    mustQuery<HTMLButtonElement>("#media-download-selected");
  private readonly analyzeSelected =
    mustQuery<HTMLButtonElement>("#media-analyze-selected");
  private readonly deleteSelected =
    mustQuery<HTMLButtonElement>("#media-delete-selected");
  private readonly empty = mustQuery<HTMLElement>("#media-empty");
  private readonly grid = mustQuery<HTMLUListElement>("#media-grid");
  private readonly count = mustQuery<HTMLElement>("#media-count");
  private readonly dialog = mustQuery<HTMLDialogElement>("#media-dialog");
  private readonly dialogTitle =
    mustQuery<HTMLElement>("#media-dialog-title");
  private readonly closeButton =
    mustQuery<HTMLButtonElement>("#media-close");
  private readonly imageWrap =
    mustQuery<HTMLElement>("#media-image-wrap");
  private readonly image = mustQuery<HTMLImageElement>("#media-image");
  private readonly video = mustQuery<HTMLVideoElement>("#media-video");
  private readonly zoomOut =
    mustQuery<HTMLButtonElement>("#media-zoom-out");
  private readonly zoomIn = mustQuery<HTMLButtonElement>("#media-zoom-in");
  private readonly zoomReset =
    mustQuery<HTMLButtonElement>("#media-zoom-reset");
  private readonly zoomLevel =
    mustQuery<HTMLElement>("#media-zoom-level");
  private readonly metadata = mustQuery<HTMLElement>("#media-metadata");
  private readonly snapshotJson =
    mustQuery<HTMLElement>("#media-snapshot-json");
  private readonly downloadLink =
    mustQuery<HTMLAnchorElement>("#media-download");
  private readonly analyzeButton =
    mustQuery<HTMLButtonElement>("#media-analyze");
  private readonly deleteButton =
    mustQuery<HTMLButtonElement>("#media-delete");

  private backend: DeviceBackend | null = null;
  private items: MediaItem[] = [];
  private selected = new Set<string>();
  private current: MediaItem | null = null;
  private zoom = 1;
  private view: ViewMode = "grid";
  private visible = false;
  private requestSeq = 0;

  constructor() {
    this.refreshButton.addEventListener("click", () => {
      void this.refresh();
    });
    this.search.addEventListener("input", () => this.render());
    this.typeFilter.addEventListener("change", () => this.render());
    this.deviceFilter.addEventListener("change", () => this.render());
    this.sort.addEventListener("change", () => this.render());
    this.gridButton.addEventListener("click", () => this.setView("grid"));
    this.listButton.addEventListener("click", () => this.setView("list"));
    this.grid.addEventListener("change", (event) => {
      const input = (event.target as HTMLElement).closest<HTMLInputElement>(
        "input[data-media-select]",
      );
      if (!input) return;
      const id = input.dataset["mediaSelect"];
      if (!id) return;
      if (input.checked) {
        this.selected.add(id);
      } else {
        this.selected.delete(id);
      }
      this.updateSelection();
    });
    this.grid.addEventListener("click", (event) => {
      const button = (event.target as HTMLElement).closest<HTMLButtonElement>(
        "button[data-media-open]",
      );
      if (!button) return;
      const id = button.dataset["mediaOpen"];
      if (id) this.openPreview(id);
    });
    this.selectAllButton.addEventListener("click", () => {
      for (const item of this.filtered()) this.selected.add(item.id);
      this.render();
    });
    this.clearButton.addEventListener("click", () => {
      this.selected.clear();
      this.render();
    });
    this.downloadSelected.addEventListener("click", () => {
      for (const item of this.selectedItems()) this.download(item);
    });
    this.analyzeSelected.addEventListener("click", () => {
      void this.analyze([...this.selected]);
    });
    this.deleteSelected.addEventListener("click", () => {
      void this.remove([...this.selected]);
    });
    this.closeButton.addEventListener("click", () => this.closePreview());
    this.dialog.addEventListener("close", () => this.resetPreview());
    this.zoomOut.addEventListener("click", () => this.setZoom(this.zoom - 0.25));
    this.zoomIn.addEventListener("click", () => this.setZoom(this.zoom + 0.25));
    this.zoomReset.addEventListener("click", () => this.setZoom(1));
    this.imageWrap.addEventListener(
      "wheel",
      (event) => {
        if (!event.ctrlKey || this.image.hidden) return;
        event.preventDefault();
        this.setZoom(this.zoom + (event.deltaY < 0 ? 0.25 : -0.25));
      },
      { passive: false },
    );
    this.dialog.addEventListener("keydown", (event) => {
      if (this.image.hidden) return;
      if (event.key === "+" || event.key === "=") {
        event.preventDefault();
        this.setZoom(this.zoom + 0.25);
      } else if (event.key === "-") {
        event.preventDefault();
        this.setZoom(this.zoom - 0.25);
      } else if (event.key === "0") {
        event.preventDefault();
        this.setZoom(1);
      }
    });
    this.analyzeButton.addEventListener("click", () => {
      if (this.current) void this.analyze([this.current.id]);
    });
    this.deleteButton.addEventListener("click", () => {
      if (this.current) void this.remove([this.current.id]);
    });
    this.setView("grid");
    this.updateSelection();
  }

  /** Switch between the demo library and the host media API. */
  setBackend(backend: DeviceBackend): void {
    if (this.backend === backend) return;
    this.backend = backend;
    this.items = [];
    this.selected.clear();
    this.current = null;
    this.mode.textContent =
      backend.mode === "host" ? "host library" : "demo library";
    this.mode.dataset["state"] = backend.mode === "host" ? "host" : "demo";
    this.notice.hidden = true;
    this.notice.textContent = "";
    this.setStatus("", "ok");
    this.render();
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
    this.grid.setAttribute("aria-busy", "true");
    try {
      const items = await backend.listMedia();
      if (seq !== this.requestSeq) return;
      this.items = items;
      this.notice.hidden = true;
      this.notice.textContent = "";
      this.pruneSelection();
      this.updateDeviceOptions();
      this.render();
      if (items.length === 0) {
        this.setStatus(
          backend.mode === "demo"
            ? "The demo library is empty. Capture a still or recording to add one."
            : "The media library is empty.",
          "ok",
        );
      }
    } catch (error) {
      if (seq !== this.requestSeq) return;
      this.items = [];
      this.selected.clear();
      this.render();
      this.notice.hidden = false;
      this.notice.textContent =
        backend.mode === "host"
          ? `The host media library is not available yet (M5). ${messageOf(error)}`
          : `Could not load the demo library: ${messageOf(error)}`;
    } finally {
      if (seq === this.requestSeq) {
        this.grid.setAttribute("aria-busy", "false");
      }
    }
  }

  private filtered(): MediaItem[] {
    const term = this.search.value.trim().toLowerCase();
    const type = this.typeFilter.value;
    const device = this.deviceFilter.value;
    const items = this.items.filter((item) => {
      if (type !== "all" && item.type !== type) return false;
      if (device !== "all" && deviceKey(item) !== device) return false;
      if (!term) return true;
      const haystack = [
        item.id,
        item.type,
        item.mime,
        item.device.model,
        item.device.serial ?? "",
      ]
        .join(" ")
        .toLowerCase();
      return haystack.includes(term);
    });
    const sort = this.sort.value;
    items.sort((a, b) => {
      if (sort === "oldest") return a.created_ms - b.created_ms;
      if (sort === "largest") return b.bytes - a.bytes;
      return b.created_ms - a.created_ms;
    });
    return items;
  }

  private render(): void {
    const items = this.filtered();
    this.grid.replaceChildren();
    const fragment = document.createDocumentFragment();
    for (const item of items) fragment.append(this.renderCard(item));
    this.grid.append(fragment);
    this.empty.hidden = items.length > 0;
    this.empty.textContent =
      this.items.length === 0
        ? "No media yet. Captures appear here after they transfer."
        : "No media matches the current filters.";
    this.count.textContent = `${items.length} of ${this.items.length} item${
      this.items.length === 1 ? "" : "s"
    }`;
    this.grid.dataset["view"] = this.view;
    this.gridButton.setAttribute(
      "aria-pressed",
      String(this.view === "grid"),
    );
    this.listButton.setAttribute(
      "aria-pressed",
      String(this.view === "list"),
    );
    this.updateSelection();
    if (this.current && !this.items.some((item) => item.id === this.current?.id)) {
      this.closePreview();
    }
  }

  private renderCard(item: MediaItem): HTMLLIElement {
    const card = document.createElement("li");
    card.className = "media-card";
    card.dataset["mediaId"] = item.id;
    card.dataset["type"] = item.type;
    card.setAttribute("data-testid", "media-card");

    const checkLabel = document.createElement("label");
    checkLabel.className = "media-check";
    const checkbox = document.createElement("input");
    checkbox.type = "checkbox";
    checkbox.checked = this.selected.has(item.id);
    checkbox.dataset["mediaSelect"] = item.id;
    checkbox.setAttribute("aria-label", `Select ${itemLabel(item)}`);
    checkbox.setAttribute("data-testid", "media-select");
    checkLabel.append(checkbox);

    const open = document.createElement("button");
    open.type = "button";
    open.className = "media-open";
    open.dataset["mediaOpen"] = item.id;
    open.setAttribute("aria-label", `Preview ${itemLabel(item)}`);
    open.setAttribute("data-testid", "media-open");

    const thumb = document.createElement("span");
    thumb.className = "media-thumb";
    const image = document.createElement("img");
    image.alt = "";
    image.loading = "lazy";
    if (item.type === "still") {
      image.src = this.backend?.mediaFileUrl(item.id) ?? "";
      image.addEventListener("error", () => {
        image.src = posterDataUrl(item);
      });
    } else {
      image.src = posterDataUrl(item);
    }
    thumb.append(image);

    const kind = document.createElement("span");
    kind.className = "badge media-kind";
    kind.dataset["state"] = item.type;
    kind.textContent = item.type === "video" ? "Video" : "Still";
    thumb.append(kind);

    if (item.type === "video") {
      const duration = document.createElement("span");
      duration.className = "media-duration";
      duration.textContent = formatDuration(item.duration_ms ?? 0);
      thumb.append(duration);
    }

    const info = document.createElement("span");
    info.className = "media-info";
    const name = document.createElement("code");
    name.className = "media-name";
    name.textContent = item.id;
    const meta = document.createElement("span");
    meta.className = "media-meta";
    meta.textContent = `${formatDate(item.created_ms)} · ${item.width}×${item.height} · ${formatBytes(item.bytes)}`;
    const device = document.createElement("span");
    device.className = "media-device";
    device.textContent = item.device.serial
      ? `${item.device.model} · ${item.device.serial}`
      : item.device.model;
    info.append(name, meta, device);

    open.append(thumb, info);
    card.append(checkLabel, open);
    return card;
  }

  private updateDeviceOptions(): void {
    const previous = this.deviceFilter.value;
    const devices = new Map<string, string>();
    for (const item of this.items) {
      devices.set(deviceKey(item), item.device.model);
    }
    this.deviceFilter.replaceChildren();
    const all = document.createElement("option");
    all.value = "all";
    all.textContent = "All devices";
    this.deviceFilter.append(all);
    for (const [key, label] of devices) {
      const option = document.createElement("option");
      option.value = key;
      option.textContent = label;
      this.deviceFilter.append(option);
    }
    this.deviceFilter.value = devices.has(previous) ? previous : "all";
  }

  private updateSelection(): void {
    const count = this.selected.size;
    this.selection.hidden = count === 0;
    this.selectedCount.textContent = `${count} selected`;
  }

  private selectedItems(): MediaItem[] {
    return this.items.filter((item) => this.selected.has(item.id));
  }

  private pruneSelection(): void {
    const ids = new Set(this.items.map((item) => item.id));
    for (const id of [...this.selected]) {
      if (!ids.has(id)) this.selected.delete(id);
    }
  }

  private setView(view: ViewMode): void {
    this.view = view;
    this.render();
  }

  private setZoom(value: number): void {
    this.zoom = Math.min(4, Math.max(1, Math.round(value * 4) / 4));
    this.image.style.transform = `scale(${this.zoom})`;
    this.zoomLevel.textContent = `${Math.round(this.zoom * 100)}%`;
  }

  private openPreview(id: string): void {
    const item = this.items.find((entry) => entry.id === id);
    if (!item) return;
    this.current = item;
    this.renderPreview(item);
    if (!this.dialog.open) this.dialog.showModal();
    void this.loadDetail(id);
  }

  private async loadDetail(id: string): Promise<void> {
    const backend = this.backend;
    if (!backend) return;
    try {
      const fresh = await backend.getMedia(id);
      if (!fresh || this.current?.id !== id) return;
      this.current = fresh;
      const index = this.items.findIndex((item) => item.id === id);
      if (index >= 0) this.items[index] = fresh;
      this.renderPreview(fresh);
      this.render();
    } catch {
      // The list item already carries the metadata; a detail route is optional.
    }
  }

  private renderPreview(item: MediaItem): void {
    this.dialogTitle.textContent = itemLabel(item);
    this.setZoom(1);
    const fileUrl = this.backend?.mediaFileUrl(item.id) ?? "";
    if (item.type === "still") {
      this.video.pause();
      this.video.removeAttribute("src");
      this.video.removeAttribute("poster");
      this.video.hidden = true;
      this.image.hidden = false;
      this.image.src = fileUrl;
      this.image.alt = `Still ${item.id}`;
    } else {
      this.image.hidden = true;
      this.image.removeAttribute("src");
      this.video.hidden = false;
      this.video.poster = posterDataUrl(item);
      this.video.src = fileUrl;
    }
    this.metadata.replaceChildren();
    const rows: Array<[string, string]> = [
      ["Media id", item.id],
      ["Type", item.type === "video" ? "Video" : "Still"],
      ["Captured", formatDate(item.created_ms)],
      ["Dimensions", `${item.width}×${item.height}`],
      ["Size", formatBytes(item.bytes)],
      ["MIME type", item.mime],
      ["SHA-256", item.sha256 || "not reported"],
      [
        "Device",
        item.device.serial
          ? `${item.device.model} · ${item.device.serial}`
          : item.device.model,
      ],
    ];
    if (item.type === "video") {
      rows.splice(4, 0, ["Duration", formatDuration(item.duration_ms ?? 0)]);
    }
    rows.push(["Host job", item.host_job_id ?? "none"]);
    for (const [label, value] of rows) {
      const wrapper = document.createElement("div");
      const term = document.createElement("dt");
      term.textContent = label;
      const detail = document.createElement("dd");
      detail.textContent = value;
      if (label === "SHA-256") detail.title = item.sha256;
      wrapper.append(term, detail);
      this.metadata.append(wrapper);
    }
    this.snapshotJson.textContent = item.control_snapshot
      ? JSON.stringify(item.control_snapshot, null, 2)
      : "No control snapshot was recorded for this item.";
    this.downloadLink.href = fileUrl;
    this.downloadLink.download = `${item.id}.${fileExtension(item)}`;
    this.analyzeButton.dataset["mediaId"] = item.id;
    this.deleteButton.dataset["mediaId"] = item.id;
  }

  private closePreview(): void {
    if (this.dialog.open) this.dialog.close();
    this.resetPreview();
  }

  private resetPreview(): void {
    this.current = null;
    this.video.pause();
    this.video.removeAttribute("src");
    this.video.removeAttribute("poster");
    this.image.removeAttribute("src");
    this.image.style.transform = "scale(1)";
    this.zoom = 1;
    this.zoomLevel.textContent = "100%";
  }

  private download(item: MediaItem): void {
    const backend = this.backend;
    if (!backend) return;
    const anchor = document.createElement("a");
    anchor.href = backend.mediaFileUrl(item.id);
    anchor.download = `${item.id}.${fileExtension(item)}`;
    document.body.append(anchor);
    anchor.click();
    anchor.remove();
  }

  private async analyze(ids: string[]): Promise<void> {
    const backend = this.backend;
    if (!backend || ids.length === 0) return;
    this.setStatus(
      `Submitting ${ids.length} item${ids.length === 1 ? "" : "s"} for analysis…`,
      "busy",
    );
    try {
      const result = await backend.analyzeMedia(ids);
      this.setStatus(
        `Analysis ${result.state}: ${result.id} (${ids.length} item${
          ids.length === 1 ? "" : "s"
        })`,
        "ok",
      );
    } catch (error) {
      this.setStatus(`Analysis failed: ${messageOf(error)}`, "error");
    }
  }

  private async remove(ids: string[]): Promise<void> {
    const backend = this.backend;
    if (!backend || ids.length === 0) return;
    let removed = 0;
    let failure: string | null = null;
    for (const id of ids) {
      try {
        await backend.deleteMedia(id);
        removed += 1;
        this.selected.delete(id);
        if (this.current?.id === id) this.closePreview();
      } catch (error) {
        failure ??= messageOf(error);
      }
    }
    await this.refresh();
    if (failure) {
      this.setStatus(
        `Deleted ${removed} item${removed === 1 ? "" : "s"}; some failed: ${failure}`,
        "error",
      );
    } else {
      this.setStatus(
        `Deleted ${removed} item${removed === 1 ? "" : "s"}.`,
        "ok",
      );
    }
  }

  private setStatus(text: string, state: StatusState): void {
    this.status.textContent = text;
    this.status.dataset["state"] = state;
    this.status.hidden = text.length === 0;
  }
}
