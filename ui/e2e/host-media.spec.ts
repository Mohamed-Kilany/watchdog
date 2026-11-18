import {
  captureStill,
  expect,
  mediaCard,
  openRoute,
  test,
} from "./fixtures";

test.describe("host media library", () => {
  test("lists, filters, sorts and switches views", async ({ hostPage }) => {
    await openRoute(hostPage, "media");
    await expect(hostPage.getByTestId("media-count")).toHaveText(
      /\d+ of \d+ items/,
    );
    await expect(mediaCard(hostPage, "m-mock-seed-still-1")).toBeVisible();
    await expect(mediaCard(hostPage, "m-mock-seed-video-1")).toBeVisible();
    await expect(mediaCard(hostPage, "m-mock-seed-still-2")).toBeVisible();

    await hostPage.getByTestId("media-filter-type").selectOption("still");
    await expect(
      hostPage.locator('li.media-card[data-type="video"]'),
    ).toHaveCount(0);
    await hostPage.getByTestId("media-filter-type").selectOption("all");

    await hostPage.getByTestId("media-search").fill("m-mock-seed-still-1");
    await expect(hostPage.getByTestId("media-card")).toHaveCount(1);
    await hostPage.getByTestId("media-search").fill("");
    await expect(hostPage.getByTestId("media-card")).not.toHaveCount(0);

    await hostPage.getByTestId("media-sort").selectOption("oldest");
    await expect(hostPage.getByTestId("media-card").first()).toHaveAttribute(
      "data-media-id",
      "m-mock-seed-still-2",
    );
    await hostPage.getByTestId("media-sort").selectOption("largest");
    await expect(hostPage.getByTestId("media-card").first()).toHaveAttribute(
      "data-type",
      "video",
    );

    await hostPage.getByTestId("media-view-list").click();
    await expect(hostPage.getByTestId("media-grid")).toHaveAttribute(
      "data-view",
      "list",
    );
    await hostPage.getByTestId("media-view-grid").click();
    await expect(hostPage.getByTestId("media-grid")).toHaveAttribute(
      "data-view",
      "grid",
    );

    await hostPage.getByTestId("media-select-all").click();
    await expect(hostPage.getByTestId("media-selection")).toBeVisible();
    await expect(hostPage.getByTestId("media-selected-count")).toHaveText(
      /\d+ selected/,
    );
    await hostPage.getByTestId("media-select-clear").click();
    await expect(hostPage.getByTestId("media-selection")).toBeHidden();
  });

  test("previews and downloads a seeded still", async ({ hostPage }) => {
    await openRoute(hostPage, "media");
    await hostPage.getByTestId("media-search").fill("m-mock-seed-still-1");
    const card = mediaCard(hostPage, "m-mock-seed-still-1");
    await expect(card).toBeVisible();
    await card.getByTestId("media-open").click();

    const dialog = hostPage.getByTestId("media-dialog");
    await expect(dialog).toBeVisible();
    await expect(hostPage.getByTestId("media-dialog-title")).toHaveText(
      "still m-mock-seed-still-1",
    );
    const metadata = hostPage.getByTestId("media-metadata");
    await expect(metadata).toContainText("image/jpeg");
    await expect(metadata).toContainText("320");
    await expect(metadata).toContainText("180");
    await expect(metadata).toContainText("Mock device");
    await expect(metadata).toContainText("MOCK-0001");

    const image = hostPage.getByTestId("media-image");
    await expect(image).toBeVisible();
    await expect(image).toHaveAttribute(
      "src",
      /\/api\/media\/m-mock-seed-still-1\/file\?token=e2e-token-1234/,
    );

    const download = hostPage.getByTestId("media-download");
    await expect(download).toHaveAttribute(
      "download",
      "m-mock-seed-still-1.jpg",
    );
    await expect(download).toHaveAttribute(
      "href",
      /\/api\/media\/m-mock-seed-still-1\/file\?token=e2e-token-1234/,
    );

    await hostPage.getByTestId("media-zoom-in").click();
    await expect(hostPage.getByTestId("media-zoom-level")).toHaveText("125%");
    await hostPage.getByTestId("media-close").click();
    await expect(dialog).toBeHidden();
  });

  test("previews a seeded video with its duration", async ({ hostPage }) => {
    await openRoute(hostPage, "media");
    await hostPage.getByTestId("media-search").fill("m-mock-seed-video-1");
    const card = mediaCard(hostPage, "m-mock-seed-video-1");
    await expect(card).toBeVisible();
    await card.getByTestId("media-open").click();

    await expect(hostPage.getByTestId("media-dialog")).toBeVisible();
    await expect(hostPage.getByTestId("media-dialog-title")).toHaveText(
      "video m-mock-seed-video-1",
    );
    const video = hostPage.getByTestId("media-video");
    await expect(video).toBeVisible();
    await expect(video).toHaveAttribute(
      "src",
      /\/api\/media\/m-mock-seed-video-1\/file\?token=e2e-token-1234/,
    );
    await expect(hostPage.getByTestId("media-metadata")).toContainText(
      "Duration",
    );
    await expect(hostPage.getByTestId("media-metadata")).toContainText("00:02");
    await expect(hostPage.getByTestId("media-download")).toHaveAttribute(
      "download",
      "m-mock-seed-video-1.webm",
    );
    await hostPage.getByTestId("media-close").click();
    await expect(hostPage.getByTestId("media-dialog")).toBeHidden();
  });

  test("queues a fresh capture for analysis", async ({ hostPage }) => {
    const id = await captureStill(hostPage);
    await openRoute(hostPage, "media");
    await hostPage.getByTestId("media-search").fill(id);
    await expect(mediaCard(hostPage, id)).toBeVisible();

    await hostPage.locator(`input[data-media-select="${id}"]`).check();
    await expect(hostPage.getByTestId("media-selection")).toBeVisible();
    await hostPage.getByTestId("media-analyze-selected").click();
    await expect(hostPage.getByTestId("media-status")).toHaveText(
      /Analysis queued: an-mock-\d+ \(1 item\)/,
    );
  });

  test("deletes fresh captures", async ({ hostPage }) => {
    const first = await captureStill(hostPage);
    const second = await captureStill(hostPage);
    expect(first).not.toBe(second);

    await openRoute(hostPage, "media");
    await hostPage.locator(`input[data-media-select="${first}"]`).check();
    await hostPage.locator(`input[data-media-select="${second}"]`).check();
    await expect(hostPage.getByTestId("media-selected-count")).toHaveText(
      "2 selected",
    );
    await hostPage.getByTestId("media-delete-selected").click();

    await expect(hostPage.getByTestId("media-status")).toHaveText(
      "Deleted 2 items.",
    );
    await expect(mediaCard(hostPage, first)).toHaveCount(0);
    await expect(mediaCard(hostPage, second)).toHaveCount(0);
  });
});
