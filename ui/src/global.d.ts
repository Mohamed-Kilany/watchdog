import type { WdControlPanel } from "./components/wd-control-panel";
import type { LivePage } from "./live";
import type { CommandLogEntry, ControlState, DeviceBackend } from "./types";

declare global {
  interface Window {
    __watchdog: {
      device: DeviceBackend;
      panel: WdControlPanel;
      panels: WdControlPanel[];
      live: LivePage;
      state: () => ControlState;
      commands: () => CommandLogEntry[];
    };
  }
}

export {};
