// Devices page: pair, provision, activate and restore cameras (M6-T03).
//
// The page talks to the host over `/api/devices*`, polls background
// provisioning jobs, and hands session changes to the live channel so the
// controls page follows without a reload. Without a host it renders a demo
// notice and leaves the controls demo untouched.

import { DeviceApi, DeviceApiError } from "./device-api";
import type {
  DeviceEntry,
  DeviceList,
  ProvisionJob,
  SessionInfo,
  StatusReport,
} from "./device-api";
import type { HostClient } from "./host";

function mustQuery<T extends Element>(selector: string): T {
  const element = document.querySelector<T>(selector);
  if (!element) throw new Error(`missing element: ${selector}`);
  return element;
}

function messageOf(error: unknown): string {
  if (error instanceof DeviceApiError && error.hint) {
    return `${error.message} (${error.hint})`;
  }
  return error instanceof Error ? error.message : String(error);
}

function deviceLabel(device: DeviceEntry): string {
  return device.model ?? device.product ?? device.guid ?? device.serial;
}

function sameSession(a: SessionInfo, b: SessionInfo): boolean {
  return (
    a.active === b.active &&
    a.closed === b.closed &&
    a.serial === b.serial &&
    a.device?.app_version_code === b.device?.app_version_code
  );
}

/** Host-backed devices page controller. */
export class DevicesPage {
  private readonly select = mustQuery<HTMLSelectElement>("#device-select");
  private readonly notice = mustQuery<HTMLParagraphElement>("#devices-notice");
  private readonly scanButton =
    mustQuery<HTMLButtonElement>("#scan-devices");
  private readonly pairButton = mustQuery<HTMLButtonElement>("#pair-open");
  private readonly pairDialog = mustQuery<HTMLDialogElement>("#pair-dialog");
  private readonly pairForm = mustQuery<HTMLFormElement>("#pair-form");
  private readonly pairTarget = mustQuery<HTMLInputElement>("#pair-target");
  private readonly pairCode = mustQuery<HTMLInputElement>("#pair-code");
  private readonly pairError = mustQuery<HTMLParagraphElement>("#pair-error");
  private readonly pairCancel = mustQuery<HTMLButtonElement>("#pair-cancel");
  private readonly activeCard =
    mustQuery<HTMLElement>("#active-device");
  private readonly activeModel = mustQuery<HTMLElement>("#active-model");
  private readonly activeSerial = mustQuery<HTMLElement>("#active-serial");
  private readonly activeApp = mustQuery<HTMLElement>("#active-app");
  private readonly activeState = mustQuery<HTMLElement>("#active-state");
  private readonly activeStatus =
    mustQuery<HTMLButtonElement>("#active-status");
  private readonly activeProvision =
    mustQuery<HTMLButtonElement>("#active-provision");
  private readonly activeRestore =
    mustQuery<HTMLButtonElement>("#active-restore");
  private readonly activeDisconnect =
    mustQuery<HTMLButtonElement>("#active-disconnect");
  private readonly updatePrompt =
    mustQuery<HTMLElement>("#active-update");
  private readonly updateRun =
    mustQuery<HTMLButtonElement>("#active-update-run");
  private readonly statusDetail =
    mustQuery<HTMLElement>("#active-status-detail");
  private readonly listElement =
    mustQuery<HTMLUListElement>("#device-list");
  private readonly empty = mustQuery<HTMLElement>("#device-empty");
  private readonly provisionPanel =
    mustQuery<HTMLElement>("#provision-panel");
  private readonly provisionDevice =
    mustQuery<HTMLElement>("#provision-device");
  private readonly provisionStatus =
    mustQuery<HTMLElement>("#provision-status");
  private readonly provisionSteps =
    mustQuery<HTMLOListElement>("#provision-steps");

  private api: DeviceApi | null = null;
  private host: HostClient | null = null;
  private list: DeviceList | null = null;
  private status: StatusReport | null = null;
  private jobTimer: number | null = null;
  private busy = false;

  constructor() {
    this.scanButton.addEventListener("click", () => {
      void this.refresh(true);
    });
    this.pairButton.addEventListener("click", () => this.openPair());
    this.pairCancel.addEventListener("click", () => this.pairDialog.close());
    this.pairForm.addEventListener("submit", (event) => {
      event.preventDefault();
      void this.submitPair();
    });
    this.select.addEventListener("change", () => {
      void this.switchDevice(this.select.value);
    });
    this.listElement.addEventListener("click", (event) => {
      const button = (event.target as HTMLElement).closest<HTMLButtonElement>(
        "button[data-action]",
      );
      if (!button) return;
      void this.runAction(button);
    });
    this.activeStatus.addEventListener("click", () => {
      void this.checkActiveStatus();
    });
    this.activeProvision.addEventListener("click", () => {
      const serial = this.list?.session.serial;
      if (serial) void this.startProvision(serial);
    });
    this.activeRestore.addEventListener("click", () => {
      void this.restoreActive();
    });
    this.activeDisconnect.addEventListener("click", () => {
      void this.disconnect();
    });
    this.updateRun.addEventListener("click", () => {
      const serial = this.list?.session.serial;
      if (serial) void this.startProvision(serial);
    });
    this.enable(false);
  }

  /** Attach to a host and start loading devices. */
  connect(token: string | null, host: HostClient): void {
    this.api = new DeviceApi(token);
    this.host = host;
    host.subscribeSession((session) => this.applySession(session));
    this.enable(true);
    void this.refresh();
  }

  /** Show the offline demo state (no host answered the probe). */
  demo(): void {
    this.enable(false);
    this.showNotice(
      "Demo mode: start `watchdog serve` to pair, provision and activate real devices.",
      "info",
    );
  }

  /** Reload the device list, optionally refreshing the mDNS cache. */
  async refresh(discover = false): Promise<void> {
    if (!this.api || this.busy) return;
    this.busy = true;
    this.setBusy(true);
    try {
      const list = await this.api.list(discover);
      this.list = list;
      this.render(list);
      if (list.session.device) {
        void this.loadStatus(list.session.serial ?? list.session.device.serial);
      }
    } catch (error) {
      this.showNotice(messageOf(error), "error");
    } finally {
      this.busy = false;
      this.setBusy(false);
    }
  }

  private enable(enabled: boolean): void {
    this.scanButton.disabled = !enabled;
    this.pairButton.disabled = !enabled;
    this.select.disabled = !enabled;
    this.activeCard.dataset.enabled = String(enabled);
    if (!enabled) {
      this.activeCard.hidden = true;
      this.provisionPanel.hidden = true;
    }
  }

  private setBusy(busy: boolean): void {
    this.scanButton.textContent = busy ? "Scanning…" : "Scan LAN";
    this.scanButton.setAttribute("aria-busy", String(busy));
    this.listElement.setAttribute("aria-busy", String(busy));
  }

  private showNotice(
    text: string,
    kind: "info" | "success" | "error" = "info",
  ): void {
    this.notice.textContent = text;
    this.notice.dataset.kind = kind;
    this.notice.hidden = text.length === 0;
  }

  private render(list: DeviceList): void {
    this.renderSelect(list.devices, list.session);
    this.renderActive(list.session);
    this.renderRows(list.devices);
  }

  private renderSelect(devices: DeviceEntry[], session: SessionInfo): void {
    const current = session.serial ?? "";
    const fragment = document.createDocumentFragment();
    const none = document.createElement("option");
    none.value = "";
    none.textContent = "No device";
    fragment.append(none);
    for (const device of devices) {
      if (device.state === "discovered" && !device.connect) continue;
      const option = document.createElement("option");
      option.value = device.serial;
      option.textContent = `${deviceLabel(device)} (${device.serial})`;
      fragment.append(option);
    }
    this.select.replaceChildren(fragment);
    this.select.value = devices.some((device) => device.serial === current)
      ? current
      : "";
  }

  private renderActive(session: SessionInfo): void {
    if (!session.device) {
      this.activeCard.hidden = true;
      return;
    }
    const device = session.device;
    this.activeCard.hidden = false;
    this.activeModel.textContent = `${device.manufacturer} ${device.model}`;
    this.activeSerial.textContent = device.serial;
    this.activeApp.textContent = `${device.app_version_name} (${device.app_version_code})`;
    this.activeState.textContent = session.active
      ? "Active"
      : session.closed
        ? "Session closed"
        : "Inactive";
    this.activeState.dataset.state = session.active ? "active" : "closed";
    this.activeProvision.hidden = false;
    this.activeRestore.hidden = false;
    this.activeDisconnect.hidden = !session.active;
    this.activeStatus.hidden = false;
    if (
      !this.status ||
      this.status.serial !== device.serial ||
      this.statusDetail.dataset.serial !== device.serial
    ) {
      this.statusDetail.hidden = true;
      this.statusDetail.textContent = "";
      this.updatePrompt.hidden = true;
    }
  }

  private renderRows(devices: DeviceEntry[]): void {
    this.listElement.replaceChildren();
    this.empty.hidden = devices.length > 0;
    for (const device of devices) {
      const item = document.createElement("li");
      item.className = "device-row";
      item.dataset.state = device.state;
      item.dataset.serial = device.serial;

      const main = document.createElement("div");
      main.className = "device-main";
      const title = document.createElement("strong");
      title.textContent = deviceLabel(device);
      const meta = document.createElement("span");
      meta.className = "device-meta";
      const parts = [
        device.kind === "usb" ? "USB" : "LAN",
        device.serial,
        device.pairing ? `pairing ${device.pairing}` : "",
      ].filter(Boolean);
      meta.textContent = parts.join(" · ");
      main.append(title, meta);

      const badge = document.createElement("span");
      badge.className = `badge badge-${device.state}`;
      badge.textContent = device.state;

      const actions = document.createElement("div");
      actions.className = "device-actions";
      const add = (
        action: string,
        label: string,
        target?: string,
      ): void => {
        const button = document.createElement("button");
        button.type = "button";
        button.className = "btn small";
        button.dataset.action = action;
        button.dataset.serial = device.serial;
        if (target) button.dataset.target = target;
        button.textContent = label;
        actions.append(button);
      };
      if (device.state === "active") {
        add("disconnect", "Disconnect");
      } else {
        add("activate", "Activate");
      }
      if (device.pairing) add("pair", "Pair", device.pairing);
      add("provision", "Provision");
      add("status", "Status");
      item.append(main, badge, actions);
      this.listElement.append(item);
    }
  }

  private applySession(session: SessionInfo): void {
    if (!this.list) return;
    if (sameSession(this.list.session, session)) return;
    this.list.session = session;
    this.renderSelect(this.list.devices, session);
    this.renderActive(session);
  }

  private async runAction(button: HTMLButtonElement): Promise<void> {
    const serial = button.dataset.serial;
    if (!serial) return;
    switch (button.dataset.action) {
      case "activate":
        await this.activate(serial);
        break;
      case "disconnect":
        await this.disconnect();
        break;
      case "pair":
        this.openPair(button.dataset.target);
        break;
      case "provision":
        await this.startProvision(serial);
        break;
      case "status":
        await this.checkStatus(serial);
        break;
      default:
        break;
    }
  }

  private async activate(serial: string): Promise<void> {
    if (!this.api) return;
    this.showNotice(`Activating ${serial}…`, "info");
    try {
      const result = await this.api.activate(serial);
      this.host?.applySession(result.session);
      this.showNotice(`Activated ${serial}.`, "success");
      await this.refresh();
    } catch (error) {
      this.showNotice(`Could not activate ${serial}: ${messageOf(error)}`, "error");
    }
  }

  private async disconnect(): Promise<void> {
    if (!this.api) return;
    try {
      const result = await this.api.disconnect();
      this.host?.applySession(result.session);
      this.status = null;
      this.showNotice("Device session closed.", "info");
      await this.refresh();
    } catch (error) {
      this.showNotice(messageOf(error), "error");
    }
  }

  private async switchDevice(serial: string): Promise<void> {
    if (!this.api) return;
    if (!serial) {
      await this.disconnect();
      return;
    }
    await this.activate(serial);
  }

  private openPair(target?: string): void {
    this.pairError.hidden = true;
    this.pairCode.value = "";
    const discovered = this.list?.devices.find(
      (device) => device.pairing && (!target || device.pairing === target),
    );
    this.pairTarget.value = target ?? discovered?.pairing ?? "";
    if (!this.pairDialog.open) this.pairDialog.showModal();
    if (this.pairTarget.value) {
      this.pairCode.focus();
    } else {
      this.pairTarget.focus();
    }
  }

  private async submitPair(): Promise<void> {
    if (!this.api) return;
    const target = this.pairTarget.value.trim();
    const code = this.pairCode.value.trim();
    if (!target || !/^\d{6}$/.test(code)) {
      this.pairError.textContent =
        "Enter the pairing address (IP:port) and the 6-digit code.";
      this.pairError.hidden = false;
      return;
    }
    try {
      await this.api.pair(target, code);
      this.pairDialog.close();
      this.showNotice(`Paired with ${target}.`, "success");
      await this.refresh(true);
    } catch (error) {
      this.pairError.textContent = messageOf(error);
      this.pairError.hidden = false;
    }
  }

  private async startProvision(serial: string): Promise<void> {
    if (!this.api) return;
    this.showNotice(`Provisioning ${serial}…`, "info");
    this.provisionPanel.hidden = false;
    this.provisionDevice.textContent = serial;
    this.provisionStatus.textContent = "Starting…";
    this.provisionSteps.replaceChildren();
    try {
      const { id } = await this.api.provision(serial);
      this.pollJob(id);
    } catch (error) {
      this.provisionStatus.textContent = "Provisioning did not start.";
      this.showNotice(messageOf(error), "error");
    }
  }

  private pollJob(id: string): void {
    if (this.jobTimer !== null) window.clearTimeout(this.jobTimer);
    const tick = async (): Promise<void> => {
      if (!this.api) return;
      try {
        const job = await this.api.provisionJob(id);
        this.renderJob(job);
        if (job.status === "running") {
          this.jobTimer = window.setTimeout(() => {
            void tick();
          }, 500);
          return;
        }
        this.jobTimer = null;
        if (job.status === "passed") {
          this.showNotice(`Provisioning ${job.serial} passed.`, "success");
          if (job.session) this.host?.applySession(job.session);
          if (job.session_error) {
            const hint = job.session_error.hint
              ? ` (${job.session_error.hint})`
              : "";
            this.showNotice(
              `Provisioning passed but the session did not open: ${job.session_error.message}${hint}`,
              "error",
            );
          }
        } else {
          this.showNotice(
            job.error?.message ?? `Provisioning ${job.serial} failed.`,
            "error",
          );
        }
        await this.refresh();
      } catch (error) {
        this.jobTimer = null;
        this.showNotice(messageOf(error), "error");
      }
    };
    void tick();
  }

  private renderJob(job: ProvisionJob): void {
    const passed = job.steps.filter((step) => step.status === "passed").length;
    const failed = job.steps.filter((step) => step.status === "failed").length;
    this.provisionStatus.textContent =
      job.status === "running"
        ? `${passed} passed, ${failed} failed, ${job.steps.length} steps…`
        : job.status === "passed"
          ? `Passed ${passed} step${passed === 1 ? "" : "s"}.`
          : job.error?.message ?? "Provisioning failed.";
    this.provisionSteps.replaceChildren();
    for (const step of job.steps) {
      const item = document.createElement("li");
      item.className = `step step-${step.status}`;
      const name = document.createElement("span");
      name.className = "step-name";
      name.textContent = step.name;
      const status = document.createElement("span");
      status.className = "step-status";
      status.textContent = step.status;
      const detail = document.createElement("span");
      detail.className = "step-detail";
      detail.textContent = `${step.detail} (${step.duration_ms} ms)`;
      item.append(name, status, detail);
      if (step.hint) {
        const hint = document.createElement("span");
        hint.className = "step-hint";
        hint.textContent = step.hint;
        item.append(hint);
      }
      this.provisionSteps.append(item);
    }
  }

  private async checkActiveStatus(): Promise<void> {
    const serial = this.list?.session.serial;
    if (serial) await this.checkStatus(serial);
  }

  private async checkStatus(serial: string): Promise<void> {
    if (!this.api) return;
    try {
      const status = await this.api.status(serial);
      if (this.list?.session.serial === serial) {
        this.status = status;
        this.renderStatus(status);
      } else {
        this.showNotice(
          `${status.props.manufacturer} ${status.props.model}: ${
            status.installed ? "app installed" : "app not installed"
          }, Android ${status.props.release} (SDK ${status.props.sdk}).`,
          status.installed ? "success" : "info",
        );
      }
    } catch (error) {
      this.showNotice(`Status for ${serial}: ${messageOf(error)}`, "error");
    }
  }

  private async loadStatus(serial: string): Promise<void> {
    if (!this.api) return;
    try {
      this.status = await this.api.status(serial);
      if (this.list?.session.serial === serial) {
        this.renderStatus(this.status);
      }
    } catch (error) {
      this.status = null;
      this.statusDetail.textContent = `Status unavailable: ${messageOf(error)}`;
      this.statusDetail.dataset.serial = serial;
      this.statusDetail.hidden = false;
      this.updatePrompt.hidden = true;
    }
  }

  private renderStatus(status: StatusReport): void {
    const props = status.props;
    const lines = [
      `${props.manufacturer} ${props.model} · Android ${props.release} (SDK ${props.sdk}) · ${props.abi}`,
      status.installed
        ? `app installed: versionCode ${status.version_code ?? "?"}`
        : "app not installed",
    ];
    if (status.expected_version_name) {
      lines.push(
        `embedded build: ${status.expected_version_name} (${status.expected_version_code ?? "?"})`,
      );
    }
    lines.push(
      `service: ${status.service_pid ? `pid ${status.service_pid}` : "not running"}`,
    );
    lines.push(
      `appops: CAMERA=${status.appops_camera ?? "unknown"} RECORD_AUDIO=${status.appops_record_audio ?? "unknown"}`,
    );
    if (status.restore?.originals) {
      const count = Object.keys(status.restore.originals).length;
      lines.push(
        count === 0
          ? "restore: no settings changed"
          : `restore: ${count} setting(s) recorded`,
      );
    }
    this.statusDetail.textContent = lines.join("\n");
    this.statusDetail.dataset.serial = status.serial;
    this.statusDetail.hidden = false;
    this.updatePrompt.hidden = status.up_to_date !== false;
    this.activeState.textContent =
      status.up_to_date === false
        ? "Update available"
        : status.installed
          ? "Active"
          : "App missing";
  }

  private async restoreActive(): Promise<void> {
    const serial = this.list?.session.serial;
    if (!serial || !this.api) return;
    this.showNotice(`Restoring settings on ${serial}…`, "info");
    try {
      const result = await this.api.restore(serial);
      const failed = result.steps.filter((step) => !step.applied).length;
      this.showNotice(
        result.steps.length === 0
          ? `No recorded settings to restore on ${serial}.`
          : failed === 0
            ? `Restored ${result.steps.length} setting(s) on ${serial}.`
            : `Restored ${result.steps.length - failed} of ${result.steps.length} setting(s); ${failed} failed.`,
        failed === 0 ? "success" : "error",
      );
      await this.refresh();
    } catch (error) {
      this.showNotice(messageOf(error), "error");
    }
  }
}
