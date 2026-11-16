import { attachStyles, baseCss, defineOnce, emit } from "./base";

const css = `${baseCss}
  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 0.35rem 0.6rem;
    padding: 0.1rem 0;
  }
  .label {
    flex: 1 1 8rem;
    font-weight: 600;
  }
  button {
    display: inline-flex;
    align-items: center;
    gap: 0.45rem;
    min-height: 2.2rem;
    padding: 0.25rem 0.65rem;
    border: 1px solid var(--wd-border);
    border-radius: 999px;
    background: var(--wd-panel-2);
    color: var(--wd-text);
    font-weight: 700;
    letter-spacing: 0.02em;
  }
  button[aria-checked="true"] {
    border-color: var(--wd-accent);
    color: var(--wd-accent);
  }
  .dot {
    width: 0.5rem;
    height: 0.5rem;
    border-radius: 50%;
    background: var(--wd-muted);
  }
  button[aria-checked="true"] .dot {
    background: var(--wd-accent);
    box-shadow: 0 0 0 0.2rem rgb(76 194 255 / 16%);
  }
  .badge {
    color: var(--wd-muted);
    font-size: 0.7rem;
    font-variant-numeric: tabular-nums;
  }
`;

export class WdAutoToggle extends HTMLElement {
  static observedAttributes = ["control-id", "label", "auto", "source", "disabled"];

  private rendered = false;
  private labelEl!: HTMLSpanElement;
  private button!: HTMLButtonElement;
  private stateEl!: HTMLSpanElement;
  private badge!: HTMLSpanElement;

  get controlId(): string {
    return this.getAttribute("control-id") ?? "";
  }

  set controlId(value: string) {
    this.setAttribute("control-id", value);
  }

  get label(): string {
    return this.getAttribute("label") ?? "Automatic";
  }

  set label(value: string) {
    this.setAttribute("label", value);
  }

  get auto(): boolean {
    return this.hasAttribute("auto");
  }

  set auto(value: boolean) {
    this.toggleAttribute("auto", value);
  }

  get source(): string {
    return this.getAttribute("source") ?? "";
  }

  set source(value: string) {
    this.setAttribute("source", value);
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

  attributeChangedCallback(): void {
    if (this.rendered) this.sync();
  }

  private render(): void {
    const root = this.shadowRoot;
    if (!root) return;
    attachStyles(root, css);
    root.innerHTML = `
      <div class="row">
        <span class="label"></span>
        <button type="button" role="switch" aria-checked="false">
          <span class="dot" aria-hidden="true"></span>
          <span class="state"></span>
        </button>
        <span class="badge" role="status" aria-live="polite"></span>
      </div>
    `;
    this.labelEl = root.querySelector(".label") as HTMLSpanElement;
    this.button = root.querySelector("button") as HTMLButtonElement;
    this.stateEl = root.querySelector(".state") as HTMLSpanElement;
    this.badge = root.querySelector(".badge") as HTMLSpanElement;
    this.rendered = true;
    this.button.addEventListener("click", () => {
      if (this.disabled) return;
      emit(this, "wd-auto-change", { id: this.controlId, auto: !this.auto });
    });
  }

  private sync(): void {
    if (!this.rendered) return;
    const auto = this.auto;
    const label = this.label;
    const disabled = this.disabled;
    this.labelEl.textContent = label;
    this.stateEl.textContent = auto ? "Auto" : "Manual";
    this.button.setAttribute("aria-checked", String(auto));
    this.button.setAttribute(
      "aria-label",
      `${label}: ${auto ? "automatic" : "manual"}`,
    );
    this.button.disabled = disabled;
    this.badge.textContent = this.source
      ? `live: ${this.source}`
      : auto
        ? "live: auto"
        : "live: manual";
    this.dataset.auto = String(auto);
  }
}

defineOnce("wd-auto-toggle", WdAutoToggle);
