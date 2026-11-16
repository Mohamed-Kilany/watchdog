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
  }
  button[aria-pressed="true"] {
    border-color: var(--wd-warn);
    color: var(--wd-warn);
  }
  .dot {
    width: 0.5rem;
    height: 0.5rem;
    border-radius: 50%;
    background: var(--wd-muted);
  }
  button[aria-pressed="true"] .dot {
    background: var(--wd-warn);
    box-shadow: 0 0 0 0.2rem rgb(255 184 77 / 16%);
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
  .badge[data-state="locked"] {
    color: var(--wd-warn);
  }
  .badge[data-state="pending"] {
    color: var(--wd-accent);
  }
  .badge[data-state="unsupported"] {
    color: var(--wd-muted);
  }
`;

export class WdLockButton extends HTMLElement {
  static observedAttributes = [
    "control-id",
    "label",
    "locked",
    "pending",
    "supported",
    "disabled",
  ];

  private rendered = false;
  private labelEl!: HTMLSpanElement;
  private button!: HTMLButtonElement;
  private textEl!: HTMLSpanElement;
  private badge!: HTMLSpanElement;

  get controlId(): string {
    return this.getAttribute("control-id") ?? "";
  }

  set controlId(value: string) {
    this.setAttribute("control-id", value);
  }

  get label(): string {
    return this.getAttribute("label") ?? "Lock";
  }

  set label(value: string) {
    this.setAttribute("label", value);
  }

  get locked(): boolean {
    return this.hasAttribute("locked");
  }

  set locked(value: boolean) {
    this.toggleAttribute("locked", value);
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
        <button type="button" aria-pressed="false">
          <span class="dot" aria-hidden="true"></span>
          <span class="text"></span>
        </button>
        <span class="badge" role="status" aria-live="polite"></span>
      </div>
    `;
    this.labelEl = root.querySelector(".label") as HTMLSpanElement;
    this.button = root.querySelector("button") as HTMLButtonElement;
    this.textEl = root.querySelector(".text") as HTMLSpanElement;
    this.badge = root.querySelector(".badge") as HTMLSpanElement;
    this.rendered = true;
    this.button.addEventListener("click", () => {
      if (this.disabled || !this.supported) return;
      emit(this, "wd-lock-change", {
        id: this.controlId,
        locked: !this.locked,
      });
    });
  }

  private sync(): void {
    if (!this.rendered) return;
    const locked = this.locked;
    const pending = this.pending;
    const supported = this.supported;
    const disabled = this.disabled || !supported;
    const state = !supported ? "unsupported" : pending ? "pending" : locked ? "locked" : "unlocked";

    this.labelEl.textContent = this.label;
    this.textEl.textContent = locked ? "Unlock" : "Lock";
    this.button.setAttribute("aria-pressed", String(locked));
    this.button.disabled = disabled;
    this.button.setAttribute(
      "aria-label",
      `${this.label}: ${state === "unsupported" ? "unsupported" : locked ? "locked" : "unlocked"}`,
    );
    this.badge.textContent =
      state === "unsupported"
        ? "Unsupported"
        : state === "pending"
          ? "Pending"
          : locked
            ? "Locked"
            : "Unlocked";
    this.badge.dataset.state = state;
    this.dataset.state = state;
  }
}

defineOnce("wd-lock-button", WdLockButton);
