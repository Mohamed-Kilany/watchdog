import { attachStyles, baseCss, defineOnce, emit } from "./base";
import type { SelectOption } from "../types";

const css = `${baseCss}
  .row {
    display: grid;
    gap: 0.25rem;
  }
  .head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.5rem;
  }
  .label {
    font-weight: 600;
  }
  select {
    width: 100%;
    min-height: 2.2rem;
    padding: 0.25rem 0.45rem;
    border: 1px solid var(--wd-border);
    border-radius: 0.45rem;
    background: var(--wd-panel-2);
    color: var(--wd-text);
  }
`;

export class WdSelect extends HTMLElement {
  static observedAttributes = ["control-id", "label", "disabled"];

  private _options: SelectOption[] = [];
  private _value: unknown;
  private rendered = false;

  private labelEl!: HTMLSpanElement;
  private select!: HTMLSelectElement;

  get controlId(): string {
    return this.getAttribute("control-id") ?? "";
  }

  set controlId(value: string) {
    this.setAttribute("control-id", value);
  }

  get label(): string {
    return this.getAttribute("label") ?? "Option";
  }

  set label(value: string) {
    this.setAttribute("label", value);
  }

  get disabled(): boolean {
    return this.hasAttribute("disabled");
  }

  set disabled(value: boolean) {
    this.toggleAttribute("disabled", value);
  }

  get options(): SelectOption[] {
    return this._options;
  }

  set options(value: SelectOption[]) {
    this._options = value.map((option) => ({ ...option }));
    this.renderOptions();
    this.sync();
  }

  get value(): unknown {
    return this._value;
  }

  set value(value: unknown) {
    this._value = value;
    this.sync();
  }

  connectedCallback(): void {
    if (!this.shadowRoot) {
      this.attachShadow({ mode: "open" });
      this.render();
    }
    this.renderOptions();
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
        <div class="head"><span class="label" id="label"></span></div>
        <select aria-labelledby="label"></select>
      </div>
    `;
    this.labelEl = root.querySelector(".label") as HTMLSpanElement;
    this.select = root.querySelector("select") as HTMLSelectElement;
    this.rendered = true;
    this.select.addEventListener("change", () => {
      const option = this._options[this.select.selectedIndex];
      if (!option) return;
      this._value = option.value;
      emit(this, "wd-commit", { id: this.controlId, value: option.value });
    });
  }

  private renderOptions(): void {
    if (!this.rendered) return;
    this.select.replaceChildren();
    if (this._options.length === 0) {
      const option = document.createElement("option");
      option.textContent = "No options";
      option.value = "";
      option.disabled = true;
      this.select.append(option);
      return;
    }
    for (const option of this._options) {
      const element = document.createElement("option");
      element.textContent = option.label;
      element.value = String(this._options.indexOf(option));
      this.select.append(element);
    }
  }

  private sync(): void {
    if (!this.rendered) return;
    this.labelEl.textContent = this.label;
    this.select.disabled = this.disabled;
    this.select.setAttribute("aria-label", this.label);
    const index = this._options.findIndex((option) =>
      Object.is(option.value, this._value),
    );
    this.select.selectedIndex = index;
    this.dataset.value = this._value === undefined ? "" : String(this._value);
  }
}

defineOnce("wd-select", WdSelect);
