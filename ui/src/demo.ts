import { DESCRIPTORS } from "./descriptors";
import { FakeDevice, type ApplyResult } from "./fake-device";
import type { WdControlPanel } from "./components/wd-control-panel";
import type { WdRegionOverlay } from "./components/wd-region-overlay";
import type {
  CommandLogEntry,
  ControlChange,
  ControlEntry,
  ControlRegion,
  ControlState,
} from "./types";

function mustQuery<T extends Element>(selector: string): T {
  const element = document.querySelector<T>(selector);
  if (!element) throw new Error(`missing element: ${selector}`);
  return element;
}

const device = new FakeDevice();
const panel = mustQuery<WdControlPanel>("wd-control-panel");
const meteringOverlay = mustQuery<WdRegionOverlay>("#metering-overlay");
const focusOverlay = mustQuery<WdRegionOverlay>("#focus-overlay");
const regionReadout = mustQuery<HTMLParagraphElement>("#region-readout");
const commandLog = mustQuery<HTMLOListElement>("#command-log");
const status = mustQuery<HTMLSpanElement>("#device-status");

const commands: CommandLogEntry[] = [];

panel.heading = "Camera controls";
panel.descriptors = DESCRIPTORS;
panel.state = device.snapshot;

function isRegion(value: unknown): value is ControlRegion {
  if (!value || typeof value !== "object") return false;
  const record = value as Record<string, unknown>;
  return ["x", "y", "w", "h"].every((key) => Number.isFinite(Number(record[key])));
}

function regionOf(entry: ControlEntry | undefined): ControlRegion | null {
  const actual = entry?.actual;
  if (Array.isArray(actual)) {
    const first: unknown = actual[0];
    return isRegion(first) ? first : null;
  }
  return isRegion(actual) ? actual : null;
}

function describe(region: ControlRegion): string {
  const pct = (value: number): string => `${Math.round(value * 1000) / 10}%`;
  return `x ${pct(region.x)} y ${pct(region.y)} w ${pct(region.w)} h ${pct(region.h)}`;
}

function syncRegions(state: ControlState): void {
  const metering = regionOf(state["ae_regions"]);
  const focus = regionOf(state["af_regions"]);
  meteringOverlay.regions = metering ? [metering] : [];
  focusOverlay.regions = focus ? [focus] : [];
  const parts: string[] = [];
  if (metering) parts.push(`metering ${describe(metering)}`);
  if (focus) parts.push(`focus ${describe(focus)}`);
  regionReadout.textContent = parts.length > 0 ? parts.join(" · ") : "No regions set";
}

function renderLog(): void {
  commandLog.replaceChildren();
  for (const entry of commands.slice(0, 30)) {
    const item = document.createElement("li");
    const code = document.createElement("code");
    const ok = entry.results.every((result) => result.ok);
    const batch =
      entry.batch.length > 0
        ? entry.batch
            .map((change) => `${change.id}=${JSON.stringify(change.value)}`)
            .join(", ")
        : "state update";
    code.textContent = `${entry.at.slice(11, 19)} ${ok ? "ok" : "FAILED"} · ${batch}`;
    item.append(code);
    commandLog.append(item);
  }
  status.textContent = `fake device · ${commands.length} command${commands.length === 1 ? "" : "s"}`;
}

function log(changes: ControlChange[], results: ApplyResult[]): void {
  commands.unshift({
    batch: structuredClone(changes),
    results: structuredClone(results),
    at: new Date().toISOString(),
  });
  renderLog();
}

panel.addEventListener("wd-control-change", (event) => {
  const detail = (event as CustomEvent<ControlChange>).detail;
  const results = device.setControls([detail]);
  log([detail], results);
});

panel.addEventListener("wd-control-reset", (event) => {
  const detail = (event as CustomEvent<{ targets: string[] }>).detail;
  const results = device.reset(detail.targets);
  log([{ id: "reset", value: detail.targets }], results);
});

for (const overlay of [meteringOverlay, focusOverlay]) {
  overlay.addEventListener("wd-region-change", (event) => {
    const detail = (
      event as CustomEvent<{
        mode: "metering" | "focus";
        region: ControlRegion;
      }>
    ).detail;
    const changes: ControlChange[] =
      detail.mode === "focus"
        ? [
            { id: "af_regions", value: detail.region },
            { id: "af_trigger", value: "start" },
          ]
        : [{ id: "ae_regions", value: detail.region }];
    const results = device.setControls(changes);
    log(changes, results);
  });
}

device.subscribe((state) => {
  panel.state = state;
  syncRegions(state);
});

const radios = Array.from(
  document.querySelectorAll<HTMLInputElement>('input[name="region-mode"]'),
);

function setActiveRegion(mode: string): void {
  meteringOverlay.editable = mode === "metering";
  focusOverlay.editable = mode === "focus";
}

for (const radio of radios) {
  radio.addEventListener("change", () => {
    if (radio.checked) setActiveRegion(radio.value);
  });
}
setActiveRegion("metering");

document.querySelector("#clear-regions")?.addEventListener("click", () => {
  const results = device.clearRegions();
  log([{ id: "clear_regions", value: true }], results);
});

document.querySelector("#drift")?.addEventListener("click", () => {
  device.drift();
  log([{ id: "simulate_drift", value: true }], [
    { control: "simulate_drift", ok: true },
  ]);
});

document.querySelector("#reset-all")?.addEventListener("click", () => {
  const results = device.reset(["all"]);
  log([{ id: "reset_all", value: true }], results);
});

const themeButton = mustQuery<HTMLButtonElement>("#theme-toggle");
const storedTheme = localStorage.getItem("watchdog.theme");
const systemTheme = window.matchMedia("(prefers-color-scheme: light)").matches
  ? "light"
  : "dark";
let theme: "dark" | "light" =
  storedTheme === "light" || storedTheme === "dark" ? storedTheme : systemTheme;

function applyTheme(): void {
  document.documentElement.dataset.theme = theme;
  themeButton.textContent = `Theme: ${theme}`;
  themeButton.setAttribute("aria-pressed", String(theme === "light"));
  localStorage.setItem("watchdog.theme", theme);
}

themeButton.addEventListener("click", () => {
  theme = theme === "dark" ? "light" : "dark";
  applyTheme();
});
applyTheme();

syncRegions(device.snapshot);
renderLog();

window.__watchdog = {
  device,
  panel,
  state: () => device.snapshot,
  commands: () => structuredClone(commands),
};
