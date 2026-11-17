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
async function routeHost(page: Page, token?: string): Promise<void> {
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

/** One host settings payload. */
function hostSettings() {
  return {
    ports: { ui: 7710, mcp: 7711, api: 7712 },
    listen: {
      mode: "mixed",
      addresses: { ui: "0.0.0.0", mcp: "0.0.0.0", api: "127.0.0.1" },
    },
    token: { configured: true, hint: "…c0de" },
    storage: {
      data_dir: "C:\\data\\watchdog",
      media_dir: "C:\\data\\watchdog\\media",
      logs_dir: "C:\\data\\watchdog\\logs",
      media_items: 12,
      media_bytes: 104_857_600,
      free_bytes: 64 * 1024 ** 3,
      total_bytes: 256 * 1024 ** 3,
    },
    retention: { max_age_days: 7, max_size_mb: 2048 },
    ffmpeg: {
      available: true,
      ffmpeg_version: "7.1",
      ffprobe_version: "7.1",
      path: "C:\\data\\watchdog\\tools\\ffmpeg.exe",
      license: "LGPL-2.1-or-later",
      filters: 418,
    },
    about: {
      version: "0.1.0",
      commit: "ceda15c",
      build: "2026-11-17T18:00:00Z",
      os: "windows",
      arch: "x86_64",
      protocol: 1,
    },
  };
}

test("routes to the settings page and back to live", async ({ page }) => {
  await page.goto("/#/settings");

  await expect(page.locator("#view-settings")).toBeVisible();
  await expect(page.locator("#view-live")).toBeHidden();
  await expect(page.locator('a[data-route="settings"]')).toHaveAttribute(
    "aria-current",
    "page",
  );

  await page.locator('a[data-route="live"]').click();
  await expect(page.locator("#view-live")).toBeVisible();
  await expect(page.locator("#view-settings")).toBeHidden();
});

test("renders demo ports, storage, ffmpeg, logs and about", async ({
  page,
}) => {
  await page.goto("/#/settings");

  await expect(page.getByTestId("settings-mode")).toHaveText("demo settings");
  await expect(page.getByTestId("settings-mode")).toHaveAttribute(
    "data-state",
    "demo",
  );
  await expect(page.getByTestId("settings-ui-port")).toHaveText("7700");
  await expect(page.getByTestId("settings-mcp-port")).toHaveText("7701");
  await expect(page.getByTestId("settings-api-port")).toHaveText("7702");
  await expect(page.getByTestId("settings-listen-mode")).toHaveText(
    "localhost",
  );
  await expect(page.getByTestId("settings-ui-listen")).toHaveText("127.0.0.1");
  await expect(page.getByTestId("settings-base-url")).toHaveText(
    "http://127.0.0.1:7700/",
  );
  await expect(page.getByTestId("settings-token-state")).toHaveText(
    "not configured",
  );
  await expect(page.getByTestId("settings-token-warning")).toBeHidden();

  await expect(page.getByTestId("settings-data-dir")).toHaveText("demo/data");
  await expect(page.getByTestId("settings-media-items")).toHaveText("3");
  await expect(page.getByTestId("settings-media-size")).toContainText("MiB");
  await expect(page.getByTestId("settings-disk-free")).toContainText("GiB");
  await expect(page.getByTestId("settings-disk-total")).toHaveText("512 GiB");
  await expect(page.getByTestId("settings-disk-meter")).toHaveAttribute(
    "aria-label",
    /Disk usage: \d+% used/,
  );

  await expect(page.getByTestId("settings-retention-age")).toHaveValue("30");
  await expect(page.getByTestId("settings-retention-size")).toHaveValue(
    "10240",
  );

  await expect(page.getByTestId("settings-ffmpeg-state")).toHaveText(
    "available",
  );
  await expect(page.getByTestId("settings-ffmpeg-version")).toHaveText(
    "7.1 (demo)",
  );
  await expect(page.getByTestId("settings-ffmpeg-license")).toHaveText(
    "LGPL-2.1-or-later",
  );
  await expect(page.getByTestId("settings-ffmpeg-filters")).toHaveText("418");

  await expect(page.getByTestId("settings-log-line")).toHaveCount(13);
  await expect(page.getByTestId("settings-log-count")).toHaveText("13 lines");
  await expect(page.getByTestId("settings-log-list")).toContainText(
    "watchdog 0.1.0 starting",
  );
  await expect(page.getByTestId("settings-log-list")).toContainText(
    "no analyzer engine yet",
  );

  await expect(page.getByTestId("settings-version")).toHaveText("0.1.0-demo");
  await expect(page.getByTestId("settings-platform")).toHaveText(
    "demo · wasm",
  );
  await expect(page.getByTestId("settings-protocol")).toHaveText("v1");
  await expect(page.getByTestId("settings-ui-version")).toHaveText("0.1.0");
  await expect(page.getByTestId("settings-status")).toHaveText(
    "Loaded demo settings.",
  );
});

test("saves retention and keeps it across pages", async ({ page }) => {
  await page.goto("/#/settings");
  await expect(page.getByTestId("settings-retention-age")).toHaveValue("30");

  await page.getByTestId("settings-retention-age").fill("14");
  await page.getByTestId("settings-retention-size").fill("2048");
  await page.getByTestId("settings-retention-save").click();
  await expect(page.getByTestId("settings-status")).toHaveText(
    "Retention saved: 14 days, 2048 MiB.",
  );

  await page.getByTestId("settings-log-refresh").click();
  await expect(page.getByTestId("settings-log-list")).toContainText(
    "retention updated: 14 days, 2048 MiB",
  );

  await page.locator('a[data-route="media"]').click();
  await page.locator('a[data-route="settings"]').click();
  await expect(page.getByTestId("settings-retention-age")).toHaveValue("14");
  await expect(page.getByTestId("settings-retention-size")).toHaveValue("2048");
});

test("filters logs and picks up capture events", async ({ page }) => {
  await page.goto("/#/settings");
  await expect(page.getByTestId("settings-log-line")).toHaveCount(13);

  await page.getByTestId("settings-log-level").selectOption("error");
  await expect(page.getByTestId("settings-log-line")).toHaveCount(1);
  await expect(page.getByTestId("settings-log-count")).toHaveText("1 line");
  await expect(page.getByTestId("settings-log-list")).toContainText(
    "no analyzer engine yet",
  );

  await page.getByTestId("settings-log-level").selectOption("debug");
  await expect(page.getByTestId("settings-log-line")).toHaveCount(2);
  await expect(page.getByTestId("settings-log-list")).toContainText(
    "preset 'OTT baseline' loaded",
  );

  await page.getByTestId("settings-log-level").selectOption("all");
  await page.locator('a[data-route="capture"]').click();
  await page.getByTestId("capture-still").click();
  await expect(page.getByTestId("capture-status")).toContainText(
    "Still captured",
  );

  await page.locator('a[data-route="settings"]').click();
  await page.getByTestId("settings-log-refresh").click();
  await expect(page.getByTestId("settings-log-list")).toContainText(
    "event capture_finished",
  );
});

test("offers a demo diagnostics bundle", async ({ page }) => {
  await page.goto("/#/settings");
  const link = page.getByTestId("settings-diagnostics");
  await expect(link).toHaveAttribute("download", "watchdog-diagnostics.json");
  const href = await link.getAttribute("href");
  expect(href).toMatch(/^data:application\/json/);
  const payload = decodeURIComponent(href ?? "");
  expect(payload).toContain('"version": "0.1.0-demo"');
  expect(payload).toContain('"logs"');
  expect(payload).toContain("watchdog 0.1.0 starting");
});

test("uses the host settings and logs APIs with the bearer token", async ({
  page,
}) => {
  await routeHost(page, "tok-123");

  const patchBodies: Array<Record<string, unknown>> = [];
  const authHeaders: Array<string | undefined> = [];
  const logQueries: Array<string> = [];
  let retention = { max_age_days: 7, max_size_mb: 2048 };

  await page.route("**/api/settings", async (route) => {
    const request = route.request();
    authHeaders.push(request.headers()["authorization"]);
    if (request.method() === "PATCH") {
      const body = request.postDataJSON() as Record<string, unknown>;
      patchBodies.push(body);
      const patch = body["retention"] as Record<string, number>;
      retention = {
        max_age_days: patch["max_age_days"] ?? retention.max_age_days,
        max_size_mb: patch["max_size_mb"] ?? retention.max_size_mb,
      };
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ retention }),
      });
      return;
    }
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({ ...hostSettings(), retention }),
    });
  });
  await page.route("**/api/logs**", async (route) => {
    const request = route.request();
    authHeaders.push(request.headers()["authorization"]);
    logQueries.push(new URL(request.url()).search);
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        lines: [
          {
            ts_ms: 1_700_000_000_000,
            level: "warn",
            target: "stream",
            message: "late frame dropped",
          },
          {
            ts_ms: 1_700_000_001_000,
            level: "error",
            target: "analysis",
            message: "engine missing",
          },
        ],
      }),
    });
  });

  await page.goto("/#/settings");
  await expect(page.getByTestId("settings-mode")).toHaveText("host settings");
  await expect(page.getByTestId("settings-ui-port")).toHaveText("7710");
  await expect(page.getByTestId("settings-listen-mode")).toHaveText("LAN");
  await expect(page.getByTestId("settings-listen-mode")).toHaveAttribute(
    "data-state",
    "warn",
  );
  await expect(page.getByTestId("settings-ui-listen")).toHaveText("0.0.0.0");
  await expect(page.getByTestId("settings-token-state")).toHaveText(
    "configured",
  );
  await expect(page.getByTestId("settings-token-hint")).toHaveText("…c0de");
  await expect(page.getByTestId("settings-token-warning")).toBeHidden();
  await expect(page.getByTestId("settings-data-dir")).toHaveText(
    "C:\\data\\watchdog",
  );
  await expect(page.getByTestId("settings-media-size")).toHaveText("100 MiB");
  await expect(page.getByTestId("settings-ffmpeg-path")).toContainText(
    "ffmpeg.exe",
  );
  await expect(page.getByTestId("settings-version")).toHaveText("0.1.0");
  await expect(page.getByTestId("settings-commit")).toHaveText("ceda15c");
  await expect(page.getByTestId("settings-platform")).toHaveText(
    "windows · x86_64",
  );
  await expect(page.getByTestId("settings-log-line")).toHaveCount(2);
  await expect(page.getByTestId("settings-log-list")).toContainText(
    "engine missing",
  );
  await expect(page.getByTestId("settings-diagnostics")).toHaveAttribute(
    "href",
    /\/api\/diagnostics\?token=tok-123$/,
  );
  await expect(page.getByTestId("settings-diagnostics")).toHaveAttribute(
    "download",
    "watchdog-diagnostics.zip",
  );

  await page.getByTestId("settings-retention-age").fill("5");
  await page.getByTestId("settings-retention-size").fill("1024");
  await page.getByTestId("settings-retention-save").click();
  await expect(page.getByTestId("settings-status")).toHaveText(
    "Retention saved: 5 days, 1024 MiB.",
  );
  expect(patchBodies).toEqual([
    { retention: { max_age_days: 5, max_size_mb: 1024 } },
  ]);

  await page.getByTestId("settings-log-level").selectOption("error");
  await expect.poll(() => logQueries.at(-1)).toContain("level=error");
  expect(logQueries.at(-1)).toContain("limit=200");
  expect(authHeaders).toContain("Bearer tok-123");
});

test("shows a clear notice when the host settings routes are missing", async ({
  page,
}) => {
  await routeHost(page);
  await page.route("**/api/settings", async (route) => {
    await route.fulfill({
      status: 404,
      contentType: "application/json",
      body: JSON.stringify({
        error: { code: "NOT_FOUND", message: "request failed" },
      }),
    });
  });

  await page.goto("/#/settings");
  await expect(page.getByTestId("settings-mode")).toHaveText("host settings");
  await expect(page.getByTestId("settings-notice")).toBeVisible();
  await expect(page.getByTestId("settings-notice")).toContainText(
    "The host settings API is not available yet (M7/M9).",
  );
  await expect(page.getByTestId("settings-notice")).toContainText(
    "the host has no settings route yet",
  );
  await expect(page.getByTestId("settings-status")).toHaveText(
    "Settings unavailable.",
  );
});
