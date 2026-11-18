import { readFile } from "node:fs/promises";
import { expect, openRoute, test } from "./fixtures";

test.describe("host settings", () => {
  test("shows the effective host configuration", async ({ hostPage }) => {
    await openRoute(hostPage, "settings");
    await expect(hostPage.getByTestId("settings-status")).toHaveText(
      "Loaded settings from the host.",
    );

    await expect(hostPage.getByTestId("settings-ui-port")).toHaveText("7717");
    await expect(hostPage.getByTestId("settings-mcp-port")).toHaveText("7718");
    await expect(hostPage.getByTestId("settings-api-port")).toHaveText("7719");
    await expect(hostPage.getByTestId("settings-listen-mode")).toHaveText(
      "localhost",
    );
    await expect(hostPage.getByTestId("settings-ui-listen")).toHaveText(
      "127.0.0.1",
    );
    await expect(hostPage.getByTestId("settings-base-url")).toHaveText(
      "http://127.0.0.1:7717/",
    );

    await expect(hostPage.getByTestId("settings-token-state")).toHaveText(
      "configured",
    );
    await expect(hostPage.getByTestId("settings-token-hint")).toHaveText(
      "...1234",
    );
    await expect(hostPage.getByTestId("settings-token-warning")).toBeHidden();

    await expect(hostPage.getByTestId("settings-media-items")).toHaveText(
      /^\d+$/,
    );
    await expect(hostPage.getByTestId("settings-disk-free")).toHaveText(
      "128 GiB",
    );
    await expect(hostPage.getByTestId("settings-disk-total")).toHaveText(
      "512 GiB",
    );
    await expect(hostPage.getByTestId("settings-data-dir")).toContainText(
      ".e2e-data",
    );

    await expect(hostPage.getByTestId("settings-ffmpeg-state")).toHaveText(
      "missing",
    );
    await expect(hostPage.getByTestId("settings-ffmpeg-version")).toHaveText(
      "unknown",
    );
    await expect(hostPage.getByTestId("settings-ffmpeg-path")).toHaveText(
      "not extracted",
    );

    await expect(hostPage.getByTestId("settings-version")).toHaveText(
      /^\d+\.\d+\.\d+/,
    );
    await expect(hostPage.getByTestId("settings-commit")).toHaveText(
      "mock-device",
    );
    await expect(hostPage.getByTestId("settings-build")).toHaveText(
      "mock-device",
    );
    await expect(hostPage.getByTestId("settings-protocol")).toHaveText(
      /^v\d+$/,
    );
    await expect(hostPage.getByTestId("settings-ui-version")).toHaveText(
      "0.1.0",
    );
  });

  test("filters the host log ring buffer by level", async ({ hostPage }) => {
    await openRoute(hostPage, "settings");
    await expect(hostPage.getByTestId("settings-status")).toHaveText(
      "Loaded settings from the host.",
    );
    const lines = hostPage.getByTestId("settings-log-line");
    await expect
      .poll(() => lines.count(), { timeout: 15_000 })
      .toBeGreaterThanOrEqual(13);

    await hostPage.getByTestId("settings-log-level").selectOption("error");
    await expect(hostPage.getByTestId("settings-log-count")).toHaveText(
      "1 line",
    );
    await expect(lines).toHaveCount(1);
    await expect(lines.first()).toContainText("no analyzer engine yet");
    await expect(lines.first()).toHaveAttribute("data-level", "error");

    await hostPage.getByTestId("settings-log-level").selectOption("debug");
    await expect
      .poll(() => lines.count(), { timeout: 15_000 })
      .toBeGreaterThanOrEqual(2);
    await expect(lines.first()).toHaveAttribute("data-level", "debug");

    await hostPage.getByTestId("settings-log-level").selectOption("all");
    await expect
      .poll(() => lines.count(), { timeout: 15_000 })
      .toBeGreaterThanOrEqual(13);
  });

  test("saves the retention limits", async ({ hostPage }) => {
    await openRoute(hostPage, "settings");
    await expect(hostPage.getByTestId("settings-status")).toHaveText(
      "Loaded settings from the host.",
    );
    await hostPage.getByTestId("settings-retention-age").fill("14");
    await hostPage.getByTestId("settings-retention-size").fill("2048");
    await hostPage.getByTestId("settings-retention-save").click();
    await expect(hostPage.getByTestId("settings-status")).toHaveText(
      "Retention saved: 14 days, 2048 MiB.",
    );
    await expect(hostPage.getByTestId("settings-retention-age")).toHaveValue(
      "14",
    );
    await expect(hostPage.getByTestId("settings-retention-size")).toHaveValue(
      "2048",
    );
  });

  test("downloads a zip diagnostics bundle", async ({ hostPage }) => {
    await openRoute(hostPage, "settings");
    await expect(hostPage.getByTestId("settings-status")).toHaveText(
      "Loaded settings from the host.",
    );

    const [download] = await Promise.all([
      hostPage.waitForEvent("download"),
      hostPage.getByTestId("settings-diagnostics").click(),
    ]);
    expect(download.suggestedFilename()).toMatch(
      /^watchdog-diagnostics.*\.zip$/,
    );
    const file = await download.path();
    expect(file, "downloaded diagnostics path").not.toBeNull();
    if (!file) return;
    const bytes = await readFile(file);
    expect(bytes.length).toBeGreaterThan(100);
    expect(bytes.subarray(0, 2).toString("latin1")).toBe("PK");
  });

  test("copies the base URL to the clipboard", async ({ hostPage }) => {
    await openRoute(hostPage, "settings");
    await expect(hostPage.getByTestId("settings-status")).toHaveText(
      "Loaded settings from the host.",
    );
    await hostPage.getByTestId("settings-copy-url").click();
    await expect(hostPage.getByTestId("settings-status")).toHaveText(
      "Copied http://127.0.0.1:7717/",
    );
    const copied = await hostPage.evaluate(() => navigator.clipboard.readText());
    expect(copied).toBe("http://127.0.0.1:7717/");
  });
});
