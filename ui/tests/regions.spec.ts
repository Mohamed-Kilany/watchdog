import { expect, test, type Locator, type Page } from "@playwright/test";
import type { ControlRegion } from "../src/types";

async function state(page: Page) {
  return page.evaluate(() => window.__watchdog.state());
}

async function regionOf(
  page: Page,
  id: "ae_regions" | "af_regions" | "awb_regions",
): Promise<ControlRegion | null> {
  return page.evaluate((controlId) => {
    const entry = window.__watchdog.state()[controlId];
    const actual = entry?.actual;
    if (!actual || typeof actual !== "object" || Array.isArray(actual)) {
      return null;
    }
    const record = actual as Record<string, unknown>;
    const x = Number(record["x"]);
    const y = Number(record["y"]);
    const w = Number(record["w"]);
    const h = Number(record["h"]);
    if (![x, y, w, h].every(Number.isFinite)) return null;
    return { x, y, w, h };
  }, id);
}

async function drag(
  page: Page,
  overlay: Locator,
  start: { x: number; y: number },
  end: { x: number; y: number },
): Promise<void> {
  const box = await overlay.boundingBox();
  expect(box).not.toBeNull();
  if (!box) return;
  await page.mouse.move(box.x + box.width * start.x, box.y + box.height * start.y);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width * end.x, box.y + box.height * end.y, {
    steps: 8,
  });
  await page.mouse.up();
}

test.beforeEach(async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("panel")).toBeVisible();
});

test("pointer drag draws a normalized metering region", async ({ page }) => {
  const overlay = page.getByTestId("metering-overlay");
  await drag(page, overlay, { x: 0.25, y: 0.25 }, { x: 0.55, y: 0.65 });

  await expect
    .poll(async () => {
      const region = await regionOf(page, "ae_regions");
      if (!region) return false;
      return (
        Math.abs(region.x - 0.25) < 0.02 &&
        Math.abs(region.y - 0.25) < 0.02 &&
        Math.abs(region.w - 0.3) < 0.02 &&
        Math.abs(region.h - 0.4) < 0.02
      );
    })
    .toBe(true);

  await expect(overlay.locator("rect.region")).toHaveCount(1);
  await expect(page.getByTestId("region-readout")).toContainText("metering");
  await expect(page.getByTestId("command-log")).toContainText("ae_regions");
});

test("focus region drag sends af_regions and a focus trigger", async ({
  page,
}) => {
  await page.getByRole("radio", { name: "Focus" }).check();
  const overlay = page.getByTestId("focus-overlay");
  await drag(page, overlay, { x: 0.3, y: 0.3 }, { x: 0.6, y: 0.7 });

  await expect
    .poll(async () => {
      const region = await regionOf(page, "af_regions");
      if (!region) return false;
      return (
        Math.abs(region.x - 0.3) < 0.02 &&
        Math.abs(region.y - 0.3) < 0.02 &&
        Math.abs(region.w - 0.3) < 0.02 &&
        Math.abs(region.h - 0.4) < 0.02
      );
    })
    .toBe(true);

  await expect.poll(async () => (await state(page)).af_trigger?.actual).toBe(
    "start",
  );
  await expect.poll(async () => (await state(page)).af_state?.actual).toBe(
    "focused_locked",
  );
  await expect(page.getByTestId("command-log")).toContainText("af_regions");
  await expect(page.getByTestId("command-log")).toContainText(
    'af_trigger="start"',
  );
});

test("keyboard moves, resizes and commits a focus region", async ({ page }) => {
  await page.getByRole("radio", { name: "Focus" }).check();
  const overlay = page.getByTestId("focus-overlay");
  await overlay.focus();
  for (let index = 0; index < 5; index += 1) {
    await page.keyboard.press("ArrowRight");
  }
  await page.keyboard.press("Shift+ArrowDown");
  await page.keyboard.press("Enter");

  await expect
    .poll(async () => {
      const region = await regionOf(page, "af_regions");
      if (!region) return false;
      return (
        Math.abs(region.x - 0.5) < 0.02 &&
        Math.abs(region.y - 0.4) < 0.02 &&
        Math.abs(region.w - 0.2) < 0.02 &&
        Math.abs(region.h - 0.25) < 0.02
      );
    })
    .toBe(true);
});

test("Escape discards a keyboard draft", async ({ page }) => {
  await page.getByRole("radio", { name: "Focus" }).check();
  const overlay = page.getByTestId("focus-overlay");
  await overlay.focus();
  await page.keyboard.press("ArrowRight");
  await page.keyboard.press("Escape");
  await page.keyboard.press("Enter");
  await expect.poll(async () => (await state(page)).af_regions?.actual).toBeUndefined();
});

test("white balance region drag sends awb_regions and updates AWB state", async ({
  page,
}) => {
  await page.getByRole("radio", { name: "White balance" }).check();
  const overlay = page.getByTestId("awb-overlay");
  await expect(overlay).toHaveAttribute("editable", "");
  await expect(page.getByTestId("metering-overlay")).not.toHaveAttribute(
    "editable",
    "",
  );

  await drag(page, overlay, { x: 0.2, y: 0.2 }, { x: 0.5, y: 0.6 });

  await expect
    .poll(async () => {
      const region = await regionOf(page, "awb_regions");
      if (!region) return false;
      return (
        Math.abs(region.x - 0.2) < 0.02 &&
        Math.abs(region.y - 0.2) < 0.02 &&
        Math.abs(region.w - 0.3) < 0.02 &&
        Math.abs(region.h - 0.4) < 0.02
      );
    })
    .toBe(true);

  await expect.poll(async () => (await state(page)).awb_state?.actual).toBe(
    "searching",
  );
  await expect(overlay.locator("rect.region")).toHaveCount(1);
  await expect(page.getByTestId("region-readout")).toContainText(
    "white balance",
  );
  await expect(page.getByTestId("command-log")).toContainText("awb_regions");
});

test("clear regions removes metering, focus and white balance regions", async ({
  page,
}) => {
  await drag(
    page,
    page.getByTestId("metering-overlay"),
    { x: 0.2, y: 0.2 },
    { x: 0.5, y: 0.5 },
  );
  await page.getByRole("radio", { name: "Focus" }).check();
  await drag(
    page,
    page.getByTestId("focus-overlay"),
    { x: 0.3, y: 0.3 },
    { x: 0.6, y: 0.6 },
  );
  await page.getByRole("radio", { name: "White balance" }).check();
  await drag(
    page,
    page.getByTestId("awb-overlay"),
    { x: 0.4, y: 0.4 },
    { x: 0.7, y: 0.7 },
  );
  await expect.poll(async () => regionOf(page, "ae_regions")).not.toBeNull();
  await expect.poll(async () => regionOf(page, "af_regions")).not.toBeNull();
  await expect.poll(async () => regionOf(page, "awb_regions")).not.toBeNull();

  await page.getByTestId("clear-regions").click();
  await expect.poll(async () => (await state(page)).ae_regions?.actual).toBeUndefined();
  await expect.poll(async () => (await state(page)).af_regions?.actual).toBeUndefined();
  await expect.poll(async () => (await state(page)).awb_regions?.actual).toBeUndefined();
  for (const id of ["metering-overlay", "focus-overlay", "awb-overlay"]) {
    await expect(page.getByTestId(id).locator("rect.region")).toHaveCount(0);
  }
  await expect(page.getByTestId("region-readout")).toHaveText("No regions set");
});
