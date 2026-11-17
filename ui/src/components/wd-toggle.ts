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
    border-color: var(--wd-accent-2);
    color: var(--wd-accent-2);
  }
  .dot {
    width: 0.5rem;
    height: 0.5rem;
    border-radius: 50%;
    background: var(--wd-muted);
  }
  button[aria-checked="true"] .dot {
    background: var(--wd-accent-2);
    box-shadow: 0 0 0 0.2rem rgb(124 224 179 / 16%);
  }
  .badge {
    min-width: 5.2rem;
    text-align: right;
    color: var(--wd-muted);
    font-size: 0.7rem;
    font-weight: 700;
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }
  .badge[data-state="on"] {
    color: var(--wd-accent-2);
  }
  .badge[data-state="pending"] {
    color: var(--wd-accent);
  }
`;

/** Boolean control rendered as an on/off switch with a live state badge. */
export class WdToggle extends HTMLElement {
  static observedAttributes = [
    "control-id",
    "label",
    "on",
    "pending",
    "supported",
    "disabled",
  ];

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
    return this.getAttribute("label") ?? "Enabled";
  }

  set label(value: string) {
    this.setAttribute("label", value);
  }

  get on(): boolean {
    return this.hasAttribute("on");
  }

  set on(value: boolean) {
    this.toggleAttribute("on", value);
  }

  get pending(): boolean {
    return this.hasAttribute("pending");
  }

  set pending(value: boolean) {
    this.toggleAttribute("pending", value);
  }

  get supported(): boolean {
    return this.getAttribute("supported") !== "false";
  }

  set supported(value: boolean) {
    this.setAttribute("supported", String(value));
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
      if (this.disabled || !this.supported) return;
      emit(this, "wd-toggle-change", { id: this.controlId, on: !this.on });
    });
  }

  private sync(): void {
    if (!this.rendered) return;
    const on = this.on;
    const supported = this.supported;
    const pending = this.pending;
    const state = !supported ? "unsupported" : pending ? "pending" : on ? "on" : "off";
    this.labelEl.textContent = this.label;
    this.stateEl.textContent = on ? "On" : "Off";
    this.button.setAttribute("aria-checked", String(on));
    this.button.setAttribute(
      "aria-label",
      `${this.label}: ${on ? "on" : "off"}`,
    );
    this.button.disabled = this.disabled || !supported;
    this.badge.textContent =
      state === "unsupported"
        ? "Unsupported"
        : state === "pending"
          ? "Pending"
          : on
            ? "On"
            : "Off";
    this.badge.dataset.state = state;
    this.dataset.state = state;
  }
}

defineOnce("wd-toggle", WdToggle);
