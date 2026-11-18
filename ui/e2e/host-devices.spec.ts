import { MOCK_SERIAL, expect, openRoute, test } from "./fixtures";

test.describe("host devices", () => {
  test("shows the active mock session and its status", async ({
    hostPage,
  }) => {
    await openRoute(hostPage, "devices");

    const active = hostPage.getByTestId("active-device");
    await expect(active).toBeVisible();
    await expect(hostPage.locator("#active-state")).toHaveText("Active");
    await expect(hostPage.locator("#active-model")).toHaveText(
      "watchdog Mock device",
    );
    await expect(hostPage.locator("#active-serial")).toHaveText(MOCK_SERIAL);
    await expect(hostPage.locator("#active-app")).toHaveText("0.1.0-mock (1)");

    const status = hostPage.getByTestId("status-detail");
    await expect(status).toBeVisible();
    await expect(status).toContainText("app installed: versionCode 1");
    await expect(status).toContainText("service: pid 4242");
    await expect(status).toContainText(
      "appops: CAMERA=allow RECORD_AUDIO=allow",
    );

    const row = hostPage.locator(
      `#device-list li[data-serial="${MOCK_SERIAL}"]`,
    );
    await expect(row).toHaveAttribute("data-state", "active");
    await expect(row).toContainText(MOCK_SERIAL);
  });

  test("rescans the LAN without losing the session", async ({ hostPage }) => {
    await openRoute(hostPage, "devices");
    const scan = hostPage.locator("#scan-devices");
    await scan.click();
    await expect(scan).toHaveText("Scan LAN");
    await expect(scan).toHaveAttribute("aria-busy", "false");
    await expect(
      hostPage.locator(`#device-list li[data-serial="${MOCK_SERIAL}"]`),
    ).toHaveAttribute("data-state", "active");
  });

  test("rejects pairing while the mock device is active", async ({
    hostPage,
  }) => {
    await openRoute(hostPage, "devices");
    await hostPage.locator("#pair-open").click();
    await expect(hostPage.getByTestId("pair-dialog")).toBeVisible();

    await hostPage.locator("#pair-target").fill("192.168.93.76:34883");
    await hostPage.locator("#pair-code").fill("123456");
    await hostPage.locator("#pair-submit").click();

    const error = hostPage.locator("#pair-error");
    await expect(error).toBeVisible();
    await expect(error).toContainText(
      "not available while the host runs the mock device",
    );
    await hostPage.locator("#pair-cancel").click();
    await expect(hostPage.getByTestId("pair-dialog")).toBeHidden();
  });

  test("rejects provisioning and reports nothing to restore", async ({
    hostPage,
  }) => {
    await openRoute(hostPage, "devices");
    await hostPage.locator("#active-provision").click();
    const notice = hostPage.getByTestId("devices-notice");
    await expect(notice).toBeVisible();
    await expect(notice).toContainText(
      "not available while the host runs the mock device",
    );

    await hostPage.locator("#active-restore").click();
    await expect(notice).toHaveText(
      `No recorded settings to restore on ${MOCK_SERIAL}.`,
    );
  });

  test("disconnects and reactivates the mock session", async ({ hostPage }) => {
    await openRoute(hostPage, "devices");
    const active = hostPage.getByTestId("active-device");
    await expect(active).toBeVisible();

    await hostPage.locator("#active-disconnect").click();
    const notice = hostPage.getByTestId("devices-notice");
    await expect(notice).toHaveText("Device session closed.");
    await expect(active).toBeHidden();

    const row = hostPage.locator(
      `#device-list li[data-serial="${MOCK_SERIAL}"]`,
    );
    // Deactivating drops the session, so the mock row returns to `available`.
    await expect(row).toHaveAttribute("data-state", "available");
    await row.locator('button[data-action="activate"]').click();

    await expect(notice).toHaveText(`Activated ${MOCK_SERIAL}.`);
    await expect(active).toBeVisible();
    await expect(hostPage.locator("#active-state")).toHaveText("Active");
    await expect(row).toHaveAttribute("data-state", "active");
  });
});
