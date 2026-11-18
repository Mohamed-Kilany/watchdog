// Accessibility checks for the offline demo shell (M6-T10).
//
// The demo build is what a browser shows without a host, so it gets the same
// axe-core bar as the host pages: WCAG 2.0/2.1/2.2 A+AA plus best-practice
// rules on every route and on the media preview dialog.

import type { Page } from "@playwright/test";
import { expect, test } from "@playwright/test";
import { expectNoViolations } from "../e2e/axe";

const ROUTES = ["live", "capture", "media", "settings", "devices"] as const;
type Route = (typeof ROUTES)[number];

async function openRoute(page: Page, route: Route): Promise<void> {
  await page.goto("/");
  await page.locator(`a[data-route="${route}"]`).click();
  await expect(page.locator(`#view-${route}`)).toBeVisible();
  switch (route) {
    case "live":
      await expect(page.locator("#viewfinder")).toHaveAttribute(
        "data-state",
        "demo",
      );
      break;
    case "capture":
      await expect(page.getByTestId("capture-status")).toHaveText("Ready.");
      break;
    case "media":
      await expect(page.getByTestId("media-card").first()).toBeVisible();
      break;
    case "settings":
      await expect(page.getByTestId("settings-status")).toHaveText(
        "Loaded demo settings.",
      );
      break;
    case "devices":
      await expect(page.getByTestId("devices-notice")).toBeVisible();
      break;
  }
}

test.describe("demo accessibility", () => {
  for (const route of ROUTES) {
    test(`${route} page has no axe violations`, async ({ page }) => {
      await openRoute(page, route);
      await expectNoViolations(page);
    });
  }

  test("media preview dialog has no axe violations", async ({ page }) => {
    await openRoute(page, "media");
    const card = page.locator('li.media-card[data-type="still"]').first();
    await expect(card).toBeVisible();
    await card.getByTestId("media-open").click();
    await expect(page.getByTestId("media-dialog")).toBeVisible();
    await expectNoViolations(page);
  });
});
