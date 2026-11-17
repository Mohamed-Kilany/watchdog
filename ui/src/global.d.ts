import type { WdControlPanel } from "./components/wd-control-panel";
import type { CommandLogEntry, ControlState, DeviceBackend } from "./types";

declare global {
  interface Window {
    __watchdog: {
      device: DeviceBackend;
      panel: WdControlPanel;
      state: () => ControlState;
      commands: () => CommandLogEntry[];
    };
  }
}

export {};
