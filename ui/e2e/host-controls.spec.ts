import type { Page } from "@playwright/test";
import { expect, openLive, test } from "./fixtures";

/** Requested value of one control in the shell's merged snapshot. */
async function requested(page: Page, controlId: string): Promise<unknown> {
  return page.evaluate(
    (id) => window.__watchdog.state()[id]?.requested,
    controlId,
  );
}

/** Actual value of one control in the shell's merged snapshot. */
async function actual(page: Page, controlId: string): Promise<unknown> {
  return page.evaluate(
    (id) => window.__watchdog.state()[id]?.actual,
    controlId,
  );
}

test.describe("host controls", () => {
  test("switches auto exposure to manual and applies ISO", async ({
    hostPage,
  }) => {
    await openLive(hostPage);
    const auto = hostPage.locator(
      'wd-auto-toggle[data-control-id="ae_mode"] button',
    );
    if ((await auto.getAttribute("aria-checked")) === "true") {
      await auto.click();
    }
    await expect(auto).toHaveAttribute("aria-checked", "false");

    const iso = hostPage.locator('wd-slider[control-id="iso"] input.number');
    await expect(iso).toBeEnabled();
    await iso.fill("800");
    await iso.press("Enter");

    await expect.poll(() => requested(hostPage, "iso")).toBe(800);
    await expect.poll(() => actual(hostPage, "iso")).toBe(800);
    await expect(hostPage.getByTestId("command-log")).toContainText("iso=800");
  });

  test("changes the white balance select", async ({ hostPage }) => {
    await openLive(hostPage);
    const select = hostPage.locator(
      'wd-select[data-control-id="awb_mode"] select',
    );
    await select.selectOption({ label: "Incandescent" });
    await expect.poll(() => requested(hostPage, "awb_mode")).toBe(2);
    await expect(hostPage.getByTestId("command-log")).toContainText(
      "awb_mode=2",
    );
  });

  test("locks auto exposure", async ({ hostPage }) => {
    await openLive(hostPage);
    const lock = hostPage.locator(
      'wd-lock-button[data-control-id="ae_lock"] button',
    );
    if ((await lock.getAttribute("aria-pressed")) !== "true") {
      await lock.click();
    }
    await expect(lock).toHaveAttribute("aria-pressed", "true");
    await expect
      .poll(() =>
        hostPage.evaluate(() => window.__watchdog.state().ae_lock?.locked),
      )
      .toBe(true);
  });

  test("toggles the torch", async ({ hostPage }) => {
    await openLive(hostPage);
    const torch = hostPage.locator('wd-toggle[data-control-id="torch"] button');
    if ((await torch.getAttribute("aria-checked")) !== "true") {
      await torch.click();
    }
    await expect(torch).toHaveAttribute("aria-checked", "true");
    await expect.poll(() => requested(hostPage, "torch")).toBe(true);
  });

  test("saves, applies and deletes a host preset", async ({ hostPage }) => {
    await openLive(hostPage);
    const name = `E2E preset ${Date.now()}`;
    const manager = hostPage.getByTestId("preset-manager");
    const status = manager.getByTestId("preset-status");

    await manager.getByTestId("preset-name").fill(name);
    await manager.getByTestId("preset-save").click();
    await expect(status).toContainText(`Saved "${name}" with`);
    await expect(manager.getByTestId("preset-select")).toHaveValue(name);

    await manager.getByTestId("preset-select").selectOption(name);
    await manager.getByTestId("preset-apply").click();
    await expect(status).toContainText(`Applied "${name}"`);
    await expect(hostPage.getByTestId("command-log")).toContainText(
      `preset=${JSON.stringify(name)}`,
    );

    await manager.getByTestId("preset-select").selectOption(name);
    await manager.getByTestId("preset-delete").click();
    await expect(status).toHaveText(`Deleted "${name}".`);
    await expect(manager.getByTestId("preset-select")).not.toHaveValue(name);
  });

  test("resets every control to auto", async ({ hostPage }) => {
    await openLive(hostPage);
    await hostPage.locator("#reset-all").click();
    await expect(hostPage.getByTestId("command-log")).toContainText(
      "reset_all=true",
    );
    await expect.poll(() => requested(hostPage, "ae_mode")).toBe(1);
  });
});
