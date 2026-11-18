import { attachStyles, baseCss, clamp, defineOnce, emit } from "./base";
import type { ControlRegion } from "../types";

const HINT =
  "Drag to draw · Arrow keys move · Shift+Arrow resize · Enter applies · Escape cancels";

const css = `${baseCss}
  :host {
    position: absolute;
    inset: 0;
    display: block;
    pointer-events: none;
    touch-action: none;
    user-select: none;
  }
  :host([editable]) {
    pointer-events: auto;
    cursor: crosshair;
  }
  :host(:focus-visible) {
    outline: 3px solid var(--wd-focus);
    outline-offset: -3px;
  }
  .canvas {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    overflow: visible;
  }
  rect.region {
    fill: rgb(76 194 255 / 10%);
    stroke: var(--wd-accent);
    stroke-width: 2;
    vector-effect: non-scaling-stroke;
  }
  :host([mode="metering"]) rect.region {
    fill: rgb(255 184 77 / 10%);
    stroke: var(--wd-warn);
    stroke-dasharray: 6 4;
  }
  rect.draft {
    fill: rgb(255 255 255 / 8%);
    stroke: #ffffff;
    stroke-width: 2;
    stroke-dasharray: 5 4;
    vector-effect: non-scaling-stroke;
  }
  .mode-label {
    position: absolute;
    top: 0.6rem;
    left: 0.6rem;
    padding: 0.12rem 0.4rem;
    border-radius: 0.3rem;
    background: rgb(5 8 11 / 68%);
    color: var(--wd-warn);
    font-size: 0.66rem;
    font-weight: 800;
    letter-spacing: 0.08em;
    pointer-events: none;
  }
  :host([mode="focus"]) .mode-label {
    left: auto;
    right: 0.6rem;
    color: var(--wd-accent);
  }
  :host([mode="awb"]) rect.region {
    fill: rgb(124 224 179 / 10%);
    stroke: var(--wd-accent-2);
  }
  :host([mode="awb"]) .mode-label {
    top: auto;
    bottom: 0.6rem;
    color: var(--wd-accent-2);
  }
  .hint {
    position: absolute;
    top: 0.6rem;
    left: 50%;
    transform: translateX(-50%);
    max-width: min(26rem, calc(100% - 1.2rem));
    padding: 0.18rem 0.45rem;
    border-radius: 0.3rem;
    background: rgb(5 8 11 / 72%);
    color: #d7e3ee;
    font-size: 0.66rem;
    pointer-events: none;
  }
  :host(:not([editable])) .hint {
    display: none;
  }
  .readout {
    position: absolute;
    right: 0.6rem;
    bottom: 0.6rem;
    padding: 0.12rem 0.4rem;
    border-radius: 0.3rem;
    background: rgb(5 8 11 / 68%);
    color: #d7e3ee;
    font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
    font-size: 0.64rem;
    pointer-events: none;
  }
  :host(:not([editable])) .readout {
    display: none;
  }
`;

interface Point {
  x: number;
  y: number;
}

export class WdRegionOverlay extends HTMLElement {
  static observedAttributes = ["mode", "label", "editable", "disabled"];

  private _regions: ControlRegion[] = [];
  private _maxRegions = 1;
  private draft: ControlRegion | null = null;
  private pointerStart: Point | null = null;
  private pointerId: number | null = null;
  private rendered = false;

  private regionGroup!: SVGGElement;
  private draftGroup!: SVGGElement;
  private modeLabel!: HTMLDivElement;
  private readout!: HTMLDivElement;

  private readonly onPointerDown = (event: PointerEvent): void => {
    if (!this.editable || this.disabled || event.button !== 0) return;
    const point = this.normalizePoint(event);
    if (!point) return;
    this.focus({ preventScroll: true });
    this.pointerStart = point;
    this.pointerId = event.pointerId;
    this.draft = { x: point.x, y: point.y, w: 0, h: 0 };
    this.setPointerCapture(event.pointerId);
    this.renderDraft();
    event.preventDefault();
    event.stopPropagation();
  };

  private readonly onPointerMove = (event: PointerEvent): void => {
    if (this.pointerId !== event.pointerId || !this.pointerStart) return;
    const point = this.normalizePoint(event);
    if (!point) return;
    this.draft = this.rectBetween(this.pointerStart, point);
    this.renderDraft();
    event.preventDefault();
    event.stopPropagation();
  };

  private readonly onPointerUp = (event: PointerEvent): void => {
    if (this.pointerId !== event.pointerId) return;
    if (this.hasPointerCapture(event.pointerId)) {
      this.releasePointerCapture(event.pointerId);
    }
    const start = this.pointerStart;
    const draft = this.draft;
    this.pointerStart = null;
    this.pointerId = null;
    this.draft = null;
    if (!draft || !start) {
      this.renderDraft();
      return;
    }
    let region = draft;
    if (region.w < 0.02 && region.h < 0.02) {
      region = {
        x: clamp(start.x - 0.1, 0, 0.8),
        y: clamp(start.y - 0.1, 0, 0.8),
        w: 0.2,
        h: 0.2,
      };
    }
    this.commit(region);
    event.preventDefault();
    event.stopPropagation();
  };

  private readonly onPointerCancel = (event: PointerEvent): void => {
    if (this.pointerId !== event.pointerId) return;
    this.pointerStart = null;
    this.pointerId = null;
    this.draft = null;
    this.renderDraft();
  };

  private readonly onKeyDown = (event: KeyboardEvent): void => {
    if (!this.editable || this.disabled) return;
    if (event.key === "Enter" || event.key === " ") {
      if (this.draft) {
        event.preventDefault();
        this.commit(this.draft);
      }
      return;
    }
    if (event.key === "Escape") {
      if (this.draft) {
        event.preventDefault();
        this.draft = null;
        this.renderDraft();
      }
      return;
    }
    const moveKeys = ["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"];
    if (!moveKeys.includes(event.key)) return;
    event.preventDefault();
    const base = this.draft ??
      this.currentRegion() ?? { x: 0.4, y: 0.4, w: 0.2, h: 0.2 };
    const next: ControlRegion = { ...base };
    const step = event.shiftKey ? 0.05 : 0.02;
    switch (event.key) {
      case "ArrowLeft":
        if (event.shiftKey) next.w = Math.max(0.02, next.w - step);
        else next.x -= step;
        break;
      case "ArrowRight":
        if (event.shiftKey) next.w = Math.min(1, next.w + step);
        else next.x += step;
        break;
      case "ArrowUp":
        if (event.shiftKey) next.h = Math.max(0.02, next.h - step);
        else next.y -= step;
        break;
      case "ArrowDown":
        if (event.shiftKey) next.h = Math.min(1, next.h + step);
        else next.y += step;
        break;
      default:
        break;
    }
    this.draft = this.clampRegion(next);
    this.renderDraft();
  };

  get mode(): "metering" | "focus" | "awb" {
    const value = this.getAttribute("mode");
    return value === "focus" || value === "awb" ? value : "metering";
  }

  set mode(value: "metering" | "focus" | "awb") {
    this.setAttribute("mode", value);
  }

  get label(): string {
    return (
      this.getAttribute("label") ??
      (this.mode === "focus"
        ? "Focus region"
        : this.mode === "awb"
          ? "White balance region"
          : "Metering region")
    );
  }

  set label(value: string) {
    this.setAttribute("label", value);
  }

  get editable(): boolean {
    return this.hasAttribute("editable");
  }

  set editable(value: boolean) {
    this.toggleAttribute("editable", value);
  }

  get disabled(): boolean {
    return this.hasAttribute("disabled");
  }

  set disabled(value: boolean) {
    this.toggleAttribute("disabled", value);
  }

  get regions(): ControlRegion[] {
    return this._regions.map((region) => ({ ...region }));
  }

  set regions(value: ControlRegion[]) {
    this._regions = value.map((region) => ({ ...region }));
    this.renderRegions();
    this.renderReadout();
  }

  get maxRegions(): number {
    return this._maxRegions;
  }

  set maxRegions(value: number) {
    this._maxRegions = Math.max(1, Math.floor(value));
  }

  connectedCallback(): void {
    if (!this.shadowRoot) {
      this.attachShadow({ mode: "open" });
      this.render();
    }
    this.addEventListener("pointerdown", this.onPointerDown);
    this.addEventListener("pointermove", this.onPointerMove);
    this.addEventListener("pointerup", this.onPointerUp);
    this.addEventListener("pointercancel", this.onPointerCancel);
    this.addEventListener("keydown", this.onKeyDown);
    this.sync();
  }

  disconnectedCallback(): void {
    this.removeEventListener("pointerdown", this.onPointerDown);
    this.removeEventListener("pointermove", this.onPointerMove);
    this.removeEventListener("pointerup", this.onPointerUp);
    this.removeEventListener("pointercancel", this.onPointerCancel);
    this.removeEventListener("keydown", this.onKeyDown);
  }

  attributeChangedCallback(): void {
    if (this.rendered) this.sync();
  }

  private render(): void {
    const root = this.shadowRoot;
    if (!root) return;
    attachStyles(root, css);
    root.innerHTML = `
      <svg class="canvas" viewBox="0 0 100 100" preserveAspectRatio="none" aria-hidden="true">
        <g class="regions"></g>
        <g class="draft"></g>
      </svg>
      <div class="mode-label" aria-hidden="true"></div>
      <div class="hint" id="hint"></div>
      <div class="readout"></div>
    `;
    this.regionGroup = root.querySelector(".regions") as SVGGElement;
    this.draftGroup = root.querySelector(".draft") as SVGGElement;
    this.modeLabel = root.querySelector(".mode-label") as HTMLDivElement;
    this.readout = root.querySelector(".readout") as HTMLDivElement;
    this.rendered = true;
    this.renderRegions();
    this.renderReadout();
  }

  private sync(): void {
    if (!this.rendered) return;
    const mode = this.mode;
    const label = this.label;
    const editable = this.editable;
    this.modeLabel.textContent =
      mode === "focus" ? "FOCUS" : mode === "awb" ? "WHITE BALANCE" : "METERING";
    this.setAttribute("role", "group");
    this.setAttribute(
      "aria-label",
      `${label}. ${editable ? HINT : "Read-only."}`,
    );
    const hint = this.shadowRoot?.querySelector(".hint");
    if (hint) hint.textContent = HINT;
    this.tabIndex = editable && !this.disabled ? 0 : -1;
    this.renderReadout();
  }

  private normalizePoint(event: PointerEvent): Point | null {
    const rect = this.getBoundingClientRect();
    if (rect.width <= 0 || rect.height <= 0) return null;
    return {
      x: clamp((event.clientX - rect.left) / rect.width, 0, 1),
      y: clamp((event.clientY - rect.top) / rect.height, 0, 1),
    };
  }

  private rectBetween(start: Point, end: Point): ControlRegion {
    const x = Math.min(start.x, end.x);
    const y = Math.min(start.y, end.y);
    const w = Math.abs(end.x - start.x);
    const h = Math.abs(end.y - start.y);
    return this.clampRegion({ x, y, w, h });
  }

  private clampRegion(region: ControlRegion): ControlRegion {
    const w = clamp(region.w, 0.02, 1);
    const h = clamp(region.h, 0.02, 1);
    return {
      x: clamp(region.x, 0, 1 - w),
      y: clamp(region.y, 0, 1 - h),
      w,
      h,
      weight: region.weight ?? 1000,
    };
  }

  private roundRegion(region: ControlRegion): ControlRegion {
    const rounded = (value: number): number => Math.round(value * 1000) / 1000;
    const w = rounded(clamp(region.w, 0.02, 1));
    const h = rounded(clamp(region.h, 0.02, 1));
    return {
      x: rounded(clamp(region.x, 0, 1 - w)),
      y: rounded(clamp(region.y, 0, 1 - h)),
      w,
      h,
      weight: region.weight ?? 1000,
    };
  }

  private currentRegion(): ControlRegion | null {
    return this._regions[0] ?? null;
  }

  private commit(region: ControlRegion): void {
    const rounded = this.roundRegion(region);
    const regions = this._regions.map((item) => ({ ...item }));
    if (regions.length < this._maxRegions) {
      regions.push(rounded);
    } else {
      regions[0] = rounded;
    }
    this.draft = null;
    this.regions = regions;
    this.renderDraft();
    emit(this, "wd-region-change", {
      mode: this.mode,
      region: rounded,
      regions: this.regions,
    });
  }

  private renderRegions(): void {
    if (!this.rendered) return;
    this.regionGroup.replaceChildren();
    for (const region of this._regions) {
      const rect = document.createElementNS("http://www.w3.org/2000/svg", "rect");
      rect.setAttribute("class", "region");
      rect.setAttribute("x", String(region.x * 100));
      rect.setAttribute("y", String(region.y * 100));
      rect.setAttribute("width", String(region.w * 100));
      rect.setAttribute("height", String(region.h * 100));
      rect.setAttribute("rx", "0.6");
      const title = document.createElementNS(
        "http://www.w3.org/2000/svg",
        "title",
      );
      title.textContent = `${this.label}: ${this.describe(region)}`;
      rect.append(title);
      this.regionGroup.append(rect);
    }
  }

  private renderDraft(): void {
    if (!this.rendered) return;
    this.draftGroup.replaceChildren();
    if (this.draft) {
      const rect = document.createElementNS("http://www.w3.org/2000/svg", "rect");
      rect.setAttribute("class", "draft");
      rect.setAttribute("x", String(this.draft.x * 100));
      rect.setAttribute("y", String(this.draft.y * 100));
      rect.setAttribute("width", String(this.draft.w * 100));
      rect.setAttribute("height", String(this.draft.h * 100));
      rect.setAttribute("rx", "0.6");
      this.draftGroup.append(rect);
    }
    this.renderReadout();
  }

  private renderReadout(): void {
    if (!this.rendered) return;
    const region = this.draft ?? this.currentRegion();
    this.readout.textContent = region ? this.describe(region) : "No region";
  }

  private describe(region: ControlRegion): string {
    const pct = (value: number): string =>
      `${Math.round(value * 1000) / 10}%`;
    return `x ${pct(region.x)} · y ${pct(region.y)} · w ${pct(region.w)} · h ${pct(region.h)}`;
  }
}

defineOnce("wd-region-overlay", WdRegionOverlay);
