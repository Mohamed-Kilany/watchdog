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
    min-height: 2.2rem;
    padding: 0.25rem 0.7rem;
    border: 1px solid var(--wd-border);
    border-radius: 0.45rem;
    background: var(--wd-panel-2);
    color: var(--wd-text);
    font-weight: 700;
  }
  button:hover:not(:disabled) {
    border-color: var(--wd-accent);
    color: var(--wd-accent);
  }
  .badge {
    min-width: 4.5rem;
    text-align: right;
    color: var(--wd-muted);
    font-size: 0.7rem;
    font-weight: 700;
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }
  .badge[data-state="pending"] {
    color: var(--wd-accent);
  }
`;

/** One-shot trigger rendered as a button (AF trigger, AE precapture). */
export class WdTrigger extends HTMLElement {
  static observedAttributes = [
    "control-id",
    "label",
    "action",
    "pending",
    "supported",
    "disabled",
  ];

  private rendered = false;
  private labelEl!: HTMLSpanElement;
  private button!: HTMLButtonElement;
  private badge!: HTMLSpanElement;

  get controlId(): string {
    return this.getAttribute("control-id") ?? "";
  }

  set controlId(value: string) {
    this.setAttribute("control-id", value);
  }

  get label(): string {
    return this.getAttribute("label") ?? "Trigger";
  }

  set label(value: string) {
    this.setAttribute("label", value);
  }

  get action(): string {
    return this.getAttribute("action") ?? "Run";
  }

  set action(value: string) {
    this.setAttribute("action", value);
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
        <button type="button"></button>
        <span class="badge" role="status" aria-live="polite"></span>
      </div>
    `;
    this.labelEl = root.querySelector(".label") as HTMLSpanElement;
    this.button = root.querySelector("button") as HTMLButtonElement;
    this.badge = root.querySelector(".badge") as HTMLSpanElement;
    this.rendered = true;
    this.button.addEventListener("click", () => {
      if (this.disabled || !this.supported) return;
      emit(this, "wd-trigger", { id: this.controlId });
    });
  }

  private sync(): void {
    if (!this.rendered) return;
    const supported = this.supported;
    const state = !supported ? "unsupported" : this.pending ? "pending" : "ready";
    this.labelEl.textContent = this.label;
    this.button.textContent = this.action;
    this.button.disabled = this.disabled || !supported;
    this.button.setAttribute("aria-label", `${this.label}: ${this.action}`);
    this.badge.textContent =
      state === "unsupported" ? "Unsupported" : state === "pending" ? "Pending" : "Ready";
    this.badge.dataset.state = state;
    this.dataset.state = state;
  }
}

defineOnce("wd-trigger", WdTrigger);
