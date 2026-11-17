// Shared app shell: panel, overlays, command log, theme and backend boot.
//
// The shell is wired synchronously with the offline fake device so every
// control works as soon as the page loads. `boot()` then probes the same
// origin: a host state envelope swaps in `HostClient`, a 401 asks for the
// bearer token, and anything else (including the Vite preview serving its own
// index.html for `/api/state`) leaves the demo in place.

import { DESCRIPTORS } from "./descriptors";
import { CapturePage } from "./capture";
import { DevicesPage } from "./devices";
import { FakeDevice } from "./fake-device";
import { MediaPage } from "./media";
import { SettingsPage } from "./settings";
import {
  consumeTokenFromUrl,
  HostClient,
  probeHost,
  readStoredToken,
  saveToken,
} from "./host";
import type { ConnectionState } from "./host";
import type { SessionInfo } from "./device-api";
import type { WdControlPanel } from "./components/wd-control-panel";
import type { WdPresetManager } from "./components/wd-preset-manager";
import type { WdRegionOverlay } from "./components/wd-region-overlay";
import { LivePage } from "./live";
import {
  captureControls,
  HostPresetSource,
  LocalPresetSource,
  orderedChanges,
} from "./presets";
import type { PresetSource } from "./presets";
import type {
  ApplyResult,
  CommandLogEntry,
  ControlChange,
  ControlEntry,
  ControlRegion,
  ControlState,
  DeviceBackend,
} from "./types";

function mustQuery<T extends Element>(selector: string): T {
  const element = document.querySelector<T>(selector);
  if (!element) throw new Error(`missing element: ${selector}`);
  return element;
}

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function isRegion(value: unknown): value is ControlRegion {
  if (!value || typeof value !== "object") return false;
  const record = value as Record<string, unknown>;
  return ["x", "y", "w", "h"].every((key) =>
    Number.isFinite(Number(record[key])),
  );
}

/**
 * The first region in an actual value, or null.
 *
 * The app accepts region arrays; the host's clear path is a full-frame region
 * with weight 0, which means "no metering" and is treated as cleared here.
 */
function regionOf(entry: ControlEntry | undefined): ControlRegion | null {
  const actual = entry?.actual;
  const candidate = Array.isArray(actual) ? actual[0] : actual;
  if (!isRegion(candidate)) return null;
  if (candidate.weight === 0) return null;
  return candidate;
}

function describe(region: ControlRegion): string {
  const pct = (value: number): string => `${Math.round(value * 1000) / 10}%`;
  return `x ${pct(region.x)} y ${pct(region.y)} w ${pct(region.w)} h ${pct(region.h)}`;
}

interface Shell {
  panel: WdControlPanel;
  panels: WdControlPanel[];
  presets: WdPresetManager;
  commands: CommandLogEntry[];
  current(): DeviceBackend;
  log(changes: ControlChange[], results: ApplyResult[]): void;
  logEvent(kind: string, detail?: string): void;
  switchTo(backend: DeviceBackend): void;
  setPresetSource(source: PresetSource): void;
}

function wireShell(
  initial: DeviceBackend,
  onState: (state: ControlState) => void,
  onBackend?: (backend: DeviceBackend) => void,
): Shell {
  const panel = mustQuery<WdControlPanel>("#panel-left");
  const rightPanel = mustQuery<WdControlPanel>("#panel-right");
  const panels = [panel, rightPanel];
  const meteringOverlay = mustQuery<WdRegionOverlay>("#metering-overlay");
  const focusOverlay = mustQuery<WdRegionOverlay>("#focus-overlay");
  const awbOverlay = mustQuery<WdRegionOverlay>("#awb-overlay");
  const presetManager = mustQuery<WdPresetManager>("#preset-manager");
  const regionReadout = mustQuery<HTMLParagraphElement>("#region-readout");
  const commandLog = mustQuery<HTMLOListElement>("#command-log");
  const status = mustQuery<HTMLSpanElement>("#device-status");
  const modeTag = mustQuery<HTMLSpanElement>("#mode-tag");
  const driftButton = document.querySelector<HTMLButtonElement>("#drift");

  const commands: CommandLogEntry[] = [];
  let backend = initial;
  let label = backend.mode === "demo" ? "fake device" : "host";

  panel.heading = "Exposure & focus";
  rightPanel.heading = "Lens & output";
  for (const target of panels) target.descriptors = DESCRIPTORS;
  panel.groups = ["Exposure", "Focus", "White balance"];
  rightPanel.groups = ["Lens", "Flash & scene", "Output", "Recording"];
  for (const target of panels) target.state = backend.snapshot;
  presetManager.source = new LocalPresetSource();
  presetManager.controls = captureControls(backend.snapshot);

  function syncRegions(state: ControlState): void {
    const metering = regionOf(state["ae_regions"]);
    const focus = regionOf(state["af_regions"]);
    const awb = regionOf(state["awb_regions"]);
    meteringOverlay.regions = metering ? [metering] : [];
    focusOverlay.regions = focus ? [focus] : [];
    awbOverlay.regions = awb ? [awb] : [];
    const parts: string[] = [];
    if (metering) parts.push(`metering ${describe(metering)}`);
    if (focus) parts.push(`focus ${describe(focus)}`);
    if (awb) parts.push(`white balance ${describe(awb)}`);
    regionReadout.textContent =
      parts.length > 0 ? parts.join(" · ") : "No regions set";
  }

  function renderLog(): void {
    commandLog.replaceChildren();
    for (const entry of commands.slice(0, 30)) {
      const item = document.createElement("li");
      const code = document.createElement("code");
      const ok = entry.results.every((result) => result.ok);
      const batch =
        entry.note ??
        (entry.batch.length > 0
          ? entry.batch
              .map((change) => `${change.id}=${JSON.stringify(change.value)}`)
              .join(", ")
          : "state update");
      code.textContent = `${entry.at.slice(11, 19)} ${ok ? "ok" : "FAILED"} · ${batch}`;
      item.append(code);
      commandLog.append(item);
    }
    status.textContent = `${label} · ${commands.length} command${
      commands.length === 1 ? "" : "s"
    }`;
  }

  function updateMode(): void {
    label = backend.mode === "demo" ? "fake device" : "host";
    modeTag.textContent = backend.mode === "demo" ? "demo device" : "host";
    if (driftButton) driftButton.hidden = backend.mode !== "demo";
  }

  function handleState(state: ControlState): void {
    for (const target of panels) target.state = state;
    presetManager.controls = captureControls(state);
    syncRegions(state);
    onState(state);
  }

  let unsubscribe = backend.subscribe(handleState);

  function log(changes: ControlChange[], results: ApplyResult[]): void {
    commands.unshift({
      batch: structuredClone(changes),
      results: structuredClone(results),
      at: new Date().toISOString(),
    });
    if (commands.length > 100) commands.length = 100;
    renderLog();
  }

  function logEvent(kind: string, detail?: string): void {
    commands.unshift({
      batch: [],
      results: [],
      at: new Date().toISOString(),
      note: detail ? `event ${kind}: ${detail}` : `event ${kind}`,
    });
    if (commands.length > 100) commands.length = 100;
    renderLog();
  }

  async function applyChanges(
    changes: ControlChange[],
    marker?: ControlChange[],
  ): Promise<void> {
    try {
      const results = await backend.setControls(changes);
      log(marker ?? changes, results);
    } catch (error) {
      log(
        marker ?? changes,
        changes.map((change) => ({
          control: change.id,
          ok: false,
          error: messageOf(error),
        })),
      );
    }
  }

  async function reset(targets: string[], marker: ControlChange): Promise<void> {
    try {
      log([marker], await backend.reset(targets));
    } catch (error) {
      log([marker], [{ control: marker.id, ok: false, error: messageOf(error) }]);
    }
  }

  async function clearRegions(): Promise<void> {
    const marker: ControlChange = { id: "clear_regions", value: true };
    try {
      log([marker], await backend.clearRegions());
    } catch (error) {
      log([marker], [
        { control: "clear_regions", ok: false, error: messageOf(error) },
      ]);
    }
  }

  for (const target of panels) {
    target.addEventListener("wd-control-change", (event) => {
      const detail = (event as CustomEvent<ControlChange>).detail;
      void applyChanges([detail]);
    });

    target.addEventListener("wd-control-reset", (event) => {
      const detail = (event as CustomEvent<{ targets: string[] }>).detail;
      if (!detail) return;
      void reset(detail.targets, { id: "reset", value: detail.targets });
    });
  }

  for (const overlay of [meteringOverlay, focusOverlay, awbOverlay]) {
    overlay.addEventListener("wd-region-change", (event) => {
      const detail = (
        event as CustomEvent<{
          mode: "metering" | "focus" | "awb";
          region: ControlRegion;
        }>
      ).detail;
      if (!detail) return;
      const changes: ControlChange[] =
        detail.mode === "focus"
          ? [
              { id: "af_regions", value: detail.region },
              { id: "af_trigger", value: "start" },
            ]
          : detail.mode === "awb"
            ? [{ id: "awb_regions", value: detail.region }]
            : [{ id: "ae_regions", value: detail.region }];
      void applyChanges(changes);
    });
  }

  const radios = Array.from(
    document.querySelectorAll<HTMLInputElement>('input[name="region-mode"]'),
  );

  function setActiveRegion(mode: string): void {
    meteringOverlay.editable = mode === "metering";
    focusOverlay.editable = mode === "focus";
    awbOverlay.editable = mode === "awb";
  }

  for (const radio of radios) {
    radio.addEventListener("change", () => {
      if (radio.checked) setActiveRegion(radio.value);
    });
  }
  setActiveRegion("metering");

  document.querySelector("#clear-regions")?.addEventListener("click", () => {
    void clearRegions();
  });

  driftButton?.addEventListener("click", () => {
    if (!(backend instanceof FakeDevice)) return;
    backend.drift();
    log([{ id: "simulate_drift", value: true }], [
      { control: "simulate_drift", ok: true },
    ]);
  });

  document.querySelector("#reset-all")?.addEventListener("click", () => {
    void reset(["all"], { id: "reset_all", value: true });
  });

  presetManager.addEventListener("wd-preset-apply", (event) => {
    const detail = (
      event as CustomEvent<{ name: string; controls: Record<string, unknown> }>
    ).detail;
    if (!detail) return;
    const changes = orderedChanges(detail.controls);
    const marker: ControlChange[] = [
      { id: "preset", value: detail.name },
      ...changes,
    ];
    void (async () => {
      try {
        const results = await backend.setControls(changes);
        log(marker, results);
        const failed = results.filter((result) => !result.ok).length;
        presetManager.report(
          failed === 0
            ? `Applied "${detail.name}" (${results.length} controls).`
            : `Applied "${detail.name}" with ${failed} of ${results.length} controls rejected.`,
          failed === 0,
        );
      } catch (error) {
        log(marker, [
          { control: "preset", ok: false, error: messageOf(error) },
        ]);
        presetManager.report(messageOf(error), false);
      }
    })();
  });

  function setPresetSource(source: PresetSource): void {
    presetManager.source = source;
  }

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

  function switchTo(next: DeviceBackend): void {
    unsubscribe();
    backend = next;
    unsubscribe = backend.subscribe(handleState);
    for (const target of panels) target.state = backend.snapshot;
    presetManager.controls = captureControls(backend.snapshot);
    syncRegions(backend.snapshot);
    onState(backend.snapshot);
    updateMode();
    renderLog();
    if (window.__watchdog) window.__watchdog.device = backend;
    onBackend?.(backend);
  }

  syncRegions(backend.snapshot);
  updateMode();
  renderLog();

  return {
    panel,
    panels,
    presets: presetManager,
    commands,
    current: () => backend,
    log,
    logEvent,
    switchTo,
    setPresetSource,
  };
}

function expose(
  shell: Shell,
  live: LivePage,
  capture: CapturePage,
  media: MediaPage,
  settings: SettingsPage,
): void {
  window.__watchdog = {
    device: shell.current(),
    panel: shell.panel,
    panels: shell.panels,
    presets: shell.presets,
    live,
    capture,
    media,
    settings,
    state: () => shell.current().snapshot,
    commands: () => structuredClone(shell.commands),
  };
}

function startHost(
  shell: Shell,
  token: string | null,
  initialState: ControlState,
  initialSession: SessionInfo,
): HostClient {
  const backend = new HostClient(token, initialState, initialSession);
  shell.switchTo(backend);
  shell.setPresetSource(new HostPresetSource(token));
  const banner = mustQuery<HTMLDivElement>("#connection-banner");

  backend.subscribeEvents((event) => {
    shell.logEvent(event.kind, event.message);
  });

  let connection: ConnectionState = "connecting";
  let session = initialSession;
  const updateBanner = (): void => {
    if (connection === "live" && session.active) {
      banner.hidden = true;
      banner.textContent = "";
      return;
    }
    banner.hidden = false;
    if (connection === "connecting") {
      banner.textContent = "Connecting to the host…";
    } else if (connection === "reconnecting") {
      banner.textContent = "Connection lost — reconnecting…";
    } else if (session.closed) {
      banner.textContent =
        "The device session closed — open Devices to activate it again.";
    } else {
      banner.textContent =
        "No device session — open Devices to pair, provision or activate a device.";
    }
  };
  backend.subscribeConnection((state) => {
    connection = state;
    updateBanner();
  });
  backend.subscribeSession((next) => {
    session = next;
    updateBanner();
  });

  backend.start();
  return backend;
}

function promptForToken(message: string): Promise<string | null> {
  const dialog = mustQuery<HTMLDialogElement>("#token-dialog");
  const form = mustQuery<HTMLFormElement>("#token-form");
  const input = mustQuery<HTMLInputElement>("#token-input");
  const error = mustQuery<HTMLParagraphElement>("#token-error");
  const cancel = mustQuery<HTMLButtonElement>("#token-cancel");

  error.textContent = message;
  error.hidden = message.length === 0;
  input.value = "";
  if (!dialog.open) dialog.showModal();
  input.focus();

  return new Promise((resolve) => {
    const finish = (value: string | null): void => {
      form.removeEventListener("submit", onSubmit);
      cancel.removeEventListener("click", onCancel);
      dialog.removeEventListener("close", onClose);
      if (dialog.open) dialog.close();
      resolve(value);
    };
    const onSubmit = (event: SubmitEvent): void => {
      event.preventDefault();
      const value = input.value.trim();
      if (!value) {
        error.textContent = "Enter the bearer token from the host console.";
        error.hidden = false;
        input.focus();
        return;
      }
      finish(value);
    };
    const onCancel = (): void => finish(null);
    const onClose = (): void => finish(null);
    form.addEventListener("submit", onSubmit);
    cancel.addEventListener("click", onCancel);
    dialog.addEventListener("close", onClose);
  });
}

/** Probe the origin once, then swap in the host channel or keep the demo. */
async function boot(
  shell: Shell,
  devices: DevicesPage,
  live: LivePage,
): Promise<void> {
  const tokenFromUrl = consumeTokenFromUrl();
  let token = tokenFromUrl ?? readStoredToken();
  let result = await probeHost(token);
  let rejected = false;

  while (result.kind === "unauthorized") {
    const entered = await promptForToken(
      rejected
        ? "That token was rejected. Check the host output and try again."
        : "This host has a bearer token enabled. Paste the token printed by `watchdog serve`.",
    );
    if (entered === null) {
      devices.demo();
      live.demo();
      return;
    }
    token = entered;
    saveToken(token);
    rejected = true;
    result = await probeHost(token);
  }

  if (result.kind === "host") {
    const host = startHost(shell, token, result.state, result.session);
    devices.connect(token, host);
    live.connectHost(token, host);
  } else {
    devices.demo();
    live.demo();
  }
}

/** Switch between `#/live`, `#/capture`, `#/media`, `#/settings` and `#/devices` without a reload. */
function wireRoutes(
  devices: DevicesPage,
  live: LivePage,
  capture: CapturePage,
  media: MediaPage,
  settings: SettingsPage,
): void {
  const liveView = mustQuery<HTMLElement>("#view-live");
  const captureView = mustQuery<HTMLElement>("#view-capture");
  const mediaView = mustQuery<HTMLElement>("#view-media");
  const settingsView = mustQuery<HTMLElement>("#view-settings");
  const devicesView = mustQuery<HTMLElement>("#view-devices");
  const links = Array.from(
    document.querySelectorAll<HTMLAnchorElement>("a[data-route]"),
  );

  const apply = (): void => {
    const hash = window.location.hash;
    const route =
      hash === "#/devices"
        ? "devices"
        : hash === "#/capture"
          ? "capture"
          : hash === "#/media"
            ? "media"
            : hash === "#/settings"
              ? "settings"
              : "live";
    liveView.hidden = route !== "live";
    captureView.hidden = route !== "capture";
    mediaView.hidden = route !== "media";
    settingsView.hidden = route !== "settings";
    devicesView.hidden = route !== "devices";
    live.setVisible(route === "live");
    capture.setVisible(route === "capture");
    media.setVisible(route === "media");
    settings.setVisible(route === "settings");
    for (const link of links) {
      if (link.dataset.route === route) {
        link.setAttribute("aria-current", "page");
      } else {
        link.removeAttribute("aria-current");
      }
    }
    if (route === "devices") void devices.refresh();
  };

  window.addEventListener("hashchange", apply);
  apply();
}

/** Wire the demo immediately, then upgrade to the host if one answers. */
export function start(): void {
  const live = new LivePage();
  const capture = new CapturePage();
  const media = new MediaPage();
  const settings = new SettingsPage();
  const shell = wireShell(
    new FakeDevice(),
    (state) => {
      live.renderState(state);
      capture.renderState(state);
    },
    (backend) => {
      capture.setBackend(backend);
      media.setBackend(backend);
      settings.setBackend(backend);
    },
  );
  const devices = new DevicesPage();
  capture.setBackend(shell.current());
  media.setBackend(shell.current());
  settings.setBackend(shell.current());
  expose(shell, live, capture, media, settings);
  wireRoutes(devices, live, capture, media, settings);
  void boot(shell, devices, live);
}
