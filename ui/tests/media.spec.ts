import { expect, test, type Page } from "@playwright/test";

/** A state envelope with the fields the shell and panels read. */
function hostState() {
  return {
    type: "state",
    seq: 1,
    timestamp_ms: 1_000,
    camera_id: "0",
    state: {
      ae_mode: { supported: true, requested: 1, actual: 1, source: "host" },
      ae_lock: {
        supported: true,
        requested: false,
        actual: false,
        source: "host",
        locked: false,
      },
      iso: { supported: true, actual: 100, source: "auto" },
      exposure_time_ns: {
        supported: true,
        actual: 16_666_666,
        source: "auto",
      },
      frame_duration_ns: {
        supported: true,
        actual: 33_333_000,
        source: "auto",
      },
      ae_compensation: {
        supported: true,
        requested: 0,
        actual: 0,
        source: "host",
      },
      anti_banding: { supported: true, requested: 3, actual: 3, source: "host" },
      af_mode: { supported: true, requested: 4, actual: 4, source: "host" },
      af_lock: {
        supported: true,
        requested: false,
        actual: false,
        source: "host",
        locked: false,
      },
      focus_distance: { supported: true, actual: 2.5, source: "auto" },
      af_state: { supported: true, actual: "passive_focused", source: "device" },
      af_regions: { supported: true },
      af_trigger: { supported: true },
      ae_regions: { supported: true },
      awb_mode: { supported: true, requested: 1, actual: 1, source: "host" },
      awb_lock: {
        supported: true,
        requested: false,
        actual: false,
        source: "host",
        locked: false,
      },
      awb_state: { supported: true, actual: "converged", source: "device" },
      zoom_ratio: { supported: true, requested: 1, actual: 1, source: "host" },
      zoom_actual: { supported: true, actual: 1, source: "device" },
    },
    dropped_frames: 0,
    storage_free_mb: 2048,
  };
}

/** Boot the shell in host mode with a mock real-time channel. */
async function routeMediaHost(page: Page, token?: string): Promise<void> {
  const state = hostState();
  if (token) {
    await page.addInitScript((value) => {
      sessionStorage.setItem("watchdog.token", value);
    }, token);
  }
  await page.route("**/api/state", async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify(state),
    });
  });
  await page.route("**/api/stats", async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        frames: 0,
        dropped: 0,
        duplicates: 0,
        fps: 0,
        bitrate_kbps: 0,
        latency_ms: 0,
        rtt_ms: 0,
        last_seq: 0,
        subscribers: 1,
        closed: false,
      }),
    });
  });
  await page.routeWebSocket("**/ws*", (socket) => {
    socket.send(JSON.stringify({ type: "hello", protocol: 1 }));
    socket.send(JSON.stringify(state));
  });
}

/** One still returned by the mocked host media API. */
function hostItem() {
  return {
    id: "m-host-1",
    type: "still",
    created_ms: 1_700_000_000_000,
    width: 4000,
    height: 3000,
    bytes: 2_000_000,
    sha256: "a".repeat(64),
    mime: "image/jpeg",
    device: { model: "Pixel 7 Pro", serial: "HOST-1", sdk: 34 },
    control_snapshot: { iso: 200, awb_mode: 1, zoom_actual: 2 },
    host_job_id: "job-9",
  };
}

/** 1x1 transparent GIF used for mocked media files. */
const GIF_BYTES = Buffer.from(
  "R0lGODlhAQABAIAAAAAAAP///ywAAAAAAQABAAACAUwAOw==",
  "base64",
);

test("routes to the media page and back to live", async ({ page }) => {
  await page.goto("/#/media");

  await expect(page.locator("#view-media")).toBeVisible();
  await expect(page.locator("#view-live")).toBeHidden();
  await expect(page.locator('a[data-route="media"]')).toHaveAttribute(
    "aria-current",
    "page",
  );

  await page.locator('a[data-route="live"]').click();
  await expect(page.locator("#view-live")).toBeVisible();
  await expect(page.locator("#view-media")).toBeHidden();
});

test("lists, filters, sorts and switches the demo library layout", async ({
  page,
}) => {
  await page.goto("/#/media");
  await expect(page.getByTestId("media-mode")).toHaveText("demo library");
  await expect(page.getByTestId("media-card")).toHaveCount(3);
  await expect(page.getByTestId("media-count")).toHaveText("3 of 3 items");
  await expect(page.getByTestId("media-grid")).toHaveAttribute(
    "data-view",
    "grid",
  );

  await page.getByTestId("media-filter-type").selectOption("video");
  await expect(page.getByTestId("media-card")).toHaveCount(1);
  await expect(page.getByTestId("media-card")).toHaveAttribute(
    "data-media-id",
    "m-demo-video-1",
  );
  await expect(page.getByTestId("media-count")).toHaveText("1 of 3 items");

  await page.getByTestId("media-filter-type").selectOption("all");
  await page.getByTestId("media-filter-device").selectOption("DEMO-0002");
  await expect(page.getByTestId("media-card")).toHaveCount(1);
  await expect(page.getByTestId("media-card")).toHaveAttribute(
    "data-media-id",
    "m-demo-still-2",
  );

  await page.getByTestId("media-filter-device").selectOption("all");
  await page.getByTestId("media-search").fill("webm");
  await expect(page.getByTestId("media-card")).toHaveCount(1);
  await expect(page.getByTestId("media-card")).toHaveAttribute(
    "data-media-id",
    "m-demo-video-1",
  );

  await page.getByTestId("media-search").fill("no-such-item");
  await expect(page.getByTestId("media-card")).toHaveCount(0);
  await expect(page.getByTestId("media-empty")).toBeVisible();
  await expect(page.getByTestId("media-empty")).toContainText(
    "No media matches the current filters.",
  );

  await page.getByTestId("media-search").fill("");
  await page.getByTestId("media-sort").selectOption("oldest");
  await expect(page.getByTestId("media-card").first()).toHaveAttribute(
    "data-media-id",
    "m-demo-still-2",
  );

  await page.getByTestId("media-sort").selectOption("largest");
  await expect(page.getByTestId("media-card").first()).toHaveAttribute(
    "data-media-id",
    "m-demo-video-1",
  );

  await page.getByTestId("media-view-list").click();
  await expect(page.getByTestId("media-grid")).toHaveAttribute(
    "data-view",
    "list",
  );
  await expect(page.getByTestId("media-view-list")).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  await page.getByTestId("media-view-grid").click();
  await expect(page.getByTestId("media-grid")).toHaveAttribute(
    "data-view",
    "grid",
  );
  await expect(page.getByTestId("media-view-grid")).toHaveAttribute(
    "aria-pressed",
    "true",
  );

  await page.getByTestId("media-select-all").click();
  await expect(page.getByTestId("media-selection")).toBeVisible();
  await expect(page.getByTestId("media-selected-count")).toHaveText(
    "3 selected",
  );
  await page.getByTestId("media-select-clear").click();
  await expect(page.getByTestId("media-selection")).toBeHidden();
});

test("previews a still with zoom, metadata, snapshot and download", async ({
  page,
}) => {
  await page.goto("/#/media");

  await page
    .locator('[data-media-id="m-demo-still-1"] [data-testid="media-open"]')
    .click();
  await expect(page.getByTestId("media-dialog")).toBeVisible();
  await expect(page.getByTestId("media-dialog-title")).toHaveText(
    "still m-demo-still-1",
  );
  await expect(page.getByTestId("media-image")).toBeVisible();
  await expect(page.getByTestId("media-video")).toBeHidden();

  const metadata = page.getByTestId("media-metadata");
  await expect(metadata).toContainText("1920×1080");
  await expect(metadata).toContainText("image/jpeg");
  await expect(metadata).toContainText("Pixel 7 (demo) · DEMO-0001");
  await expect(metadata).toContainText("SHA-256");
  await expect(metadata).toContainText(/[0-9a-f]{16,}/);
  await expect(page.getByTestId("media-snapshot-json")).toContainText(
    '"iso": 400',
  );

  await expect(page.getByTestId("media-zoom-level")).toHaveText("100%");
  await page.getByTestId("media-zoom-in").click();
  await expect(page.getByTestId("media-zoom-level")).toHaveText("125%");
  await expect(page.getByTestId("media-image")).toHaveCSS(
    "transform",
    "matrix(1.25, 0, 0, 1.25, 0, 0)",
  );
  await page.getByTestId("media-zoom-reset").click();
  await expect(page.getByTestId("media-zoom-level")).toHaveText("100%");

  await expect(page.getByTestId("media-download")).toHaveAttribute(
    "href",
    /^data:image\/jpeg/,
  );
  await expect(page.getByTestId("media-download")).toHaveAttribute(
    "download",
    "m-demo-still-1.jpg",
  );

  await page.getByTestId("media-close").click();
  await expect(page.getByTestId("media-dialog")).toBeHidden();
});

test("plays the demo video in the preview", async ({ page }) => {
  await page.goto("/#/media");

  await page
    .locator('[data-media-id="m-demo-video-1"] [data-testid="media-open"]')
    .click();
  await expect(page.getByTestId("media-dialog-title")).toHaveText(
    "video m-demo-video-1",
  );
  await expect(page.getByTestId("media-video")).toBeVisible();
  await expect(page.getByTestId("media-image")).toBeHidden();
  await expect(page.getByTestId("media-video")).toHaveAttribute(
    "src",
    /demo-clip.*\.webm$/,
  );
  await expect(page.getByTestId("media-video")).toHaveAttribute(
    "poster",
    /^data:image\/svg\+xml/,
  );
  await expect(page.getByTestId("media-metadata")).toContainText("00:12");
  await expect(page.getByTestId("media-metadata")).toContainText("1920×1080");

  await page.getByTestId("media-close").click();
  await expect(page.getByTestId("media-dialog")).toBeHidden();
});

test("multi-selects media, sends it to analysis and deletes it", async ({
  page,
}) => {
  await page.goto("/#/media");
  await expect(page.getByTestId("media-card")).toHaveCount(3);

  await page.getByTestId("media-select").nth(0).check();
  await page.getByTestId("media-select").nth(1).check();
  await expect(page.getByTestId("media-selection")).toBeVisible();
  await expect(page.getByTestId("media-selected-count")).toHaveText(
    "2 selected",
  );

  await page.getByTestId("media-analyze-selected").click();
  await expect(page.getByTestId("media-status")).toContainText(
    "Analysis queued: an-demo-",
  );
  await expect(page.getByTestId("media-status")).toContainText("2 items");

  await page.getByTestId("media-delete-selected").click();
  await expect(page.getByTestId("media-status")).toContainText(
    "Deleted 2 items.",
  );
  await expect(page.getByTestId("media-card")).toHaveCount(1);
  await expect(page.getByTestId("media-selection")).toBeHidden();
});

test("adds captured stills to the demo media library", async ({ page }) => {
  await page.goto("/#/capture");
  await page.getByTestId("capture-still").click();
  await expect(page.getByTestId("capture-status")).toContainText(
    "Still captured",
  );

  await page.locator('a[data-route="media"]').click();
  await expect(page.getByTestId("media-card")).toHaveCount(4);
  const captured = page.locator(
    '[data-testid="media-card"][data-media-id^="m-still-"]',
  );
  await expect(captured).toHaveCount(1);

  await captured.locator('[data-testid="media-open"]').click();
  await expect(page.getByTestId("media-metadata")).toContainText("4000×3000");
  await expect(page.getByTestId("media-snapshot-json")).toContainText(
    '"iso"',
  );
});

test("uses the host media API with the stored bearer token", async ({
  page,
}) => {
  await routeMediaHost(page, "tok-123");

  let deleted = false;
  const analysisBodies: Array<Record<string, unknown>> = [];
  const authHeaders: Array<string | undefined> = [];
  await page.route("**/api/media**", async (route) => {
    const request = route.request();
    const path = new URL(request.url()).pathname;
    if (path === "/api/media" && request.method() === "GET") {
      authHeaders.push(request.headers()["authorization"]);
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ items: deleted ? [] : [hostItem()] }),
      });
      return;
    }
    if (path === "/api/media/m-host-1" && request.method() === "GET") {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ item: hostItem() }),
      });
      return;
    }
    if (path === "/api/media/m-host-1" && request.method() === "DELETE") {
      deleted = true;
      await route.fulfill({ status: 204, body: "" });
      return;
    }
    if (path === "/api/media/m-host-1/file") {
      await route.fulfill({
        status: 200,
        contentType: "image/gif",
        body: GIF_BYTES,
      });
      return;
    }
    await route.fulfill({
      status: 404,
      contentType: "application/json",
      body: JSON.stringify({
        error: { code: "NOT_FOUND", message: "request failed" },
      }),
    });
  });
  await page.route("**/api/analyses", async (route) => {
    analysisBodies.push(
      route.request().postDataJSON() as Record<string, unknown>,
    );
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({ analysis_id: "an-host-1", state: "queued" }),
    });
  });

  await page.goto("/#/media");
  await expect(page.getByTestId("media-mode")).toHaveText("host library");
  await expect(page.getByTestId("media-mode")).toHaveAttribute(
    "data-state",
    "host",
  );
  await expect(page.getByTestId("media-card")).toHaveCount(1);

  await page.getByTestId("media-open").click();
  await expect(page.getByTestId("media-dialog-title")).toHaveText(
    "still m-host-1",
  );
  await expect(page.getByTestId("media-metadata")).toContainText("4000×3000");
  await expect(page.getByTestId("media-metadata")).toContainText(
    "Pixel 7 Pro · HOST-1",
  );
  await expect(page.getByTestId("media-metadata")).toContainText("job-9");
  await expect(page.getByTestId("media-download")).toHaveAttribute(
    "href",
    /\/api\/media\/m-host-1\/file\?token=tok-123$/,
  );

  await page.getByTestId("media-analyze").click();
  await expect(page.getByTestId("media-status")).toContainText(
    "Analysis queued: an-host-1 (1 item)",
  );
  expect(analysisBodies).toEqual([{ media_ids: ["m-host-1"] }]);

  await page.getByTestId("media-delete").click();
  await expect(page.getByTestId("media-dialog")).toBeHidden();
  await expect(page.getByTestId("media-card")).toHaveCount(0);
  await expect(page.getByTestId("media-status")).toContainText(
    "Deleted 1 item.",
  );
  await expect(page.getByTestId("media-empty")).toContainText(
    "No media yet. Captures appear here after they transfer.",
  );

  expect(deleted).toBe(true);
  expect(authHeaders).toContain("Bearer tok-123");
});

test("shows a clear notice when the host media routes are missing", async ({
  page,
}) => {
  await routeMediaHost(page);
  await page.route("**/api/media**", async (route) => {
    await route.fulfill({
      status: 404,
      contentType: "application/json",
      body: JSON.stringify({
        error: { code: "NOT_FOUND", message: "request failed" },
      }),
    });
  });

  await page.goto("/#/media");
  await expect(page.getByTestId("media-mode")).toHaveText("host library");
  await expect(page.getByTestId("media-notice")).toBeVisible();
  await expect(page.getByTestId("media-notice")).toContainText(
    "The host media library is not available yet (M5).",
  );
  await expect(page.getByTestId("media-notice")).toContainText(
    "the host has no media route yet",
  );
  await expect(page.getByTestId("media-card")).toHaveCount(0);
});
