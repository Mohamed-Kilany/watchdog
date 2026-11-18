import { expect, openLive, openRoute, test } from "./fixtures";

test.describe("host live view", () => {
  test("streams the mock viewfinder and rolling stats", async ({
    hostPage,
  }) => {
    await openLive(hostPage);

    const stream = hostPage.getByTestId("live-stream");
    await expect(stream).toBeVisible();
    await expect(stream).toHaveAttribute(
      "src",
      /\/stream\.mjpg\?token=e2e-token-1234&rev=\d+/,
    );
    await expect(hostPage.getByTestId("vf-corner")).toContainText("LIVE");
    await expect(hostPage.getByTestId("vf-corner")).toContainText(
      "Mock device",
    );

    const stats = hostPage.getByTestId("live-stats");
    await expect(stats).toHaveAttribute("data-state", "live", {
      timeout: 30_000,
    });
    await expect(hostPage.getByTestId("stat-fps")).toHaveText(/[0-9]/);
    await expect(hostPage.getByTestId("stat-frames")).toHaveText(/[0-9]/);

    // Leaving the page stops the stream and polling; returning restarts it
    // with a fresh revision cache-buster.
    await openRoute(hostPage, "capture");
    await expect(stream).toBeHidden();
    await openLive(hostPage);
    await expect(stream).toBeVisible();
    await expect(stream).toHaveAttribute(
      "src",
      /\/stream\.mjpg\?token=e2e-token-1234&rev=\d+/,
    );
  });

  test("saves a snapshot JPEG", async ({ hostPage }) => {
    await openLive(hostPage);
    const [download] = await Promise.all([
      hostPage.waitForEvent("download"),
      hostPage.getByTestId("snapshot").click(),
    ]);
    expect(download.suggestedFilename()).toBe("watchdog-snapshot.jpg");
    await expect(hostPage.getByTestId("live-status")).toContainText(
      "Snapshot saved.",
    );
  });

  test("toggles the composition grid", async ({ hostPage }) => {
    await openLive(hostPage);
    const grid = hostPage.getByTestId("grid-overlay");
    await expect(grid).toBeHidden();
    await hostPage.getByTestId("show-grid").check();
    await expect(grid).toBeVisible();
    await hostPage.getByTestId("show-grid").uncheck();
    await expect(grid).toBeHidden();
  });

  test("enters and leaves fullscreen when the browser allows it", async ({
    hostPage,
  }) => {
    await openLive(hostPage);
    const button = hostPage.getByTestId("fullscreen");
    await button.click();

    const active = await hostPage.evaluate(
      () => document.fullscreenElement !== null,
    );
    if (active) {
      await expect(hostPage.getByTestId("exit-fullscreen")).toBeVisible();
      await expect(button).toHaveText("Exit fullscreen");
      await hostPage.getByTestId("exit-fullscreen").click();
      await expect(hostPage.getByTestId("exit-fullscreen")).toBeHidden();
      await expect(button).toHaveText("Fullscreen");
    } else {
      // Headless browsers may refuse the request; the page reports it instead
      // of leaving a stuck state.
      await expect(hostPage.getByTestId("live-status")).toContainText(
        "fullscreen unavailable",
      );
    }
  });

  test("draws and clears a metering region", async ({ hostPage }) => {
    await openLive(hostPage);
    const readout = hostPage.getByTestId("region-readout");
    await expect(readout).toHaveText("No regions set");

    const overlay = hostPage.getByTestId("metering-overlay");
    const box = await overlay.boundingBox();
    expect(box, "metering overlay bounds").not.toBeNull();
    if (!box) return;

    const startX = box.x + box.width * 0.25;
    const startY = box.y + box.height * 0.25;
    await hostPage.mouse.move(startX, startY);
    await hostPage.mouse.down();
    await hostPage.mouse.move(
      box.x + box.width * 0.6,
      box.y + box.height * 0.6,
      { steps: 8 },
    );
    await hostPage.mouse.up();

    await expect(readout).toContainText("metering x", { timeout: 15_000 });
    await expect(readout).not.toHaveText("No regions set");

    await hostPage.getByTestId("clear-regions").click();
    await expect(readout).toHaveText("No regions set", { timeout: 15_000 });
  });
});
