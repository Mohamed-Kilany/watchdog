import { expect, test, type Page } from "@playwright/test";
import type { ControlState } from "../src/types";

async function state(page: Page): Promise<ControlState> {
  return page.evaluate(() => window.__watchdog.state());
}

test.beforeEach(async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("panel")).toBeVisible();
});

test("slider numeric entry commits, clamps and updates the actual marker", async ({
  page,
}) => {
  const slider = page.locator('wd-slider[data-control-id="iso"]');
  const number = slider.locator('input[type="number"]');
  const range = slider.locator('input[type="range"]');
  const actual = page.locator('wd-actual[data-control-id="iso"]');

  await expect(number).toBeDisabled();
  await page.locator('wd-auto-toggle[data-control-id="ae_mode"] button').click();
  await expect(number).toBeEnabled();

  await number.fill("800");
  await number.press("Enter");
  await expect.poll(async () => (await state(page)).iso?.requested).toBe(800);
  await expect(number).toHaveValue("800");
  await expect(range).toHaveValue("800");
  await expect(actual.locator(".badge")).toHaveText("match");
  await expect(slider.locator(".actual-readout")).toHaveText("actual 800 ISO");
  await expect(slider.locator(".actual-marker")).toBeVisible();

  await number.fill("99999");
  await number.press("Enter");
  await expect.poll(async () => (await state(page)).iso?.requested).toBe(3200);
  await expect(number).toHaveValue("3200");

  await range.focus();
  await range.press("ArrowLeft");
  await expect.poll(async () => (await state(page)).iso?.requested).toBe(3150);
});

test("scaled slider converts milliseconds to device nanoseconds", async ({
  page,
}) => {
  await page.locator('wd-auto-toggle[data-control-id="ae_mode"] button').click();
  const exposure = page.locator('wd-slider[data-control-id="exposure_time_ns"]');
  const number = exposure.locator('input[type="number"]');
  const actual = page.locator('wd-actual[data-control-id="exposure_time_ns"]');

  await number.fill("25");
  await number.press("Enter");
  await expect
    .poll(async () => (await state(page)).exposure_time_ns?.requested)
    .toBe(25_000_000);
  await expect(number).toHaveValue("25");
  await expect(actual.locator(".actual-value")).toHaveText("25 ms");
  await expect(actual.locator(".badge")).toHaveText("match");
});

test("auto toggle drives manual exposure and disables manual sliders", async ({
  page,
}) => {
  const toggle = page.locator('wd-auto-toggle[data-control-id="ae_mode"]');
  const button = toggle.locator("button");
  const iso = page.locator('wd-slider[data-control-id="iso"]');
  const actual = page.locator('wd-actual[data-control-id="iso"]');

  await expect(button).toHaveAttribute("aria-checked", "true");
  await expect(iso.locator('input[type="number"]')).toBeDisabled();

  await button.click();
  await expect(button).toHaveAttribute("aria-checked", "false");
  await expect(iso.locator('input[type="number"]')).toBeEnabled();
  await expect.poll(async () => (await state(page)).ae_mode?.actual).toBe(0);
  await expect.poll(async () => (await state(page)).iso?.requested).toBe(100);

  await button.click();
  await expect(button).toHaveAttribute("aria-checked", "true");
  await expect(iso.locator('input[type="number"]')).toBeDisabled();
  await expect.poll(async () => (await state(page)).ae_mode?.actual).toBe(1);
  await expect(actual.locator(".badge")).toHaveText("auto");
});

test("lock button reflects live state, pending state and keyboard activation", async ({
  page,
}) => {
  const lock = page.locator('wd-lock-button[data-control-id="awb_lock"]');
  const button = lock.locator("button");
  const badge = lock.locator(".badge");

  await expect(badge).toHaveText("Unlocked");
  await button.click();
  await expect(button).toHaveAttribute("aria-pressed", "true");
  await expect(badge).toHaveText("Locked");
  await expect.poll(async () => (await state(page)).awb_lock?.actual).toBe(true);
  await expect.poll(async () => (await state(page)).awb_state?.actual).toBe(
    "locked",
  );

  await page.evaluate(() => {
    const panel = window.__watchdog.panel;
    const snapshot = structuredClone(window.__watchdog.state());
    snapshot.awb_lock = {
      supported: true,
      requested: true,
      actual: false,
      source: "host",
    };
    panel.state = snapshot;
  });
  await expect(badge).toHaveText("Pending");

  await page.evaluate(() => {
    window.__watchdog.panel.state = window.__watchdog.state();
  });
  await button.focus();
  await page.keyboard.press("Space");
  await expect.poll(async () => (await state(page)).awb_lock?.actual).toBe(false);
  await expect(badge).toHaveText("Unlocked");
});

test("reset-to-auto restores a group and clears manual requests", async ({
  page,
}) => {
  await page.locator('wd-auto-toggle[data-control-id="ae_mode"] button').click();
  const iso = page.locator('wd-slider[data-control-id="iso"] input[type="number"]');
  await iso.fill("800");
  await iso.press("Enter");
  await page.locator('wd-lock-button[data-control-id="ae_lock"] button').click();
  await expect.poll(async () => (await state(page)).ae_lock?.actual).toBe(true);

  await page.locator('wd-reset[data-reset-group="Exposure"] button').click();
  await expect.poll(async () => (await state(page)).ae_mode?.actual).toBe(1);
  await expect.poll(async () => (await state(page)).ae_lock?.actual).toBe(false);
  const snapshot = await state(page);
  expect(snapshot.iso?.requested).toBeUndefined();
  expect(snapshot.iso?.source).toBe("auto");
  await expect(
    page.locator('wd-auto-toggle[data-control-id="ae_mode"] button'),
  ).toHaveAttribute("aria-checked", "true");
  await expect(
    page.locator('wd-slider[data-control-id="iso"] input[type="number"]'),
  ).toBeDisabled();
  await expect(page.locator('wd-actual[data-control-id="iso"] .badge')).toHaveText(
    "auto",
  );
});

test("select commits enum values and shows requested vs actual", async ({
  page,
}) => {
  const antiBanding = page.locator(
    'wd-select[data-control-id="anti_banding"] select',
  );
  await antiBanding.selectOption({ label: "Off" });
  await expect
    .poll(async () => (await state(page)).anti_banding?.requested)
    .toBe(0);
  await expect(
    page.locator('wd-actual[data-control-id="anti_banding"] .badge'),
  ).toHaveText("match");
  await expect(page.getByTestId("command-log")).toContainText(
    "anti_banding=0",
  );

  const awb = page.locator('wd-select[data-control-id="awb_mode"] select');
  await awb.selectOption({ label: "Off" });
  await expect.poll(async () => (await state(page)).awb_mode?.actual).toBe(0);
  await expect.poll(async () => (await state(page)).awb_state?.actual).toBe(
    "locked",
  );
});

test("actual-vs-requested flags device drift and reset restores auto", async ({
  page,
}) => {
  await page.locator('wd-auto-toggle[data-control-id="ae_mode"] button').click();
  const isoNumber = page.locator(
    'wd-slider[data-control-id="iso"] input[type="number"]',
  );
  await isoNumber.fill("400");
  await isoNumber.press("Enter");
  const exposureNumber = page.locator(
    'wd-slider[data-control-id="exposure_time_ns"] input[type="number"]',
  );
  await exposureNumber.fill("20");
  await exposureNumber.press("Enter");
  await page.locator("#drift").click();

  const isoActual = page.locator('wd-actual[data-control-id="iso"]');
  await expect(isoActual.locator(".requested")).toHaveText("400 ISO");
  await expect(isoActual.locator(".actual-value")).toHaveText("640 ISO");
  await expect(isoActual.locator(".badge")).toHaveText("differs");

  const exposureActual = page.locator(
    'wd-actual[data-control-id="exposure_time_ns"]',
  );
  await expect(exposureActual.locator(".actual-value")).toHaveText("30 ms");
  await expect(exposureActual.locator(".badge")).toHaveText("differs");

  await page.locator('wd-reset[data-reset-group="Exposure"] button').click();
  await expect(isoActual.locator(".badge")).toHaveText("auto");
  await expect(isoActual.locator(".actual-value")).toHaveText("100 ISO");
});

test("controls expose accessible roles and names", async ({ page }) => {
  await expect(
    page.getByRole("switch", { name: "Auto exposure: automatic" }),
  ).toBeVisible();
  await expect(page.getByRole("slider", { name: "ISO" })).toBeVisible();
  await expect(page.getByRole("spinbutton", { name: "ISO" })).toBeVisible();
  await expect(
    page.getByRole("button", { name: "AE lock: unlocked" }),
  ).toBeVisible();
  await expect(
    page.getByRole("combobox", { name: "Anti-banding" }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Reset exposure" }),
  ).toBeVisible();
});
