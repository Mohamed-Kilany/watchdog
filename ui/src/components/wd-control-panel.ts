import { attachStyles, baseCss, defineOnce, emit, valuesEqual } from "./base";
import type {
  AutoDescriptor,
  ControlChange,
  ControlDescriptor,
  ControlEntry,
  ControlState,
  LockDescriptor,
  ResetDescriptor,
  SelectDescriptor,
  SliderDescriptor,
} from "../types";
import type { WdSlider } from "./wd-slider";
import type { WdActual } from "./wd-actual";
import type { WdAutoToggle } from "./wd-auto-toggle";
import type { WdLockButton } from "./wd-lock-button";
import type { WdSelect } from "./wd-select";
import type { WdReset } from "./wd-reset";

import "./wd-slider";
import "./wd-actual";
import "./wd-auto-toggle";
import "./wd-lock-button";
import "./wd-select";
import "./wd-reset";

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

type Row = SliderRow | SelectRow | AutoRow | LockRow;

export class WdControlPanel extends HTMLElement {
  static observedAttributes = ["heading", "disabled"];

  private _descriptors: ControlDescriptor[] = [];
  private _state: ControlState = {};
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
      const list = groups.get(descriptor.group) ?? [];
      list.push(descriptor);
      groups.set(descriptor.group, list);
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
      const host = document.createElement("wd-slider") as WdSlider;
      host.controlId = descriptor.id;
      host.dataset.controlId = descriptor.id;
      host.label = descriptor.label;
      host.min = descriptor.min;
      host.max = descriptor.max;
      host.step = descriptor.step;
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

        row.host.value = requested ?? actual ?? row.desc.min;
        row.host.actual = actual;
        row.host.disabled =
          this.disabled || !entry.supported || autoDisabled;
        row.actual.requested = requested;
        row.actual.actual = actual;
        row.actual.source = entry.source ?? "";
        row.actual.supported = entry.supported;
      } else if (row.kind === "select") {
        row.host.value = entry.requested ?? entry.actual;
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

  private readonly onReset = (event: Event): void => {
    const detail = (event as CustomEvent<{ targets: string[] }>).detail;
    if (!detail) return;
    emit(this, "wd-control-reset", { targets: [...detail.targets] });
  };
}

defineOnce("wd-control-panel", WdControlPanel);
