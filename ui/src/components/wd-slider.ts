import {
  attachStyles,
  baseCss,
  clamp,
  decimalsForStep,
  defineOnce,
  emit,
  formatNumber,
  parseNumber,
  roundToStep,
} from "./base";

const css = `${baseCss}
  .row {
    display: grid;
    gap: 0.25rem;
  }
  .head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.6rem;
  }
  .label {
    font-weight: 600;
    color: var(--wd-text);
  }
  .value-box {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
  }
  .number {
    width: 6.5rem;
    min-height: 2rem;
    padding: 0.2rem 0.4rem;
    border: 1px solid var(--wd-border);
    border-radius: 0.4rem;
    background: var(--wd-panel-2);
    color: var(--wd-text);
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  .unit {
    color: var(--wd-muted);
    font-size: 0.78rem;
  }
  .track-wrap {
    position: relative;
    padding: 0.4rem 0 0.3rem;
  }
  .range {
    width: 100%;
    margin: 0;
    accent-color: var(--wd-accent);
  }
  .actual-marker {
    position: absolute;
    top: 0.2rem;
    bottom: 0.1rem;
    width: 2px;
    background: var(--wd-warn);
    transform: translateX(-50%);
    pointer-events: none;
  }
  .actual-marker::after {
    content: "";
    position: absolute;
    top: -0.15rem;
    left: -0.2rem;
    width: 0.45rem;
    height: 0.45rem;
    border-radius: 50%;
    background: var(--wd-warn);
  }
  .scale {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.5rem;
    color: var(--wd-muted);
    font-size: 0.72rem;
    font-variant-numeric: tabular-nums;
  }
  .actual-readout {
    color: var(--wd-muted);
  }
  :host([data-mismatch]) .actual-readout {
    color: var(--wd-warn);
  }
`;

export class WdSlider extends HTMLElement {
  static observedAttributes = [
    "control-id",
    "label",
    "min",
    "max",
    "step",
    "value",
    "actual",
    "unit",
    "precision",
    "disabled",
  ];

  private _value = 0;
  private _actual: number | null = null;
  private rendered = false;
  private pending = false;

  private range!: HTMLInputElement;
  private numberInput!: HTMLInputElement;
  private labelEl!: HTMLSpanElement;
  private unitEl!: HTMLSpanElement;
  private minEl!: HTMLSpanElement;
  private maxEl!: HTMLSpanElement;
  private marker!: HTMLDivElement;
  private actualReadout!: HTMLSpanElement;

  get controlId(): string {
    return this.getAttribute("control-id") ?? "";
  }

  set controlId(value: string) {
    this.setAttribute("control-id", value);
  }

  get label(): string {
    return this.getAttribute("label") ?? "Value";
  }

  set label(value: string) {
    this.setAttribute("label", value);
  }

  get min(): number {
    return parseNumber(this.getAttribute("min"), 0);
  }

  set min(value: number) {
    this.setAttribute("min", String(value));
  }

  get max(): number {
    return parseNumber(this.getAttribute("max"), 100);
  }

  set max(value: number) {
    this.setAttribute("max", String(value));
  }

  get step(): number {
    return parseNumber(this.getAttribute("step"), 1);
  }

  set step(value: number) {
    this.setAttribute("step", String(value));
  }

  get precision(): number {
    const attr = this.getAttribute("precision");
    return attr === null ? decimalsForStep(this.step) : parseNumber(attr, 0);
  }

  set precision(value: number) {
    this.setAttribute("precision", String(value));
  }

  get unit(): string {
    return this.getAttribute("unit") ?? "";
  }

  set unit(value: string) {
    this.setAttribute("unit", value);
  }

  get value(): number {
    return this._value;
  }

  set value(value: number) {
    this._value = clamp(Number.isFinite(value) ? value : this.min, this.min, this.max);
    this.sync();
  }

  get actual(): number | null {
    return this._actual;
  }

  set actual(value: number | null) {
    this._actual = value !== null && Number.isFinite(value) ? value : null;
    this.sync();
  }

  get disabled(): boolean {
    return this.hasAttribute("disabled");
  }

  set disabled(value: boolean) {
    this.toggleAttribute("disabled", value);
  }

  connectedCallback(): void {
    if (!this.shadowRoot) {
      this.attachShadow({ mode: "open" });
      this.render();
    }
    this.sync();
  }

  attributeChangedCallback(
    name: string,
    oldValue: string | null,
    newValue: string | null,
  ): void {
    if (name === "value") {
      this._value = parseNumber(newValue, this._value);
    } else if (name === "actual") {
      this._actual =
        newValue === null || newValue.trim() === ""
          ? null
          : parseNumber(newValue, 0);
    }
    if (this.rendered && oldValue !== newValue) this.sync();
  }

  private render(): void {
    const root = this.shadowRoot;
    if (!root) return;
    attachStyles(root, css);
    root.innerHTML = `
      <div class="row">
        <div class="head">
          <span class="label" id="label"></span>
          <span class="value-box">
            <input class="number" type="number" inputmode="decimal" aria-labelledby="label" />
            <span class="unit"></span>
          </span>
        </div>
        <div class="track-wrap">
          <input class="range" type="range" aria-labelledby="label" />
          <div class="actual-marker" hidden></div>
        </div>
        <div class="scale">
          <span class="min"></span>
          <span class="actual-readout"></span>
          <span class="max"></span>
        </div>
      </div>
    `;
    this.range = root.querySelector(".range") as HTMLInputElement;
    this.numberInput = root.querySelector(".number") as HTMLInputElement;
    this.labelEl = root.querySelector(".label") as HTMLSpanElement;
    this.unitEl = root.querySelector(".unit") as HTMLSpanElement;
    this.minEl = root.querySelector(".min") as HTMLSpanElement;
    this.maxEl = root.querySelector(".max") as HTMLSpanElement;
    this.marker = root.querySelector(".actual-marker") as HTMLDivElement;
    this.actualReadout = root.querySelector(".actual-readout") as HTMLSpanElement;
    this.rendered = true;

    this.range.addEventListener("input", () => {
      this.pending = true;
      this.numberInput.value = this.range.value;
    });
    this.range.addEventListener("change", () => {
      if (this.pending) this.commit(parseNumber(this.range.value, this._value));
    });
    this.numberInput.addEventListener("input", () => {
      this.pending = true;
    });
    this.numberInput.addEventListener("change", () => {
      if (this.pending) {
        this.commit(parseNumber(this.numberInput.value, this._value));
      }
    });
    this.numberInput.addEventListener("keydown", (event) => {
      if (event.key !== "Enter") return;
      event.preventDefault();
      this.commit(parseNumber(this.numberInput.value, this._value));
      this.numberInput.blur();
    });
    this.numberInput.addEventListener("blur", () => {
      if (this.pending) {
        this.commit(parseNumber(this.numberInput.value, this._value));
      }
    });
  }

  private commit(raw: number): void {
    if (this.disabled) return;
    const next = roundToStep(raw, this.min, this.max, this.step);
    this._value = next;
    this.pending = false;
    this.sync();
    emit(this, "wd-commit", { id: this.controlId, value: next });
  }

  private sync(): void {
    if (!this.rendered) return;
    const min = this.min;
    const max = this.max;
    const step = this.step;
    const precision = this.precision;
    const unit = this.unit;
    const value = clamp(this._value, min, max);
    const disabled = this.disabled;

    this.labelEl.textContent = this.label;
    this.unitEl.textContent = unit;
    this.minEl.textContent = formatNumber(min, precision);
    this.maxEl.textContent = formatNumber(max, precision);

    this.range.min = String(min);
    this.range.max = String(max);
    this.range.step = String(step);
    this.range.value = String(roundToStep(value, min, max, step));
    this.range.disabled = disabled;
    this.range.setAttribute(
      "aria-valuetext",
      `${formatNumber(value, precision)}${unit ? ` ${unit}` : ""}`,
    );

    this.numberInput.min = String(min);
    this.numberInput.max = String(max);
    this.numberInput.step = String(step);
    this.numberInput.value = formatNumber(value, precision);
    this.numberInput.disabled = disabled;

    const actual =
      this._actual !== null && Number.isFinite(this._actual)
        ? this._actual
        : null;
    const mismatch = actual !== null && Math.abs(actual - value) > step / 2;
    this.marker.hidden = actual === null;
    if (actual !== null) {
      const pct = ((clamp(actual, min, max) - min) / (max - min || 1)) * 100;
      this.marker.style.left = `${pct}%`;
      this.marker.title = `Actual ${formatNumber(actual, precision)}${unit ? ` ${unit}` : ""}`;
      this.actualReadout.textContent = `actual ${formatNumber(actual, precision)}${unit ? ` ${unit}` : ""}`;
    } else {
      this.marker.style.left = "0%";
      this.actualReadout.textContent = "actual —";
    }
    this.toggleAttribute("data-mismatch", mismatch);
  }
}

defineOnce("wd-slider", WdSlider);
