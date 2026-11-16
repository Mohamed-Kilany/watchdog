import { expect, test } from "@playwright/test";

test("theme toggle switches and persists the palette", async ({ page }) => {
  await page.goto("/");
  const root = page.locator("html");
  const initialTheme = await root.getAttribute("data-theme");
  expect(initialTheme === "dark" || initialTheme === "light").toBe(true);
  const initialBackground = await page.evaluate(
    () => getComputedStyle(document.body).backgroundColor,
  );

  await page.locator("#theme-toggle").click();
  const nextTheme = initialTheme === "dark" ? "light" : "dark";
  await expect(root).toHaveAttribute("data-theme", nextTheme);
  const nextBackground = await page.evaluate(
    () => getComputedStyle(document.body).backgroundColor,
  );
  expect(nextBackground).not.toBe(initialBackground);

  await page.reload();
  await expect(root).toHaveAttribute("data-theme", nextTheme);
});

test("tablet width keeps every control usable", async ({ page }) => {
  await page.setViewportSize({ width: 768, height: 1024 });
  await page.goto("/");
  await expect(page.getByTestId("panel")).toBeVisible();
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= window.innerWidth,
    ),
  ).toBe(true);

  await page.locator('wd-auto-toggle[data-control-id="ae_mode"] button').click();
  const iso = page.locator('wd-slider[data-control-id="iso"] input[type="number"]');
  await expect(iso).toBeVisible();
  await iso.fill("400");
  await iso.press("Enter");
  await expect
    .poll(async () => (await page.evaluate(() => window.__watchdog.state())).iso?.requested)
    .toBe(400);
});

test("loads offline with no external requests or web fonts", async ({ page }) => {
  const requests: string[] = [];
  page.on("request", (request) => requests.push(request.url()));
  await page.goto("/");
  await expect(page.getByTestId("panel")).toBeVisible();

  const external = requests.filter(
    (url) => !url.startsWith("http://127.0.0.1:4173"),
  );
  expect(external).toEqual([]);

  const fontFaces = await page.evaluate(() => document.fonts.size);
  expect(fontFaces).toBe(0);

  const externalLinks = await page.evaluate(() =>
    Array.from(document.querySelectorAll<HTMLLinkElement>("link[href]"))
      .map((link) => link.href)
      .filter((href) => !href.startsWith(window.location.origin)),
  );
  expect(externalLinks).toEqual([]);
});

test("command log records the fake-device batch", async ({ page }) => {
  await page.goto("/");
  await page.locator('wd-auto-toggle[data-control-id="ae_mode"] button').click();
  const iso = page.locator('wd-slider[data-control-id="iso"] input[type="number"]');
  await iso.fill("800");
  await iso.press("Enter");
  await expect(page.getByTestId("command-log")).toContainText("iso=800");
  await expect(page.locator("#device-status")).toContainText("command");
});
