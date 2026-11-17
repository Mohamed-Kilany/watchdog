import { expect, test, type Page, type WebSocketRoute } from "@playwright/test";

/** A complete-enough state envelope so the panel renders like the demo. */
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

interface StateRouteOptions {
  /** Reject probes that do not carry `Bearer <token>`. */
  unauthorized?: boolean;
  token?: string;
}

async function routeHostState(
  page: Page,
  options: StateRouteOptions = {},
): Promise<void> {
  await page.route("**/api/state", async (route) => {
    const authorization = route.request().headers()["authorization"];
    if (options.unauthorized && authorization !== `Bearer ${options.token}`) {
      await route.fulfill({
        status: 401,
        contentType: "text/plain; charset=utf-8",
        body: "unauthorized\n",
        headers: { "WWW-Authenticate": "Bearer" },
      });
      return;
    }
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify(hostState()),
    });
  });
}

async function routeHostSocket(
  page: Page,
  handler?: (socket: WebSocketRoute) => void,
): Promise<void> {
  await page.routeWebSocket("**/ws*", (socket) => {
    if (handler) {
      handler(socket);
      return;
    }
    socket.send(JSON.stringify({ type: "hello", protocol: 1 }));
    socket.send(JSON.stringify(hostState()));
  });
}

test("falls back to the offline demo when no host answers", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("panel")).toBeVisible();
  await expect(page.locator("#mode-tag")).toHaveText("demo device");
  await expect(page.getByTestId("connection-banner")).toBeHidden();
  expect(await page.evaluate(() => window.__watchdog.device.mode)).toBe("demo");
});

test("prompts for the bearer token on 401 and retries", async ({ page }) => {
  await routeHostState(page, { unauthorized: true, token: "secret" });
  await routeHostSocket(page);
  await page.goto("/");

  const dialog = page.getByTestId("token-dialog");
  await expect(dialog).toBeVisible();
  await expect(page.locator("#token-error")).toContainText("bearer token");

  await page.locator("#token-input").fill("wrong");
  await page.locator("#token-submit").click();
  await expect(dialog).toBeVisible();
  await expect(page.locator("#token-error")).toContainText("rejected");

  await page.locator("#token-input").fill("secret");
  await page.locator("#token-submit").click();
  await expect(dialog).toBeHidden();
  await expect(page.locator("#mode-tag")).toHaveText("host");
  await expect(page.getByTestId("connection-banner")).toBeHidden();
  expect(
    await page.evaluate(() => sessionStorage.getItem("watchdog.token")),
  ).toBe("secret");
});

test("consumes ?token= from the URL into session storage", async ({ page }) => {
  await routeHostState(page, { unauthorized: true, token: "secret" });
  await routeHostSocket(page);
  await page.goto("/?token=secret");
  await expect(page.locator("#mode-tag")).toHaveText("host");
  expect(new URL(page.url()).searchParams.get("token")).toBeNull();
  expect(
    await page.evaluate(() => sessionStorage.getItem("watchdog.token")),
  ).toBe("secret");
});

test("streams state snapshots and events from the host channel", async ({
  page,
}) => {
  await routeHostState(page);
  let socket: WebSocketRoute | undefined;
  await routeHostSocket(page, (route) => {
    socket = route;
    route.send(JSON.stringify({ type: "hello", protocol: 1 }));
    route.send(JSON.stringify(hostState()));
  });
  await page.goto("/");
  await expect(page.locator("#mode-tag")).toHaveText("host");
  await expect
    .poll(async () => (await page.evaluate(() => window.__watchdog.state())).iso?.actual)
    .toBe(100);

  const next = hostState();
  next.seq = 2;
  next.state.iso = { supported: true, actual: 400, source: "device" };
  socket?.send(JSON.stringify(next));
  await expect
    .poll(async () => (await page.evaluate(() => window.__watchdog.state())).iso?.actual)
    .toBe(400);

  socket?.send(
    JSON.stringify({
      type: "event",
      kind: "camera_disconnected",
      timestamp_ms: 2_000,
    }),
  );
  await expect(page.getByTestId("command-log")).toContainText(
    "event camera_disconnected",
  );
});

test("shows the reconnect banner and reconnects with backoff", async ({
  page,
}) => {
  await routeHostState(page);
  let connections = 0;
  await page.routeWebSocket("**/ws*", (socket) => {
    connections += 1;
    if (connections === 1) {
      socket.close();
      return;
    }
    socket.send(JSON.stringify(hostState()));
  });

  await page.goto("/");
  const banner = page.getByTestId("connection-banner");
  await expect(banner).toBeVisible();
  await expect(banner).toContainText("reconnecting");
  await expect(banner).toBeHidden({ timeout: 10_000 });
  expect(connections).toBeGreaterThanOrEqual(2);
});

test("sends controls over REST and logs the app result", async ({ page }) => {
  await routeHostState(page);
  await routeHostSocket(page);
  interface CommandBody {
    id: string;
    command: string;
    params: { controls: Array<{ id: string; value: unknown }> };
  }
  const bodies: CommandBody[] = [];
  await page.route("**/api/command", async (route) => {
    const body = route.request().postDataJSON() as CommandBody;
    bodies.push(body);
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        type: "result",
        id: body.id,
        timestamp_ms: 1_234,
        results: body.params.controls.map((control) => ({
          control: control.id,
          ok: true,
          applied: control.value,
        })),
      }),
    });
  });

  await page.goto("/");
  await expect(page.locator("#mode-tag")).toHaveText("host");
  await page.locator('wd-auto-toggle[data-control-id="ae_mode"] button').click();
  await expect(page.getByTestId("command-log")).toContainText("ae_mode=0");
  await expect(page.getByTestId("command-log")).toContainText("ok");

  const body = bodies[0];
  expect(body?.command).toBe("set_controls");
  expect(body?.params).toEqual({ controls: [{ id: "ae_mode", value: 0 }] });
});
