import {
  MOCK_SERIAL,
  expect,
  openLive,
  openRoute,
  test,
} from "./fixtures";

test.describe("host shell", () => {
  test("boots against the host with an active mock session", async ({
    hostPage,
  }) => {
    await expect(hostPage.locator("#mode-tag")).toHaveText("host");
    await expect(hostPage.locator("#connection-banner")).toBeHidden();
    await expect(hostPage.locator("#device-select")).toHaveValue(MOCK_SERIAL);
    await expect(hostPage.locator("#device-status")).toContainText("host");
    await expect(hostPage.locator("#device-status")).toContainText("command");

    await openLive(hostPage);
    await expect(hostPage.getByTestId("vf-corner")).toContainText("LIVE");
    await expect(hostPage.getByTestId("vf-corner")).toContainText(
      "Mock device",
    );
  });

  test("persists the theme across reloads", async ({ hostPage }) => {
    const html = hostPage.locator("html");
    const before = await html.getAttribute("data-theme");
    expect(before === "dark" || before === "light").toBeTruthy();
    const after = before === "dark" ? "light" : "dark";

    await hostPage.locator("#theme-toggle").click();
    await expect(html).toHaveAttribute("data-theme", after);
    await expect(hostPage.locator("#theme-toggle")).toHaveText(
      `Theme: ${after}`,
    );

    await hostPage.reload();
    await expect(hostPage.locator("#mode-tag")).toHaveText("host");
    await expect(hostPage.locator("html")).toHaveAttribute("data-theme", after);
  });

  test("navigates every page without a reload", async ({ hostPage }) => {
    await openLive(hostPage);
    await expect(
      hostPage.locator('a[data-route="live"]'),
    ).toHaveAttribute("aria-current", "page");

    await openRoute(hostPage, "capture");
    await expect(hostPage.getByTestId("capture-mode")).toHaveText("host");
    await expect(
      hostPage.locator('a[data-route="capture"]'),
    ).toHaveAttribute("aria-current", "page");

    await openRoute(hostPage, "media");
    await expect(hostPage.getByTestId("media-mode")).toHaveText("host library");
    await expect(hostPage.getByTestId("media-count")).toHaveText(
      /\d+ of \d+ items/,
    );

    await openRoute(hostPage, "settings");
    await expect(hostPage.getByTestId("settings-mode")).toHaveText(
      "host settings",
    );
    await expect(hostPage.getByTestId("settings-status")).toHaveText(
      "Loaded settings from the host.",
    );

    await openRoute(hostPage, "devices");
    await expect(hostPage.getByTestId("active-device")).toBeVisible();
    await expect(
      hostPage.locator('a[data-route="devices"]'),
    ).toHaveAttribute("aria-current", "page");

    await openRoute(hostPage, "live");
    await expect(hostPage.locator("#view-live")).toBeVisible();
  });
});
