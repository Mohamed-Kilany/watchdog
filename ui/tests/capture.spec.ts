import { expect, test, type Page, type WebSocketRoute } from "@playwright/test";

/** A state envelope with the fields the control panels read. */
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

interface HostRoutes {
  socket?: (socket: WebSocketRoute) => void;
}

/** Boot the shell in host mode with a mock real-time channel. */
async function routeCaptureHost(
  page: Page,
  options: HostRoutes = {},
): Promise<void> {
  const state = hostState();
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
    if (options.socket) {
      options.socket(socket);
      return;
    }
    socket.send(JSON.stringify({ type: "hello", protocol: 1 }));
    socket.send(JSON.stringify(state));
  });
}

test("routes to the capture page and back to live", async ({ page }) => {
  await page.goto("/#/capture");

  await expect(page.locator("#view-capture")).toBeVisible();
  await expect(page.locator("#view-live")).toBeHidden();
  await expect(page.locator('a[data-route="capture"]')).toHaveAttribute(
    "aria-current",
    "page",
  );

  await page.locator('a[data-route="live"]').click();
  await expect(page.locator("#view-live")).toBeVisible();
  await expect(page.locator("#view-capture")).toBeHidden();
});

test("captures a still and streams transfer progress in the demo", async ({
  page,
}) => {
  await page.goto("/#/capture");
  await expect(page.getByTestId("capture-mode")).toHaveText("demo device");

  await page.getByTestId("capture-still").click();

  await expect(page.getByTestId("capture-status")).toContainText(
    "Still captured",
  );
  await expect(page.getByTestId("capture-last-still")).toContainText(
    "4000×3000",
  );

  const transfer = page.getByTestId("transfer-item");
  await expect(transfer).toHaveCount(1);
  await expect(transfer).toHaveAttribute("data-state", "done", {
    timeout: 10_000,
  });
  await expect(transfer.locator("code")).toContainText("m-still-");
  await expect(page.getByTestId("transfer-empty")).toBeHidden();
});

test("runs a burst and reports per-frame progress", async ({ page }) => {
  await page.goto("/#/capture");

  await page.getByTestId("capture-burst-count").fill("3");
  await page.getByTestId("capture-burst-interval").fill("0");
  await page.getByTestId("capture-burst").click();

  await expect(page.getByTestId("capture-status")).toContainText(
    "Burst complete: 3 frames",
  );
  await expect(page.getByTestId("capture-burst-progress")).toHaveText(
    "3 / 3",
  );
  await expect(page.getByTestId("capture-burst-bar")).toHaveJSProperty(
    "value",
    3,
  );
  await expect(page.getByTestId("transfer-item")).toHaveCount(1);
});

test("records video with a live timer and stops cleanly", async ({ page }) => {
  await page.goto("/#/capture");

  await page.getByTestId("capture-record-start").click();
  await expect(page.getByTestId("recording-state")).toHaveText("Recording");
  await expect(page.getByTestId("capture-record-stop")).toBeEnabled();

  await expect
    .poll(
      async () =>
        (await page.getByTestId("recording-timer").textContent()) ?? "00:00",
      { timeout: 5_000 },
    )
    .not.toBe("00:00");

  await page.getByTestId("capture-record-stop").click();
  await expect(page.getByTestId("recording-state")).toHaveText("Idle");
  await expect(page.getByTestId("capture-record-start")).toBeEnabled();
  await expect(page.getByTestId("capture-status")).toContainText(
    /Recording stopped after \d\d:\d\d/,
  );
  await expect(page.getByTestId("transfer-item")).toHaveCount(1);
});

test("forwards capture commands to the host and renders transfers", async ({
  page,
}) => {
  let socket: WebSocketRoute | undefined;
  await routeCaptureHost(page, {
    socket: (route) => {
      socket = route;
      route.send(JSON.stringify({ type: "hello", protocol: 1 }));
      route.send(JSON.stringify(hostState()));
    },
  });

  interface CommandBody {
    id: string;
    command: string;
    params: Record<string, unknown>;
  }
  const bodies: CommandBody[] = [];
  await page.route("**/api/command", async (route) => {
    const body = route.request().postDataJSON() as CommandBody;
    bodies.push(body);
    const applied =
      body.command === "capture_burst"
        ? {
            sequence_id: "seq-host-1",
            media_ids: ["m-burst-1", "m-burst-2"],
            count: 2,
          }
        : {
            media_id: "m-host-1",
            type: "still",
            width: 4000,
            height: 3000,
            bytes: 1_000_000,
          };
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        type: "result",
        id: body.id,
        results: [{ control: "command", ok: true, applied }],
      }),
    });
  });

  await page.goto("/#/capture");
  await expect(page.getByTestId("capture-mode")).toHaveText("host");

  await page.getByTestId("capture-still").click();
  await expect(page.getByTestId("capture-status")).toContainText("m-host-1");
  await expect(page.getByTestId("capture-last-still")).toContainText(
    "4000×3000",
  );

  socket?.send(
    JSON.stringify({
      type: "transfer",
      transfer_id: "t-host-1",
      media_id: "m-host-1",
      state: "transferring",
      bytes_total: 1_000_000,
      bytes_done: 400_000,
    }),
  );
  await expect(page.getByTestId("transfer-item")).toHaveAttribute(
    "data-state",
    "transferring",
  );
  socket?.send(
    JSON.stringify({
      type: "transfer",
      transfer_id: "t-host-1",
      media_id: "m-host-1",
      state: "done",
      bytes_total: 1_000_000,
      bytes_done: 1_000_000,
    }),
  );
  await expect(page.getByTestId("transfer-item")).toHaveAttribute(
    "data-state",
    "done",
  );

  await page.getByTestId("capture-burst-count").fill("2");
  await page.getByTestId("capture-burst-interval").fill("100");
  await page.getByTestId("capture-from-stream").check();
  await page.getByTestId("capture-burst").click();
  await expect(page.getByTestId("capture-status")).toContainText(
    "Burst complete: 2 frames",
  );

  expect(bodies[0]?.command).toBe("capture_still");
  expect(bodies[0]?.params).toEqual({});
  expect(bodies[1]?.command).toBe("capture_burst");
  expect(bodies[1]?.params).toEqual({
    count: 2,
    interval_ms: 100,
    from_stream: true,
  });
});

test("surfaces an unsupported capture command from the host", async ({
  page,
}) => {
  await routeCaptureHost(page);
  await page.route("**/api/command", async (route) => {
    const body = route.request().postDataJSON() as { id: string };
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        type: "result",
        id: body.id,
        results: [
          {
            control: "command",
            ok: false,
            error_code: "UNSUPPORTED_COMMAND",
            message: "capture_still is not supported by the remote app",
          },
        ],
      }),
    });
  });

  await page.goto("/#/capture");
  await expect(page.getByTestId("capture-mode")).toHaveText("host");
  await page.getByTestId("capture-still").click();

  await expect(page.getByTestId("capture-status")).toContainText(
    "Still capture failed: capture_still is not supported by the remote app",
  );
  await expect(page.getByTestId("capture-status")).toHaveAttribute(
    "data-state",
    "error",
  );
  await expect(page.getByTestId("capture-still")).toBeEnabled();
});
