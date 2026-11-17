import type { WdControlPanel } from "./components/wd-control-panel";
import type { WdPresetManager } from "./components/wd-preset-manager";
import type { CapturePage } from "./capture";
import type { LivePage } from "./live";
import type { CommandLogEntry, ControlState, DeviceBackend } from "./types";

declare global {
  interface Window {
    __watchdog: {
      device: DeviceBackend;
      panel: WdControlPanel;
      panels: WdControlPanel[];
      presets: WdPresetManager;
      live: LivePage;
      capture: CapturePage;
      state: () => ControlState;
      commands: () => CommandLogEntry[];
    };
  }
}

export {};
