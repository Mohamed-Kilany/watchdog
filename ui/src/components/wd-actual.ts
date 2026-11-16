import {
  attachStyles,
  baseCss,
  defineOnce,
  formatValue,
  valuesEqual,
} from "./base";

const css = `${baseCss}
  .actual {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.2rem 0.45rem;
    color: var(--wd-muted);
    font-size: 0.74rem;
    font-variant-numeric: tabular-nums;
  }
  .label {
    color: var(--wd-muted);
    font-weight: 600;
  }
  .pair {
    display: inline-flex;
    align-items: baseline;
    gap: 0.25rem;
  }
  .requested,
  .actual-value {
    color: var(--wd-text);
  }
  .arrow {
    color: var(--wd-muted);
  }
  .source {
    color: var(--wd-muted);
    font-size: 0.68rem;
  }
  .badge {
    padding: 0.02rem 0.34rem;
    border: 1px solid var(--wd-border);
    border-radius: 999px;
    font-size: 0.64rem;
    font-weight: 700;
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }
  .badge[data-state="match"] {
    color: var(--wd-ok);
    border-color: color-mix(in srgb, var(--wd-ok) 55%, var(--wd-border));
  }
  .badge[data-state="differs"] {
    color: var(--wd-warn);
    border-color: color-mix(in srgb, var(--wd-warn) 60%, var(--wd-border));
  }
  .badge[data-state="auto"] {
    color: var(--wd-accent);
    border-color: color-mix(in srgb, var(--wd-accent) 55%, var(--wd-border));
  }
  .badge[data-state="pending"],
  .badge[data-state="n/a"] {
    color: var(--wd-muted);
  }
`;

export class WdActual extends HTMLElement {
  static observedAttributes = [
    "control-id",
    "label",
    "unit",
    "precision",
    "source",
    "supported",
  ];

  private _requested: unknown = undefined;
  private _actual: unknown = undefined;
  private rendered = false;

  private labelEl!: HTMLSpanElement;
  private requestedEl!: HTMLSpanElement;
  private actualEl!: HTMLSpanElement;
  private unitEl!: HTMLSpanElement;
  private sourceEl!: HTMLSpanElement;
  private badge!: HTMLSpanElement;

  get controlId(): string {
    return this.getAttribute("control-id") ?? "";
  }

  set controlId(value: string) {
    this.setAttribute("control-id", value);
  }

  get label(): string {
    return this.getAttribute("label") ?? "";
  }

  set label(value: string) {
    this.setAttribute("label", value);
  }

  get unit(): string {
    return this.getAttribute("unit") ?? "";
  }

  set unit(value: string) {
    this.setAttribute("unit", value);
  }

  get precision(): number {
    const attr = this.getAttribute("precision");
    if (attr === null || attr.trim() === "") return 3;
    const value = Number(attr);
    return Number.isFinite(value) ? value : 3;
  }

  set precision(value: number) {
    this.setAttribute("precision", String(value));
  }

  get source(): string {
    return this.getAttribute("source") ?? "";
  }

  set source(value: string) {
    this.setAttribute("source", value);
  }

  get supported(): boolean {
    return this.getAttribute("supported") !== "false";
  }

  set supported(value: boolean) {
    this.setAttribute("supported", String(value));
  }

  get requested(): unknown {
    return this._requested;
  }

  set requested(value: unknown) {
    this._requested = value;
    this.sync();
  }

  get actual(): unknown {
    return this._actual;
  }

  set actual(value: unknown) {
    this._actual = value;
    this.sync();
  }

  connectedCallback(): void {
    if (!this.shadowRoot) {
      this.attachShadow({ mode: "open" });
      this.render();
    }
    this.sync();
  }

  attributeChangedCallback(): void {
    if (this.rendered) this.sync();
  }

  private render(): void {
    const root = this.shadowRoot;
    if (!root) return;
    attachStyles(root, css);
    root.innerHTML = `
      <span class="actual" role="group">
        <span class="label"></span>
        <span class="pair">
          <span class="requested"></span>
          <span class="arrow" aria-hidden="true">→</span>
          <span class="actual-value"></span>
          <span class="unit"></span>
        </span>
        <span class="source"></span>
        <span class="badge" role="status" aria-live="polite"></span>
      </span>
    `;
    this.labelEl = root.querySelector(".label") as HTMLSpanElement;
    this.requestedEl = root.querySelector(".requested") as HTMLSpanElement;
    this.actualEl = root.querySelector(".actual-value") as HTMLSpanElement;
    this.unitEl = root.querySelector(".unit") as HTMLSpanElement;
    this.sourceEl = root.querySelector(".source") as HTMLSpanElement;
    this.badge = root.querySelector(".badge") as HTMLSpanElement;
    this.rendered = true;
  }

  private sync(): void {
    if (!this.rendered) return;
    const requested = this._requested;
    const actual = this._actual;
    const hasRequested = requested !== undefined && requested !== null;
    const hasActual = actual !== undefined && actual !== null;
    const source = this.source;
    const supported = this.supported;

    let state: "match" | "differs" | "auto" | "pending" | "n/a";
    if (!supported) {
      state = "n/a";
    } else if (!hasActual) {
      state = "pending";
    } else if (!hasRequested) {
      state = source === "auto" ? "auto" : "pending";
    } else if (valuesEqual(requested, actual)) {
      state = "match";
    } else {
      state = "differs";
    }

    const requestedText = hasRequested
      ? formatValue(requested, this.precision, this.unit)
      : source === "auto"
        ? "auto"
        : "—";
    const actualText = hasActual
      ? formatValue(actual, this.precision, this.unit)
      : "—";

    this.labelEl.textContent = this.label;
    this.requestedEl.textContent = requestedText;
    this.actualEl.textContent = actualText;
    this.unitEl.textContent = "";
    this.sourceEl.textContent = source ? `source ${source}` : "";
    this.badge.textContent = state === "n/a" ? "n/a" : state;
    this.badge.dataset.state = state;
    this.dataset.state = state;
    this.setAttribute(
      "aria-label",
      `${this.label || this.controlId}: requested ${requestedText}, actual ${actualText}, ${state}`,
    );
  }
}

defineOnce("wd-actual", WdActual);
