import { attachStyles, baseCss, defineOnce, emit } from "./base";

const css = `${baseCss}
  button {
    min-height: 2rem;
    padding: 0.25rem 0.6rem;
    border: 1px solid var(--wd-border);
    border-radius: 0.45rem;
    background: transparent;
    color: var(--wd-muted);
    font-size: 0.76rem;
    font-weight: 700;
  }
  button:hover {
    border-color: var(--wd-accent);
    color: var(--wd-accent);
  }
`;

export class WdReset extends HTMLElement {
  static observedAttributes = ["label", "disabled"];

  private _targets: string[] = [];
  private rendered = false;
  private button!: HTMLButtonElement;

  get label(): string {
    return this.getAttribute("label") ?? "Reset to auto";
  }

  set label(value: string) {
    this.setAttribute("label", value);
  }

  get targets(): string[] {
    return [...this._targets];
  }

  set targets(value: string[]) {
    this._targets = [...value];
    if (this.rendered) this.sync();
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
    this.button = document.createElement("button");
    this.button.type = "button";
    root.append(this.button);
    this.rendered = true;
    this.button.addEventListener("click", () => {
      if (this.disabled) return;
      emit(this, "wd-reset", { targets: [...this._targets] });
    });
  }

  private sync(): void {
    if (!this.rendered) return;
    this.button.textContent = this.label;
    this.button.disabled = this.disabled;
    this.button.setAttribute("aria-label", this.label);
    this.dataset.targets = this._targets.join(",");
  }
}

defineOnce("wd-reset", WdReset);
