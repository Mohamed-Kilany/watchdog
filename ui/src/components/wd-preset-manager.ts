import { attachStyles, baseCss, defineOnce, emit } from "./base";
import type { PresetSource, PresetSummary } from "../presets";

const css = `${baseCss}
  .manager {
    display: grid;
    gap: 0.45rem;
  }
  h2 {
    margin: 0;
    font-size: 0.82rem;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--wd-muted);
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    gap: 0.4rem;
  }
  input,
  select {
    flex: 1 1 9rem;
    min-width: 0;
    min-height: 2.1rem;
    padding: 0.25rem 0.45rem;
    border: 1px solid var(--wd-border);
    border-radius: 0.45rem;
    background: var(--wd-panel-2);
    color: var(--wd-text);
  }
  button {
    min-height: 2.1rem;
    padding: 0.25rem 0.65rem;
    border: 1px solid var(--wd-border);
    border-radius: 0.45rem;
    background: transparent;
    color: var(--wd-text);
    font-weight: 700;
  }
  button:hover:not(:disabled) {
    border-color: var(--wd-accent);
    color: var(--wd-accent);
  }
  button.primary {
    border-color: var(--wd-accent);
    color: var(--wd-accent);
  }
  button.danger:hover:not(:disabled) {
    border-color: var(--wd-danger);
    color: var(--wd-danger);
  }
  .status {
    margin: 0;
    min-height: 1rem;
    color: var(--wd-muted);
    font-size: 0.72rem;
  }
  .status[data-kind="ok"] {
    color: var(--wd-ok);
  }
  .status[data-kind="error"] {
    color: var(--wd-danger);
  }
  .hint {
    margin: 0;
    color: var(--wd-muted);
    font-size: 0.68rem;
  }
`;

/** Save, apply and delete host-stored control presets. */
export class WdPresetManager extends HTMLElement {
  static observedAttributes = ["disabled"];

  private _source: PresetSource | null = null;
  private _controls: Record<string, unknown> = {};
  private summaries: PresetSummary[] = [];
  private rendered = false;
  private busy = false;

  private nameInput!: HTMLInputElement;
  private select!: HTMLSelectElement;
  private saveButton!: HTMLButtonElement;
  private applyButton!: HTMLButtonElement;
  private deleteButton!: HTMLButtonElement;
  private statusEl!: HTMLParagraphElement;

  get disabled(): boolean {
    return this.hasAttribute("disabled");
  }

  set disabled(value: boolean) {
    this.toggleAttribute("disabled", value);
  }

  get source(): PresetSource | null {
    return this._source;
  }

  set source(value: PresetSource | null) {
    this._source = value;
    if (this.rendered) void this.refresh();
  }

  /** Current writable controls captured from the live state. */
  get controls(): Record<string, unknown> {
    return this._controls;
  }

  set controls(value: Record<string, unknown>) {
    this._controls = value;
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

  /** Re-read the preset list (after save/delete or a backend switch). */
  async refresh(): Promise<void> {
    if (!this.rendered) return;
    const source = this._source;
    if (!source) {
      this.summaries = [];
      this.renderOptions();
      this.setStatus("Presets need a device session or the demo backend.", "error");
      return;
    }
    try {
      this.summaries = await source.list();
      this.renderOptions();
      if (this.summaries.length === 0) {
        this.setStatus("No saved presets yet.", "idle");
      } else {
        this.setStatus(
          `${this.summaries.length} preset${this.summaries.length === 1 ? "" : "s"} stored ${
            source.mode === "host" ? "on the host" : "in this browser"
          }.`,
          "idle",
        );
      }
    } catch (error) {
      this.summaries = [];
      this.renderOptions();
      this.setStatus(messageOf(error), "error");
    }
    this.sync();
  }

  /** Show the outcome of an apply request from the shell. */
  report(message: string, ok: boolean): void {
    this.setStatus(message, ok ? "ok" : "error");
  }

  private render(): void {
    const root = this.shadowRoot;
    if (!root) return;
    attachStyles(root, css);
    root.innerHTML = `
      <section class="manager" part="manager" aria-label="Control presets">
        <h2>Presets</h2>
        <div class="row">
          <input
            class="name"
            data-testid="preset-name"
            type="text"
            maxlength="64"
            placeholder="Preset name"
            aria-label="Preset name"
          />
          <button class="save primary" data-testid="preset-save" type="button">
            Save current
          </button>
        </div>
        <div class="row">
          <select class="presets" data-testid="preset-select" aria-label="Saved presets">
            <option value="">No presets</option>
          </select>
          <button class="apply" data-testid="preset-apply" type="button">Apply</button>
          <button class="delete danger" data-testid="preset-delete" type="button">
            Delete
          </button>
        </div>
        <p class="hint">
          Saves every writable control except camera selection, output structure and
          one-shot triggers. Applying sends one batch with camera ids first.
        </p>
        <p class="status" data-testid="preset-status" role="status" aria-live="polite"></p>
      </section>
    `;
    this.nameInput = root.querySelector(".name") as HTMLInputElement;
    this.select = root.querySelector(".presets") as HTMLSelectElement;
    this.saveButton = root.querySelector(".save") as HTMLButtonElement;
    this.applyButton = root.querySelector(".apply") as HTMLButtonElement;
    this.deleteButton = root.querySelector(".delete") as HTMLButtonElement;
    this.statusEl = root.querySelector(".status") as HTMLParagraphElement;
    this.rendered = true;

    this.saveButton.addEventListener("click", () => void this.save());
    this.applyButton.addEventListener("click", () => void this.apply());
    this.deleteButton.addEventListener("click", () => void this.delete());
    this.select.addEventListener("change", () => this.sync());
  }

  private async save(): Promise<void> {
    const source = this._source;
    if (!source || this.busy) return;
    const name = this.nameInput.value.trim();
    if (!name) {
      this.setStatus("Enter a preset name first.", "error");
      this.nameInput.focus();
      return;
    }
    const count = Object.keys(this._controls).length;
    if (count === 0) {
      this.setStatus("There are no writable controls to save.", "error");
      return;
    }
    this.setBusy(true);
    try {
      const preset = await source.save(name, this._controls);
      this.nameInput.value = preset.name;
      emit(this, "wd-preset-saved", { name: preset.name });
      await this.refresh();
      this.select.value = preset.name;
      this.setStatus(
        `Saved "${preset.name}" with ${Object.keys(preset.controls).length} controls.`,
        "ok",
      );
      this.sync();
    } catch (error) {
      this.setStatus(messageOf(error), "error");
    } finally {
      this.setBusy(false);
    }
  }

  private async apply(): Promise<void> {
    const source = this._source;
    if (!source || this.busy) return;
    const name = this.select.value;
    if (!name) {
      this.setStatus("Choose a preset to apply.", "error");
      return;
    }
    this.setBusy(true);
    try {
      const preset = await source.load(name);
      const controls = preset.controls;
      const count = Object.keys(controls).length;
      emit(this, "wd-preset-apply", { name: preset.name, controls });
      this.setStatus(`Applying "${preset.name}" (${count} controls)…`, "idle");
    } catch (error) {
      this.setStatus(messageOf(error), "error");
    } finally {
      this.setBusy(false);
    }
  }

  private async delete(): Promise<void> {
    const source = this._source;
    if (!source || this.busy) return;
    const name = this.select.value;
    if (!name) {
      this.setStatus("Choose a preset to delete.", "error");
      return;
    }
    this.setBusy(true);
    try {
      await source.remove(name);
      emit(this, "wd-preset-deleted", { name });
      await this.refresh();
      this.setStatus(`Deleted "${name}".`, "ok");
      this.sync();
    } catch (error) {
      this.setStatus(messageOf(error), "error");
    } finally {
      this.setBusy(false);
    }
  }

  private renderOptions(): void {
    if (!this.rendered) return;
    const previous = this.select.value;
    this.select.replaceChildren();
    if (this.summaries.length === 0) {
      const option = document.createElement("option");
      option.value = "";
      option.textContent = "No presets";
      this.select.append(option);
      return;
    }
    for (const summary of this.summaries) {
      const option = document.createElement("option");
      option.value = summary.name;
      option.textContent = `${summary.name} (${summary.controls})`;
      this.select.append(option);
    }
    if (this.summaries.some((summary) => summary.name === previous)) {
      this.select.value = previous;
    }
  }

  private sync(): void {
    if (!this.rendered) return;
    const disabled = this.disabled || this.busy;
    const hasSelection = this.select.value !== "";
    this.nameInput.disabled = disabled;
    this.select.disabled = disabled;
    this.saveButton.disabled = disabled;
    this.applyButton.disabled = disabled || !hasSelection;
    this.deleteButton.disabled = disabled || !hasSelection;
  }

  private setBusy(value: boolean): void {
    this.busy = value;
    this.sync();
  }

  private setStatus(message: string, kind: "idle" | "ok" | "error"): void {
    if (!this.rendered) return;
    this.statusEl.textContent = message;
    this.statusEl.dataset.kind = kind;
  }
}

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

defineOnce("wd-preset-manager", WdPresetManager);
