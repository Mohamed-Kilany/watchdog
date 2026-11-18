import { expect, test as base } from "@playwright/test";
import type { APIRequestContext, Locator, Page } from "@playwright/test";

/** Origin of the E2E host (see `playwright.host.config.ts`). */
export const E2E_BASE_URL = "http://127.0.0.1:7717";
/** Bearer token configured for the E2E host (`--token`). */
export const TOKEN = "e2e-token-1234";
/** Serial of the synthetic device activated by `serve --mock-device`. */
export const MOCK_SERIAL = "MOCK-0001";
/** Header form of the bearer token, for direct API assertions. */
export const AUTH_HEADERS = { Authorization: `Bearer ${TOKEN}` };

/** Routes wired by the app shell. */
export type Route = "live" | "capture" | "media" | "settings" | "devices";

interface DeviceListBody {
  session?: { active?: boolean };
}

/**
 * Re-open the mock session when a previous test (or retry) disconnected it.
 *
 * Tests share one host process, so device and media state is deliberately
 * shared; this keeps the suite independent of test order and retries.
 */
export async function ensureMockActive(
  request: APIRequestContext,
): Promise<void> {
  const response = await request.get("/api/devices", {
    headers: AUTH_HEADERS,
  });
  expect(response.ok(), "GET /api/devices").toBeTruthy();
  const body = (await response.json()) as DeviceListBody;
  if (body.session?.active) return;
  const activated = await request.post("/api/devices/activate", {
    headers: AUTH_HEADERS,
    data: { serial: MOCK_SERIAL },
  });
  expect(activated.ok(), "POST /api/devices/activate").toBeTruthy();
}

/** Seed the tab token and wait for the shell to swap in the host backend. */
export async function bootHost(page: Page): Promise<void> {
  await page.addInitScript((token) => {
    try {
      window.sessionStorage.setItem("watchdog.token", token);
    } catch {
      // The URL token path is covered separately; storage should work here.
    }
  }, TOKEN);
  await page.goto("/");
  await expect(page.locator("#mode-tag")).toHaveText("host", {
    timeout: 20_000,
  });
}

/** Click one nav link and wait for its view. */
export async function openRoute(page: Page, route: Route): Promise<void> {
  await page.locator(`a[data-route="${route}"]`).click();
  await expect(page.locator(`#view-${route}`)).toBeVisible();
}

/** Open the live page and wait for the host stream to report live. */
export async function openLive(page: Page): Promise<void> {
  await openRoute(page, "live");
  await expect(page.locator("#viewfinder")).toHaveAttribute(
    "data-state",
    "live",
    { timeout: 30_000 },
  );
}

/**
 * Capture one still through the UI and return its media id.
 *
 * The mock device answers synchronously, so the status is the source of truth
 * for the id; the transfer badge is asserted separately.
 */
export async function captureStill(page: Page): Promise<string> {
  await openRoute(page, "capture");
  await page.getByTestId("capture-still").click();
  const status = page.getByTestId("capture-status");
  await expect(status).toHaveAttribute("data-state", "ok");
  await expect(status).toContainText("Still captured: media");
  const text = (await status.textContent()) ?? "";
  const id = /Still captured: media (m-mock-still-\d+)/.exec(text)?.[1];
  if (!id) throw new Error(`no media id in capture status: ${text}`);
  return id;
}

/** Wait for the host transfer for one media id to reach `done`. */
export async function expectTransferDone(
  page: Page,
  mediaId: string,
): Promise<void> {
  const item = page.locator('[data-testid="transfer-item"]', {
    hasText: mediaId,
  });
  await expect(item).toHaveAttribute("data-state", "done", {
    timeout: 20_000,
  });
}

/** Card locator for one media id. */
export function mediaCard(page: Page, mediaId: string): Locator {
  return page.locator(`li.media-card[data-media-id="${mediaId}"]`);
}

/** Number of items reported by the host media API. */
export async function mediaCount(
  request: APIRequestContext,
  type?: "still" | "video",
): Promise<number> {
  const response = await request.get("/api/media", {
    headers: AUTH_HEADERS,
  });
  expect(response.ok(), "GET /api/media").toBeTruthy();
  const body = (await response.json()) as {
    items?: Array<{ type?: string }>;
  };
  const items = body.items ?? [];
  return type ? items.filter((item) => item.type === type).length : items.length;
}

/** A page that already booted the host backend with the mock session open. */
export const test = base.extend<{ hostPage: Page }>({
  hostPage: async ({ page, request }, use) => {
    await ensureMockActive(request);
    await bootHost(page);
    await use(page);
  },
});

export { expect };
