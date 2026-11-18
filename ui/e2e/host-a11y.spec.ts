// Accessibility checks for the host-backed UI (M6-T10).
//
// Each page is driven to a settled state against the mock device, then scanned
// with axe-core (WCAG 2.0/2.1/2.2 A+AA and best-practice rules). Dialogs that
// are only reachable through interaction get their own scans so the suite
// covers every visible surface, not just the default route.

import { expectNoViolations } from "./axe";
import { E2E_BASE_URL, expect, openLive, openRoute, test } from "./fixtures";

test.describe("host accessibility", () => {
  test("live page has no axe violations", async ({ hostPage }) => {
    await openLive(hostPage);
    await expectNoViolations(hostPage);
  });

  test("capture page has no axe violations", async ({ hostPage }) => {
    await openRoute(hostPage, "capture");
    await expect(hostPage.getByTestId("capture-status")).toHaveText("Ready.");
    await expectNoViolations(hostPage);
  });

  test("media page has no axe violations", async ({ hostPage }) => {
    await openRoute(hostPage, "media");
    await expect(hostPage.getByTestId("media-card").first()).toBeVisible();
    await expectNoViolations(hostPage);
  });

  test("media preview dialog has no axe violations", async ({ hostPage }) => {
    await openRoute(hostPage, "media");
    await hostPage.getByTestId("media-search").fill("m-mock-seed-still-1");
    const card = hostPage.locator(
      'li.media-card[data-media-id="m-mock-seed-still-1"]',
    );
    await expect(card).toBeVisible();
    await card.getByTestId("media-open").click();
    await expect(hostPage.getByTestId("media-dialog")).toBeVisible();
    await expectNoViolations(hostPage);
  });

  test("media video preview has no axe violations", async ({ hostPage }) => {
    await openRoute(hostPage, "media");
    await hostPage.getByTestId("media-search").fill("m-mock-seed-video-1");
    const card = hostPage.locator(
      'li.media-card[data-media-id="m-mock-seed-video-1"]',
    );
    await expect(card).toBeVisible();
    await card.getByTestId("media-open").click();
    await expect(hostPage.getByTestId("media-video")).toBeVisible();
    await expectNoViolations(hostPage);
  });

  test("settings page has no axe violations", async ({ hostPage }) => {
    await openRoute(hostPage, "settings");
    await expect(hostPage.getByTestId("settings-status")).toHaveText(
      "Loaded settings from the host.",
    );
    await expect(
      hostPage.locator("#settings-log-list li").first(),
    ).toBeVisible();
    await expectNoViolations(hostPage);
  });

  test("devices page has no axe violations", async ({ hostPage }) => {
    await openRoute(hostPage, "devices");
    await expect(hostPage.getByTestId("active-device")).toBeVisible();
    await expectNoViolations(hostPage);
  });

  test("pair dialog has no axe violations", async ({ hostPage }) => {
    await openRoute(hostPage, "devices");
    await hostPage.locator("#pair-open").click();
    await expect(hostPage.getByTestId("pair-dialog")).toBeVisible();
    await expectNoViolations(hostPage);
  });

  test("token prompt has no axe violations", async ({ browser }) => {
    const context = await browser.newContext({ baseURL: E2E_BASE_URL });
    const page = await context.newPage();
    try {
      await page.goto("/");
      await expect(page.getByTestId("token-dialog")).toBeVisible();
      await expectNoViolations(page);
    } finally {
      await context.close();
    }
  });
});
