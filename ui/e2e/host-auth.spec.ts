import {
  E2E_BASE_URL,
  MOCK_SERIAL,
  TOKEN,
  ensureMockActive,
  expect,
  test,
} from "./fixtures";

const WRONG_TOKEN = "definitely-not-the-host-token";

test.describe("host authentication", () => {
  test("consumes the URL token and keeps it in session storage", async ({
    browser,
    request,
  }) => {
    await ensureMockActive(request);
    const context = await browser.newContext({ baseURL: E2E_BASE_URL });
    const page = await context.newPage();
    try {
      await page.goto(`/?token=${TOKEN}`);
      await expect(page.locator("#mode-tag")).toHaveText("host", {
        timeout: 20_000,
      });
      expect(page.url()).not.toContain("token=");
      await expect
        .poll(() =>
          page.evaluate(() => window.sessionStorage.getItem("watchdog.token")),
        )
        .toBe(TOKEN);
      await expect(page.locator("#device-select")).toHaveValue(MOCK_SERIAL);
    } finally {
      await context.close();
    }
  });

  test("recovers from a stored token the host rejects", async ({
    browser,
    request,
  }) => {
    await ensureMockActive(request);
    const context = await browser.newContext({ baseURL: E2E_BASE_URL });
    const page = await context.newPage();
    try {
      await page.addInitScript((token) => {
        try {
          window.sessionStorage.setItem("watchdog.token", token);
        } catch {
          // Ignore storage failures; the dialog still appears.
        }
      }, WRONG_TOKEN);
      await page.goto("/");

      const dialog = page.getByTestId("token-dialog");
      await expect(dialog).toBeVisible();
      await page.locator("#token-input").fill(TOKEN);
      await page.locator("#token-submit").click();

      await expect(page.locator("#mode-tag")).toHaveText("host", {
        timeout: 20_000,
      });
      await expect(dialog).toBeHidden();
      await expect
        .poll(() =>
          page.evaluate(() => window.sessionStorage.getItem("watchdog.token")),
        )
        .toBe(TOKEN);
    } finally {
      await context.close();
    }
  });

  test("falls back to the offline demo when the token prompt is cancelled", async ({
    browser,
    request,
  }) => {
    await ensureMockActive(request);
    const context = await browser.newContext({ baseURL: E2E_BASE_URL });
    const page = await context.newPage();
    try {
      await page.addInitScript((token) => {
        try {
          window.sessionStorage.setItem("watchdog.token", token);
        } catch {
          // Ignore storage failures; the dialog still appears.
        }
      }, WRONG_TOKEN);
      await page.goto("/");

      const dialog = page.getByTestId("token-dialog");
      await expect(dialog).toBeVisible();
      await page.locator("#token-cancel").click();

      await expect(page.locator("#mode-tag")).toHaveText("demo device");
      await expect(page.getByTestId("live-status")).toContainText(
        "Demo viewfinder",
      );
      await expect(dialog).toBeHidden();
    } finally {
      await context.close();
    }
  });
});
