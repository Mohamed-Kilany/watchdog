import type { FakeDevice } from "./fake-device";
import type { WdControlPanel } from "./components/wd-control-panel";
import type { CommandLogEntry, ControlState } from "./types";

declare global {
  interface Window {
    __watchdog: {
      device: FakeDevice;
      panel: WdControlPanel;
      state: () => ControlState;
      commands: () => CommandLogEntry[];
    };
  }
}

export {};
