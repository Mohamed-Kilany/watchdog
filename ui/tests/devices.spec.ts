import { expect, test, type Page } from "@playwright/test";

/** A minimal state envelope for a host with no active session. */
function idleState() {
  return {
    type: "state",
    waiting: true,
    seq: 0,
    timestamp_ms: 0,
    camera_id: "",
    state: {},
    session: { active: false, closed: false },
  };
}

function activeSession() {
  return {
    active: true,
    closed: false,
    serial: "R58R41VKXTY",
    device: {
      serial: "R58R41VKXTY",
      kind: "usb",
      manufacturer: "samsung",
      model: "SM-A515F",
      device: "a51",
      sdk: 33,
      release: "13",
      abi: "arm64-v8a",
      app_package: "app.watchdog.remote",
      app_version_name: "0.1.0-dev",
      app_version_code: 1000,
      protocol_version: 1,
      camera_count: 4,
    },
  };
}

function deviceList(session: unknown = { active: false, closed: false }) {
  return {
    session,
    devices: [
      {
        serial: "R58R41VKXTY",
        kind: "usb",
        state: (session as { active?: boolean }).active
          ? "active"
          : "available",
        product: "SM-A515F",
      },
      {
        serial: "192.168.1.10:37123",
        kind: "lan",
        state: "discovered",
        connect: "192.168.1.10:37123",
        pairing: "192.168.1.10:40101",
        guid: "adb-ABC123",
        last_seen_ms: 1_000,
      },
    ],
    discovered_at_ms: 1_000,
  };
}

async function routeHost(page: Page, state = idleState()): Promise<void> {
  await page.route("**/api/state", async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify(state),
    });
  });
  await page.routeWebSocket("**/ws*", (socket) => {
    socket.send(
      JSON.stringify({
        type: "hello",
        protocol: 1,
        session: state.session,
      }),
    );
    socket.send(JSON.stringify(state));
  });
}

test("devices route shows the demo notice without a host", async ({ page }) => {
  await page.goto("/#/devices");
  await expect(page.locator("#view-devices")).toBeVisible();
  await expect(page.locator("#view-live")).toBeHidden();
  await expect(page.getByTestId("devices-notice")).toContainText("Demo mode");
  await expect(page.locator("#scan-devices")).toBeDisabled();
});

test("lists devices and activates a session", async ({ page }) => {
  let session: unknown = { active: false, closed: false };
  await routeHost(page);
  await page.route("**/api/devices", async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify(deviceList(session)),
    });
  });
  await page.route("**/api/devices/activate", async (route) => {
    const body = route.request().postDataJSON() as { serial: string };
    session = activeSession();
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        ok: true,
        session,
        serial: body.serial,
      }),
    });
  });
  await page.route("**/api/devices/status*", async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        serial: "R58R41VKXTY",
        props: {
          sdk: 33,
          abi: "arm64-v8a",
          model: "SM-A515F",
          manufacturer: "samsung",
          release: "13",
          device: "a51",
        },
        installed: true,
        version_code: 1000,
        expected_version_name: "0.1.0-dev",
        expected_version_code: 1000,
        up_to_date: true,
        service_pid: 4242,
        appops_camera: "CAMERA: foreground",
        appops_record_audio: "RECORD_AUDIO: foreground",
      }),
    });
  });

  await page.goto("/#/devices");
  const rows = page.getByTestId("device-list").locator(".device-row");
  await expect(rows).toHaveCount(2);
  await expect(rows.first()).toContainText("R58R41VKXTY");

  await rows.first().getByRole("button", { name: "Activate" }).click();
  await expect(page.getByTestId("devices-notice")).toContainText("Activated");
  await expect(page.getByTestId("active-device")).toBeVisible();
  await expect(page.locator("#active-model")).toContainText("samsung SM-A515F");
  await expect(page.locator("#device-select")).toHaveValue("R58R41VKXTY");
  await expect(page.getByTestId("status-detail")).toContainText("pid 4242");
});

test("pairs with a 6-digit code and refreshes discovery", async ({ page }) => {
  await routeHost(page);
  await page.route("**/api/devices", async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify(deviceList()),
    });
  });
  let pairBody: unknown = null;
  await page.route("**/api/devices/pair", async (route) => {
    pairBody = route.request().postDataJSON();
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({ ok: true, guid: "adb-ABC123" }),
    });
  });
  await page.route("**/api/devices/discover", async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify(deviceList()),
    });
  });

  await page.goto("/#/devices");
  await page.getByRole("button", { name: "Pair device" }).click();
  await expect(page.getByTestId("pair-dialog")).toBeVisible();
  await page.locator("#pair-target").fill("192.168.1.10:40101");
  await page.locator("#pair-code").fill("123456");
  await page.getByTestId("pair-dialog").getByRole("button", { name: "Pair" }).click();
  await expect(page.getByTestId("pair-dialog")).toBeHidden();
  await expect(page.getByTestId("devices-notice")).toContainText("Paired");
  expect(pairBody).toEqual({
    target: "192.168.1.10:40101",
    code: "123456",
  });
});

test("shows provisioning progress and the update prompt", async ({ page }) => {
  const session = activeSession();
  await routeHost(page, { ...idleState(), session });
  await page.route("**/api/devices", async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify(deviceList(session)),
    });
  });
  await page.route("**/api/devices/status*", async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        serial: "R58R41VKXTY",
        props: {
          sdk: 33,
          abi: "arm64-v8a",
          model: "SM-A515F",
          manufacturer: "samsung",
          release: "13",
          device: "a51",
        },
        installed: true,
        version_code: 999,
        expected_version_name: "0.1.0-dev",
        expected_version_code: 1000,
        up_to_date: false,
        service_pid: 4242,
      }),
    });
  });
  await page.route("**/api/devices/provision", async (route) => {
    await route.fulfill({
      status: 202,
      contentType: "application/json",
      body: JSON.stringify({
        id: "job-1",
        job: {
          id: "job-1",
          serial: "R58R41VKXTY",
          status: "running",
          steps: [],
          started_ms: 1_000,
        },
      }),
    });
  });
  let polls = 0;
  await page.route("**/api/devices/provision/job-1", async (route) => {
    polls += 1;
    const done = polls >= 2;
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        id: "job-1",
        serial: "R58R41VKXTY",
        status: done ? "passed" : "running",
        started_ms: 1_000,
        ...(done ? { finished_ms: 2_000, session } : {}),
        steps: [
          {
            name: "device",
            status: "passed",
            duration_ms: 12,
            detail: "Android 13 (SDK 33), SM-A515F",
          },
          ...(done
            ? [
                {
                  name: "app",
                  status: "passed",
                  duration_ms: 240,
                  detail: "installed 0.1.0-dev",
                },
              ]
            : []),
        ],
      }),
    });
  });

  await page.goto("/#/devices");
  await expect(page.getByTestId("update-prompt")).toBeVisible();
  await page.getByTestId("active-device")
    .getByRole("button", { name: "Provision / update" })
    .click();
  await expect(page.getByTestId("provision-panel")).toBeVisible();
  await expect(page.getByTestId("provision-steps")).toContainText("device");
  await expect(page.getByTestId("provision-steps")).toContainText("app");
  await expect(page.getByTestId("provision-status")).toContainText("Passed");
  expect(polls).toBeGreaterThanOrEqual(2);
});

test("disconnects the active session", async ({ page }) => {
  let session: unknown = activeSession();
  await routeHost(page, { ...idleState(), session: activeSession() });
  await page.route("**/api/devices", async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify(deviceList(session)),
    });
  });
  await page.route("**/api/devices/disconnect", async (route) => {
    session = { active: false, closed: false };
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({ ok: true, session }),
    });
  });
  await page.route("**/api/devices/status*", async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        serial: "R58R41VKXTY",
        props: {
          sdk: 33,
          abi: "arm64-v8a",
          model: "SM-A515F",
          manufacturer: "samsung",
          release: "13",
          device: "a51",
        },
        installed: true,
        version_code: 1000,
        up_to_date: true,
      }),
    });
  });

  await page.goto("/#/devices");
  await expect(page.getByTestId("active-device")).toBeVisible();
  await page
    .getByTestId("active-device")
    .getByRole("button", { name: "Disconnect" })
    .click();
  await expect(page.getByTestId("devices-notice")).toContainText("closed");
  await expect(page.getByTestId("active-device")).toBeHidden();
});
