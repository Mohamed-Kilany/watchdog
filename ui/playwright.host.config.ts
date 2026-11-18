import path from "node:path";
import { defineConfig, devices } from "@playwright/test";

// End-to-end suite against the real release host binary running the synthetic
// device (`watchdog serve --mock-device`). The host serves the built UI, the
// WebSocket channel, the MJPEG stream and the mock media/settings routes, so
// every page is exercised through the same code paths a phone would use.
const PORT = 7717;
const BASE_URL = `http://127.0.0.1:${PORT}`;
const TOKEN = "e2e-token-1234";

// `npm run test:host` runs from `ui/`; the workspace release binary lives in
// the repository's target directory. CI may point at another build.
const HOST_BIN =
  process.env.WATCHDOG_HOST_BIN ??
  path.resolve(
    process.cwd(),
    "..",
    "target",
    "release",
    process.platform === "win32" ? "watchdog.exe" : "watchdog",
  );
const DATA_DIR = path.resolve(process.cwd(), ".e2e-data");

const command = [
  `"${HOST_BIN}"`,
  `--data-dir "${DATA_DIR}"`,
  `--token ${TOKEN}`,
  `--ui-port ${PORT}`,
  "--mcp-port 7718",
  "--api-port 7719",
  "--log-level debug",
  "serve",
  "--mock-device",
].join(" ");

export default defineConfig({
  testDir: "./e2e",
  outputDir: "test-results-host",
  fullyParallel: false,
  workers: 1,
  forbidOnly: Boolean(process.env.CI),
  retries: process.env.CI ? 2 : 0,
  timeout: 60_000,
  // A wedged run must still end early enough to print its failures before the
  // CI job's own timeout kills the log.
  globalTimeout: process.env.CI ? 15 * 60_000 : 0,
  expect: { timeout: 10_000 },
  reporter: [
    ["list"],
    ["html", { open: "never", outputFolder: "playwright-report-host" }],
  ],
  use: {
    baseURL: BASE_URL,
    trace: "on-first-retry",
    screenshot: "only-on-failure",
    permissions: ["clipboard-read", "clipboard-write"],
  },
  projects: [
    {
      name: "host-chromium",
      use: {
        ...devices["Desktop Chrome"],
        viewport: { width: 1440, height: 1000 },
      },
    },
  ],
  webServer: {
    command,
    url: `${BASE_URL}/healthz?token=${TOKEN}`,
    cwd: process.cwd(),
    reuseExistingServer: !process.env.CI,
    timeout: 120_000,
    stdout: "pipe",
    stderr: "pipe",
  },
});
