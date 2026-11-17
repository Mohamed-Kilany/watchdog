import { expect, test, type Page } from "@playwright/test";
import type { ControlState } from "../src/types";

async function state(page: Page): Promise<ControlState> {
  return page.evaluate(() => window.__watchdog.state());
}

test.beforeEach(async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("panel")).toBeVisible();
  await expect(page.getByTestId("preset-manager")).toBeVisible();
});

test("saves writable controls only and stores them in the demo", async ({
  page,
}) => {
  await page.locator('wd-auto-toggle[data-control-id="ae_mode"] button').click();
  const iso = page.locator('wd-slider[data-control-id="iso"] input[type="number"]');
  await iso.fill("800");
  await iso.press("Enter");
  await page
    .locator('wd-select[data-control-id="anti_banding"] select')
    .selectOption({ label: "Off" });

  await page.getByTestId("preset-name").fill("TV capture");
  await page.getByTestId("preset-save").click();

  await expect(page.getByTestId("preset-status")).toContainText(
    'Saved "TV capture"',
  );
  await expect(
    page.getByTestId("preset-select").locator('option[value="TV capture"]'),
  ).toHaveCount(1);

  const controls = await page.evaluate(() => {
    const raw = localStorage.getItem("watchdog.presets");
    if (!raw) return null;
    const store = JSON.parse(raw) as Record<
      string,
      { controls: Record<string, unknown> }
    >;
    return store["tv-capture"]?.controls ?? null;
  });
  expect(controls).not.toBeNull();
  expect(controls?.iso).toBe(800);
  expect(controls?.ae_mode).toBe(0);
  expect(controls?.anti_banding).toBe(0);
  expect(controls).not.toHaveProperty("battery_percent");
  expect(controls).not.toHaveProperty("af_trigger");
  expect(controls).not.toHaveProperty("stream_size");
  expect(controls).not.toHaveProperty("camera_id");
});

test("applies a saved preset as one batch and restores the values", async ({
  page,
}) => {
  await page.locator('wd-auto-toggle[data-control-id="ae_mode"] button').click();
  const iso = page.locator('wd-slider[data-control-id="iso"] input[type="number"]');
  await iso.fill("800");
  await iso.press("Enter");
  await page
    .locator('wd-select[data-control-id="anti_banding"] select')
    .selectOption({ label: "Off" });

  await page.getByTestId("preset-name").fill("TV capture");
  await page.getByTestId("preset-save").click();
  await expect(page.getByTestId("preset-status")).toContainText('Saved "TV capture"');

  // Move the camera away from the saved state.
  await iso.fill("100");
  await iso.press("Enter");
  await page
    .locator('wd-select[data-control-id="anti_banding"] select')
    .selectOption({ label: "50 Hz" });
  await expect.poll(async () => (await state(page)).iso?.requested).toBe(100);
  await expect
    .poll(async () => (await state(page)).anti_banding?.requested)
    .toBe(1);

  const select = page.getByTestId("preset-select");
  await select.selectOption({ value: "TV capture" });
  await page.getByTestId("preset-apply").click();

  await expect(page.getByTestId("preset-status")).toContainText(
    'Applied "TV capture"',
  );
  await expect.poll(async () => (await state(page)).iso?.requested).toBe(800);
  await expect
    .poll(async () => (await state(page)).anti_banding?.requested)
    .toBe(0);
  await expect(page.getByTestId("command-log")).toContainText(
    'preset="TV capture"',
  );

  const batches = await page.evaluate(() => window.__watchdog.commands());
  const apply = batches.find((entry) =>
    entry.batch.some((change) => change.id === "preset"),
  );
  expect(apply).toBeDefined();
  expect(apply?.results.every((result) => result.ok)).toBe(true);
});

test("deletes a saved preset and rejects empty or slugless names", async ({
  page,
}) => {
  await page.getByTestId("preset-save").click();
  await expect(page.getByTestId("preset-status")).toHaveText(
    "Enter a preset name first.",
  );

  await page.getByTestId("preset-name").fill("!!!");
  await page.getByTestId("preset-save").click();
  await expect(page.getByTestId("preset-status")).toContainText(
    "preset name needs an ASCII letter or digit",
  );

  await page.getByTestId("preset-name").fill("TV capture");
  await page.getByTestId("preset-save").click();
  await expect(page.getByTestId("preset-status")).toContainText('Saved "TV capture"');

  const select = page.getByTestId("preset-select");
  await select.selectOption({ value: "TV capture" });
  await page.getByTestId("preset-delete").click();

  await expect(page.getByTestId("preset-status")).toContainText(
    'Deleted "TV capture"',
  );
  await expect(select.locator('option[value="TV capture"]')).toHaveCount(0);
  const stored = await page.evaluate(() =>
    localStorage.getItem("watchdog.presets"),
  );
  expect(stored === null || stored === "{}").toBe(true);
});
