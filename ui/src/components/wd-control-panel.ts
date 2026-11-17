import { attachStyles, baseCss, defineOnce, emit, valuesEqual } from "./base";
import type {
  AutoDescriptor,
  ControlChange,
  ControlDescriptor,
  ControlEntry,
  ControlState,
  LockDescriptor,
  ReadoutDescriptor,
  ResetDescriptor,
  SelectDescriptor,
  SliderDescriptor,
  ToggleDescriptor,
  TriggerDescriptor,
} from "../types";
import type { WdSlider } from "./wd-slider";
import type { WdActual } from "./wd-actual";
import type { WdAutoToggle } from "./wd-auto-toggle";
import type { WdLockButton } from "./wd-lock-button";
import type { WdSelect } from "./wd-select";
import type { WdReset } from "./wd-reset";
import type { WdToggle } from "./wd-toggle";
import type { WdTrigger } from "./wd-trigger";

import "./wd-slider";
import "./wd-actual";
import "./wd-auto-toggle";
import "./wd-lock-button";
import "./wd-select";
import "./wd-reset";
import "./wd-toggle";
import "./wd-trigger";

const css = `${baseCss}
  .panel {
    display: grid;
    gap: 0.85rem;
    padding: 0.9rem;
    border: 1px solid var(--wd-border);
    border-radius: 0.85rem;
    background: var(--wd-panel);
    box-shadow: var(--wd-shadow);
  }
  header {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.3rem 0.6rem;
  }
  h2 {
    margin: 0;
    font-size: 1rem;
  }
  .subtitle {
    color: var(--wd-muted);
    font-size: 0.72rem;
    letter-spacing: 0.03em;
  }
  .groups {
    display: grid;
    gap: 0.7rem;
  }
  .group {
    display: grid;
    gap: 0.45rem;
    padding-top: 0.7rem;
    border-top: 1px solid var(--wd-border);
  }
  .group:first-child {
    padding-top: 0;
    border-top: 0;
  }
  .group-head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 0.35rem 0.6rem;
  }
  h3 {
    margin: 0;
    color: var(--wd-muted);
    font-size: 0.72rem;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }
  .rows {
    display: grid;
    gap: 0.5rem;
  }
  .row {
    display: grid;
    gap: 0.15rem;
    padding: 0.4rem 0.5rem;
    border: 1px solid transparent;
    border-radius: 0.55rem;
  }
  .row:hover {
    border-color: var(--wd-border);
    background: var(--wd-panel-2);
  }
  .row.compact {
    padding: 0.15rem 0.1rem;
  }
  .row.compact:hover {
    border-color: transparent;
    background: transparent;
  }
  .help {
    margin: 0;
    color: var(--wd-muted);
    font-size: 0.7rem;
  }
  .empty {
    margin: 0;
    color: var(--wd-muted);
  }
`;

function parseSize(value: string): { width: number; height: number } | null {
  const match = /^(\d+)x(\d+)$/.exec(value.trim());
  if (!match) return null;
  const width = Number(match[1]);
  const height = Number(match[2]);
  return width > 0 && height > 0 ? { width, height } : null;
}

function isSize(value: unknown): value is { width: number; height: number } {
  if (!value || typeof value !== "object") return false;
  const record = value as Record<string, unknown>;
  return Number.isFinite(record["width"]) && Number.isFinite(record["height"]);
}

function parseFpsRange(value: string): [number, number] | null {
  const match = /^(\d+)-(\d+)$/.exec(value.trim());
  if (!match) return null;
  const lower = Number(match[1]);
  const upper = Number(match[2]);
  return lower > 0 && upper >= lower ? [lower, upper] : null;
}

function isFpsRange(value: unknown): value is [number, number] {
  return (
    Array.isArray(value) &&
    value.length === 2 &&
    typeof value[0] === "number" &&
    typeof value[1] === "number"
  );
}

interface SliderRow {
  kind: "slider";
  host: WdSlider;
  actual: WdActual;
  desc: SliderDescriptor;
}

interface SelectRow {
  kind: "select";
  host: WdSelect;
  actual: WdActual;
  desc: SelectDescriptor;
}

interface AutoRow {
  kind: "auto";
  host: WdAutoToggle;
  desc: AutoDescriptor;
}

interface LockRow {
  kind: "lock";
  host: WdLockButton;
  desc: LockDescriptor;
}

interface ReadoutRow {
  kind: "readout";
  host: WdActual;
  desc: ReadoutDescriptor;
}

interface ToggleRow {
  kind: "toggle";
  host: WdToggle;
  desc: ToggleDescriptor;
}

interface TriggerRow {
  kind: "trigger";
  host: WdTrigger;
  desc: TriggerDescriptor;
}

type Row = SliderRow | SelectRow | AutoRow | LockRow | ReadoutRow | ToggleRow | TriggerRow;

export class WdControlPanel extends HTMLElement {
  static observedAttributes = ["heading", "disabled"];

  private _descriptors: ControlDescriptor[] = [];
  private _state: ControlState = {};
  private _groups: string[] | null = null;
  private rendered = false;
  private rows = new Map<string, Row>();
  private resets = new Map<string, { host: WdReset; desc: ResetDescriptor }>();

  private titleEl!: HTMLHeadingElement;
  private groupsEl!: HTMLDivElement;

  get heading(): string {
    return this.getAttribute("heading") ?? "Camera controls";
  }

  set heading(value: string) {
    this.setAttribute("heading", value);
  }

  get disabled(): boolean {
    return this.hasAttribute("disabled");
  }

  set disabled(value: boolean) {
    this.toggleAttribute("disabled", value);
  }

  get descriptors(): ControlDescriptor[] {
    return this._descriptors;
  }

  set descriptors(value: ControlDescriptor[]) {
    this._descriptors = value;
    if (this.rendered) this.render();
  }

  /** Only render these groups; `null` (the default) renders every group. */
  get groups(): string[] | null {
    return this._groups;
  }

  set groups(value: string[] | null) {
    this._groups = value;
    if (this.rendered) this.render();
  }

  get state(): ControlState {
    return this._state;
  }

  set state(value: ControlState) {
    this._state = value;
    this.sync();
  }

  connectedCallback(): void {
    let root = this.shadowRoot;
    if (!root) {
      root = this.attachShadow({ mode: "open" });
      attachStyles(root, css);
      root.innerHTML = `
        <section class="panel" part="panel">
          <header>
            <h2></h2>
            <span class="subtitle">actual vs requested</span>
          </header>
          <div class="groups"></div>
        </section>
      `;
      this.titleEl = root.querySelector("h2") as HTMLHeadingElement;
      this.groupsEl = root.querySelector(".groups") as HTMLDivElement;
      this.rendered = true;
      root.addEventListener("wd-commit", this.onCommit);
      root.addEventListener("wd-auto-change", this.onAutoChange);
      root.addEventListener("wd-lock-change", this.onLockChange);
      root.addEventListener("wd-toggle-change", this.onToggleChange);
      root.addEventListener("wd-trigger", this.onTrigger);
      root.addEventListener("wd-reset", this.onReset);
      this.render();
    } else {
      this.sync();
    }
  }

  attributeChangedCallback(): void {
    if (!this.rendered) return;
    this.titleEl.textContent = this.heading;
    this.sync();
  }

  private render(): void {
    if (!this.rendered) return;
    this.rows.clear();
    this.resets.clear();
    this.groupsEl.replaceChildren();
    this.titleEl.textContent = this.heading;

    if (this._descriptors.length === 0) {
      const empty = document.createElement("p");
      empty.className = "empty";
      empty.textContent = "No controls configured.";
      this.groupsEl.append(empty);
      return;
    }

    const groups = new Map<string, ControlDescriptor[]>();
    for (const descriptor of this._descriptors) {
      if (this._groups && !this._groups.includes(descriptor.group)) continue;
      const list = groups.get(descriptor.group) ?? [];
      list.push(descriptor);
      groups.set(descriptor.group, list);
    }

    if (groups.size === 0) {
      const empty = document.createElement("p");
      empty.className = "empty";
      empty.textContent = "No controls in this panel.";
      this.groupsEl.append(empty);
      return;
    }

    for (const [group, descriptors] of groups) {
      const section = document.createElement("section");
      section.className = "group";
      section.dataset.group = group;

      const head = document.createElement("div");
      head.className = "group-head";
      const heading = document.createElement("h3");
      heading.textContent = group;
      head.append(heading);

      for (const descriptor of descriptors) {
        if (descriptor.kind !== "reset") continue;
        const host = document.createElement("wd-reset") as WdReset;
        host.label = descriptor.label;
        host.targets = descriptor.targets;
        host.dataset.resetGroup = group;
        host.dataset.resetId = descriptor.id;
        head.append(host);
        this.resets.set(descriptor.id, { host, desc: descriptor });
      }
      section.append(head);

      const rows = document.createElement("div");
      rows.className = "rows";
      for (const descriptor of descriptors) {
        if (descriptor.kind === "reset") continue;
        const { element, row } = this.createRow(descriptor);
        rows.append(element);
        this.rows.set(descriptor.id, row);
      }
      section.append(rows);
      this.groupsEl.append(section);
    }

    this.sync();
  }

  private createRow(descriptor: Exclude<ControlDescriptor, ResetDescriptor>): {
    element: HTMLElement;
    row: Row;
  } {
    const wrapper = document.createElement("div");
    wrapper.className = "row";
    wrapper.dataset.controlId = descriptor.id;
    wrapper.dataset.kind = descriptor.kind;

    if (descriptor.kind === "slider") {
      const scale = descriptor.scale ?? 1;
      const host = document.createElement("wd-slider") as WdSlider;
      host.controlId = descriptor.id;
      host.dataset.controlId = descriptor.id;
      host.label = descriptor.label;
      host.min = descriptor.min / scale;
      host.max = descriptor.max / scale;
      host.step = descriptor.step / scale;
      host.unit = descriptor.unit ?? "";
      if (descriptor.precision !== undefined) host.precision = descriptor.precision;

      const actual = document.createElement("wd-actual") as WdActual;
      actual.controlId = descriptor.id;
      actual.dataset.controlId = descriptor.id;
      actual.label = descriptor.label;
      actual.unit = descriptor.unit ?? "";
      if (descriptor.precision !== undefined) actual.precision = descriptor.precision;

      wrapper.append(host, actual);
      if (descriptor.help) wrapper.append(this.help(descriptor.help));
      return {
        element: wrapper,
        row: { kind: "slider", host, actual, desc: descriptor },
      };
    }

    if (descriptor.kind === "select") {
      const host = document.createElement("wd-select") as WdSelect;
      host.controlId = descriptor.id;
      host.dataset.controlId = descriptor.id;
      host.label = descriptor.label;
      host.options = descriptor.options;

      const actual = document.createElement("wd-actual") as WdActual;
      actual.controlId = descriptor.id;
      actual.dataset.controlId = descriptor.id;
      actual.label = descriptor.label;

      wrapper.append(host, actual);
      if (descriptor.help) wrapper.append(this.help(descriptor.help));
      return {
        element: wrapper,
        row: { kind: "select", host, actual, desc: descriptor },
      };
    }

    if (descriptor.kind === "auto") {
      wrapper.classList.add("compact");
      const host = document.createElement("wd-auto-toggle") as WdAutoToggle;
      host.controlId = descriptor.id;
      host.dataset.controlId = descriptor.id;
      host.label = descriptor.label;
      wrapper.append(host);
      if (descriptor.help) wrapper.append(this.help(descriptor.help));
      return {
        element: wrapper,
        row: { kind: "auto", host, desc: descriptor },
      };
    }

    if (descriptor.kind === "readout") {
      const actual = document.createElement("wd-actual") as WdActual;
      actual.controlId = descriptor.id;
      actual.dataset.controlId = descriptor.id;
      actual.label = descriptor.label;
      actual.readonly = true;
      actual.unit = descriptor.unit ?? "";
      if (descriptor.precision !== undefined) actual.precision = descriptor.precision;
      if (descriptor.options) actual.options = descriptor.options;
      wrapper.append(actual);
      if (descriptor.help) wrapper.append(this.help(descriptor.help));
      return {
        element: wrapper,
        row: { kind: "readout", host: actual, desc: descriptor },
      };
    }

    if (descriptor.kind === "toggle") {
      wrapper.classList.add("compact");
      const host = document.createElement("wd-toggle") as WdToggle;
      host.controlId = descriptor.id;
      host.dataset.controlId = descriptor.id;
      host.label = descriptor.label;
      wrapper.append(host);
      if (descriptor.help) wrapper.append(this.help(descriptor.help));
      return {
        element: wrapper,
        row: { kind: "toggle", host, desc: descriptor },
      };
    }

    if (descriptor.kind === "trigger") {
      wrapper.classList.add("compact");
      const host = document.createElement("wd-trigger") as WdTrigger;
      host.controlId = descriptor.id;
      host.dataset.controlId = descriptor.id;
      host.label = descriptor.label;
      host.action = descriptor.actionLabel ?? "Run";
      wrapper.append(host);
      if (descriptor.help) wrapper.append(this.help(descriptor.help));
      return {
        element: wrapper,
        row: { kind: "trigger", host, desc: descriptor },
      };
    }

    wrapper.classList.add("compact");
    const host = document.createElement("wd-lock-button") as WdLockButton;
    host.controlId = descriptor.id;
    host.dataset.controlId = descriptor.id;
    host.label = descriptor.label;
    wrapper.append(host);
    if (descriptor.help) wrapper.append(this.help(descriptor.help));
    return {
      element: wrapper,
      row: { kind: "lock", host, desc: descriptor },
    };
  }

  private help(text: string): HTMLParagraphElement {
    const element = document.createElement("p");
    element.className = "help";
    element.textContent = text;
    return element;
  }

  private sync(): void {
    if (!this.rendered) return;
    this.titleEl.textContent = this.heading;

    for (const [id, row] of this.rows) {
      const entry: ControlEntry = this._state[id] ?? { supported: false };
      if (row.kind === "slider") {
        const scale = row.desc.scale ?? 1;
        const requested =
          typeof entry.requested === "number"
            ? entry.requested / scale
            : undefined;
        const actual =
          typeof entry.actual === "number" ? entry.actual / scale : null;
        const autoDisabled =
          row.desc.disabledWhenAuto === true &&
          row.desc.autoId !== undefined &&
          this.isAuto(row.desc.autoId);

        row.host.value = requested ?? actual ?? row.desc.min / scale;
        row.host.actual = actual;
        row.host.disabled =
          this.disabled || !entry.supported || autoDisabled;
        row.actual.requested = requested;
        row.actual.actual = actual;
        row.actual.source = entry.source ?? "";
        row.actual.supported = entry.supported;
      } else if (row.kind === "select") {
        row.host.value = this.selectValue(row.desc, entry.requested ?? entry.actual);
        row.host.disabled = this.disabled || !entry.supported;
        row.actual.requested = entry.requested;
        row.actual.actual = entry.actual;
        row.actual.source = entry.source ?? "";
        row.actual.supported = entry.supported;
      } else if (row.kind === "auto") {
        const value = entry.actual ?? entry.requested;
        row.host.auto = value === row.desc.autoValue;
        row.host.source = entry.source ?? "";
        row.host.disabled = this.disabled || !entry.supported;
      } else if (row.kind === "readout") {
        row.host.requested = undefined;
        row.host.actual = entry.actual;
        row.host.source = entry.source ?? "";
        row.host.supported = entry.supported;
      } else if (row.kind === "toggle") {
        row.host.on = (entry.actual ?? entry.requested) === true;
        row.host.pending =
          entry.requested !== undefined &&
          entry.actual !== undefined &&
          !valuesEqual(entry.requested, entry.actual);
        row.host.supported = entry.supported;
        row.host.disabled = this.disabled;
      } else if (row.kind === "trigger") {
        row.host.pending =
          entry.requested !== undefined &&
          entry.actual !== undefined &&
          !valuesEqual(entry.requested, entry.actual);
        row.host.supported = entry.supported;
        row.host.disabled = this.disabled;
      } else {
        row.host.locked = entry.actual === true;
        row.host.pending =
          entry.requested !== undefined &&
          entry.actual !== undefined &&
          !valuesEqual(entry.requested, entry.actual);
        row.host.supported = entry.supported;
        row.host.disabled = this.disabled;
      }
    }

    for (const { host } of this.resets.values()) {
      host.disabled = this.disabled;
    }
  }

  private selectValue(descriptor: SelectDescriptor, value: unknown): unknown {
    if (descriptor.format === "size" && isSize(value)) {
      return `${value.width}x${value.height}`;
    }
    if (descriptor.format === "fps-range" && isFpsRange(value)) {
      return `${value[0]}-${value[1]}`;
    }
    if (
      typeof value === "number" &&
      descriptor.options.every((option) => typeof option.value === "string")
    ) {
      return String(value);
    }
    return value;
  }

  private isAuto(id: string): boolean {
    const descriptor = this._descriptors.find(
      (item): item is AutoDescriptor => item.kind === "auto" && item.id === id,
    );
    if (!descriptor) return false;
    const entry = this._state[id];
    if (!entry) return false;
    return (entry.actual ?? entry.requested) === descriptor.autoValue;
  }

  private readonly onCommit = (event: Event): void => {
    const detail = (event as CustomEvent<ControlChange>).detail;
    if (!detail) return;
    const descriptor = this._descriptors.find((item) => item.id === detail.id);
    let value = detail.value;
    if (
      descriptor !== undefined &&
      descriptor.kind === "slider" &&
      typeof value === "number" &&
      descriptor.scale !== undefined
    ) {
      value *= descriptor.scale;
    }
    if (
      descriptor !== undefined &&
      descriptor.kind === "select" &&
      descriptor.format === "size" &&
      typeof value === "string"
    ) {
      const size = parseSize(value);
      if (size) value = size;
    }
    if (
      descriptor !== undefined &&
      descriptor.kind === "select" &&
      descriptor.format === "fps-range" &&
      typeof value === "string"
    ) {
      const range = parseFpsRange(value);
      if (range) value = range;
    }
    emit(this, "wd-control-change", { id: detail.id, value });
  };

  private readonly onAutoChange = (event: Event): void => {
    const detail = (
      event as CustomEvent<{ id: string; auto: boolean }>
    ).detail;
    if (!detail) return;
    const descriptor = this._descriptors.find(
      (item): item is AutoDescriptor =>
        item.kind === "auto" && item.id === detail.id,
    );
    if (!descriptor) return;
    emit(this, "wd-control-change", {
      id: detail.id,
      value: detail.auto ? descriptor.autoValue : descriptor.manualValue,
    });
  };

  private readonly onLockChange = (event: Event): void => {
    const detail = (
      event as CustomEvent<{ id: string; locked: boolean }>
    ).detail;
    if (!detail) return;
    emit(this, "wd-control-change", {
      id: detail.id,
      value: detail.locked,
    });
  };

  private readonly onToggleChange = (event: Event): void => {
    const detail = (
      event as CustomEvent<{ id: string; on: boolean }>
    ).detail;
    if (!detail) return;
    emit(this, "wd-control-change", { id: detail.id, value: detail.on });
  };

  private readonly onTrigger = (event: Event): void => {
    const detail = (event as CustomEvent<{ id: string }>).detail;
    if (!detail) return;
    const descriptor = this._descriptors.find(
      (item): item is TriggerDescriptor =>
        item.kind === "trigger" && item.id === detail.id,
    );
    if (!descriptor) return;
    emit(this, "wd-control-change", {
      id: detail.id,
      value: descriptor.value,
    });
  };

  private readonly onReset = (event: Event): void => {
    const detail = (event as CustomEvent<{ targets: string[] }>).detail;
    if (!detail) return;
    emit(this, "wd-control-reset", { targets: [...detail.targets] });
  };
}

defineOnce("wd-control-panel", WdControlPanel);
