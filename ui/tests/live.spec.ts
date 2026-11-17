import { expect, test, type Page, type WebSocketRoute } from "@playwright/test";

/** 1x1 white JPEG used as the viewfinder and snapshot body. */
const JPEG = Buffer.from(
  "/9j/4AAQSkZJRgABAQEAYABgAAD/2wBDAAgGBgcGBQgHBwcJCQgKDBQNDAsLDBkSEw8UHRofHh0aHBwgJC4nICIsIxwcKDcpLDAxNDQ0Hyc5PTgyPC4zNDL/2wBDAQkJCQwLDBgNDRgyIRwhMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjL/wAARCAABAAEDASIAAhEBAxEB/8QAHwAAAQUBAQEBAQEAAAAAAAAAAAECAwQFBgcICQoL/8QAtRAAAgEDAwIEAwUFBAQAAAF9AQIDAAQRBRIhMUEGE1FhByJxFDKBkaEII0KxwRVS0fAkM2JyggkKFhcYGRolJicoKSo0NTY3ODk6Q0RFRkdISUpTVFVWV1hZWmNkZWZnaGlqc3R1dnd4eXqDhIWGh4iJipKTlJWWl5iZmqKjpKWmp6ipqrKztLW2t7i5usLDxMXGx8jJytLT1NXW19jZ2uHi4+Tl5ufo6erx8vP09fb3+Pn6/8QAHwEAAwEBAQEBAQEBAQAAAAAAAAECAwQFBgcICQoL/8QAtREAAgECBAQDBAcFBAQAAQJ3AAECAxEEBSExBhJBUQdhcRMiMoEIFEKRobHBCSMzUvAVYnLRChYkNOEl8RcYGRomJygpKjU2Nzg5OkNERUZHSElKU1RVVldYWVpjZGVmZ2hpanN0dXZ3eHl6goOEhYaHiImKkpOUlZaXmJmaoqOkpaanqKmqsrO0tba3uLm6wsPExcbHyMnK0tPU1dbX2Nna4uPk5ebn6Onq8vP09fb3+Pn6/9oADAMBAAIRAxEAPwD3+iiigD//2Q==",
  "base64",
);

function session(active = true) {
  return {
    active,
    closed: false,
    serial: "192.168.93.76:34883",
    device: {
      serial: "192.168.93.76:34883",
      kind: "lan",
      manufacturer: "samsung",
      model: "SM-A515F",
      device: "a51",
      sdk: 33,
      release: "13",
      abi: "arm64-v8a",
      app_package: "app.watchdog.remote",
      app_version_name: "0.1.0",
      app_version_code: 1,
      protocol_version: 1,
      camera_count: 2,
    },
  };
}

/** A state envelope with the fields the live overlays read. */
function hostState(overrides: Record<string, unknown> = {}) {
  return {
    type: "state",
    seq: 7,
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
      exposure_time_ns: { supported: true, actual: 16_666_666, source: "auto" },
      frame_duration_ns: { supported: true, actual: 33_333_000, source: "auto" },
      ae_compensation: { supported: true, requested: 0, actual: 0, source: "host" },
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
      stream_rotation: {
        supported: true,
        requested: 0,
        actual: 0,
        source: "host",
      },
      faces: { supported: true, actual: [], source: "device" },
      ...overrides,
    },
    dropped_frames: 0,
    storage_free_mb: 2048,
    session: session(),
  };
}

interface LiveRoutes {
  stats?: () => Record<string, unknown>;
  state?: Record<string, unknown>;
  socket?: (socket: WebSocketRoute) => void;
}

async function routeLiveHost(page: Page, options: LiveRoutes = {}): Promise<void> {
  const state = options.state ?? hostState();
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
      body: JSON.stringify(
        options.stats?.() ?? {
          frames: 120,
          dropped: 3,
          duplicates: 0,
          fps: 29.9,
          bitrate_kbps: 2450.5,
          latency_ms: 42.4,
          rtt_ms: 5,
          last_seq: 120,
          subscribers: 2,
          closed: false,
          session: session(),
        },
      ),
    });
  });
  await page.route("**/api/snapshot", async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "image/jpeg",
      headers: { "Content-Disposition": 'attachment; filename="watchdog-snapshot.jpg"' },
      body: JPEG,
    });
  });
  await page.route("**/stream.mjpg*", async (route) => {
    await route.fulfill({ status: 200, contentType: "image/jpeg", body: JPEG });
  });
  await page.routeWebSocket("**/ws*", (socket) => {
    if (options.socket) {
      options.socket(socket);
      return;
    }
    socket.send(JSON.stringify({ type: "hello", protocol: 1, session: session() }));
    socket.send(JSON.stringify(state));
  });
}

test("demo viewfinder keeps the synthetic scene and empty stats", async ({
  page,
}) => {
  await page.goto("/");
  await expect(page.getByTestId("viewfinder")).toHaveAttribute(
    "data-state",
    "demo",
  );
  await expect(page.getByTestId("live-status")).toContainText("Demo viewfinder");
  await expect(page.getByTestId("live-stream")).toBeHidden();
  await expect(page.getByTestId("stat-fps")).toHaveText("—");
  await expect(page.getByTestId("snapshot")).toBeDisabled();
  await expect(page.getByTestId("fullscreen")).toBeVisible();
});

test("splits the controls into left and right panels around the video", async ({
  page,
}) => {
  await page.goto("/");
  const left = page.getByTestId("panel");
  const right = page.getByTestId("panel-right");
  await expect(left).toBeVisible();
  await expect(right).toBeVisible();
  await expect(
    left.locator('wd-auto-toggle[data-control-id="ae_mode"]'),
  ).toBeVisible();
  await expect(
    left.locator('wd-select[data-control-id="awb_mode"]'),
  ).toBeVisible();
  await expect(
    right.locator('wd-slider[data-control-id="zoom_ratio"]'),
  ).toBeVisible();
  await expect(
    left.locator('wd-slider[data-control-id="zoom_ratio"]'),
  ).toHaveCount(0);
});

test("streams the host viewfinder and polls the stats", async ({ page }) => {
  let polls = 0;
  await routeLiveHost(page, {
    stats: () => {
      polls += 1;
      return {
        frames: 120,
        dropped: 3,
        duplicates: 0,
        fps: 30.1,
        bitrate_kbps: 2450.5,
        latency_ms: 42.4,
        rtt_ms: 5,
        last_seq: 120,
        subscribers: 2,
        closed: false,
        session: session(),
      };
    },
  });
  await page.goto("/");

  const stream = page.getByTestId("live-stream");
  await expect(stream).toBeVisible();
  await expect(stream).toHaveAttribute("src", /\/stream\.mjpg\?rev=\d+/);
  await expect(page.getByTestId("live-status")).toBeHidden();
  await expect(page.getByTestId("vf-corner")).toContainText("LIVE");
  await expect(page.getByTestId("stat-fps")).toHaveText("30.1");
  await expect(page.getByTestId("stat-bitrate")).toHaveText("2.45 Mbit/s");
  await expect(page.getByTestId("stat-latency")).toHaveText("42 ms");
  await expect(page.getByTestId("stat-drops")).toHaveText("3");
  await expect(page.getByTestId("stat-frames")).toHaveText("120");
  await expect.poll(() => polls, { timeout: 5_000 }).toBeGreaterThan(1);
});

test("draws face boxes and the grid in the browser", async ({ page }) => {
  const state = hostState({
    faces: {
      supported: true,
      actual: [{ id: 0, score: 92, x: 0.1, y: 0.2, w: 0.25, h: 0.35 }],
      source: "device",
    },
    stream_rotation: {
      supported: true,
      requested: 90,
      actual: 90,
      source: "host",
    },
  });
  await routeLiveHost(page, { state });
  await page.goto("/");

  await expect(page.getByTestId("viewfinder")).toHaveAttribute(
    "data-rotation",
    "90",
  );
  const boxes = page.getByTestId("face-overlay").locator(".box");
  await expect(boxes).toHaveCount(1);
  await expect(boxes).toContainText("face 1");

  await page.getByTestId("show-faces").uncheck();
  await expect(page.getByTestId("face-overlay").locator(".box")).toHaveCount(0);

  await page.getByTestId("show-grid").check();
  await expect(page.getByTestId("grid-overlay")).toBeVisible();
  await page.getByTestId("show-grid").uncheck();
  await expect(page.getByTestId("grid-overlay")).toBeHidden();
});

test("takes a snapshot from the host frame", async ({ page }) => {
  await routeLiveHost(page);
  await page.goto("/");
  const snapshot = page.getByTestId("snapshot");
  await expect(snapshot).toBeEnabled();

  const downloadPromise = page.waitForEvent("download");
  await snapshot.click();
  const download = await downloadPromise;
  expect(download.suggestedFilename()).toBe("watchdog-snapshot.jpg");
  await expect(page.getByTestId("live-status")).toContainText("Snapshot saved");
});

test("fullscreen toggles the viewfinder", async ({ page }) => {
  await routeLiveHost(page);
  await page.goto("/");
  const button = page.getByTestId("fullscreen");
  const exit = page.getByTestId("exit-fullscreen");
  await expect(exit).toBeHidden();

  await button.click();
  await expect(button).toHaveAttribute("aria-pressed", "true");
  await expect(button).toHaveText("Exit fullscreen");
  await expect(exit).toBeVisible();

  await exit.click();
  await expect(button).toHaveAttribute("aria-pressed", "false");
  await expect(exit).toBeHidden();
});

test("closes the stream when the session ends", async ({ page }) => {
  let socket: WebSocketRoute | undefined;
  await routeLiveHost(page, {
    socket: (route) => {
      socket = route;
      route.send(
        JSON.stringify({ type: "hello", protocol: 1, session: session() }),
      );
      route.send(JSON.stringify(hostState()));
    },
  });
  await page.goto("/");
  await expect(page.getByTestId("live-stream")).toBeVisible();

  socket?.send(JSON.stringify({ type: "closed" }));
  await expect(page.getByTestId("live-stream")).toBeHidden();
  await expect(page.getByTestId("live-status")).toContainText(
    "session closed",
  );
  await expect(page.getByTestId("stat-fps")).toHaveText("—");
  await expect(page.getByTestId("snapshot")).toBeDisabled();
});

test("pauses the stream while the devices page is open", async ({ page }) => {
  await page.route("**/api/devices", async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({ session: session(), devices: [] }),
    });
  });
  await routeLiveHost(page);
  await page.goto("/");
  await expect(page.getByTestId("live-stream")).toBeVisible();

  await page.getByRole("link", { name: "Devices" }).click();
  await expect(page.getByTestId("live-stream")).toBeHidden();

  await page.getByRole("link", { name: "Live" }).click();
  await expect(page.getByTestId("live-stream")).toBeVisible();
  await expect(page.getByTestId("live-stream")).toHaveAttribute(
    "src",
    /rev=2/,
  );
});
