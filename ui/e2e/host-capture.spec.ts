import {
  captureStill,
  expect,
  expectTransferDone,
  mediaCard,
  mediaCount,
  openRoute,
  test,
} from "./fixtures";

test.describe("host capture", () => {
  test("captures a still, transfers it and indexes it", async ({
    hostPage,
  }) => {
    const id = await captureStill(hostPage);
    await expect(hostPage.getByTestId("capture-status")).toContainText(
      `Still captured: media ${id}`,
    );
    await expect(hostPage.getByTestId("capture-last-still")).toContainText(id);
    await expectTransferDone(hostPage, id);

    await openRoute(hostPage, "media");
    await expect(mediaCard(hostPage, id)).toBeVisible();
    await expect(mediaCard(hostPage, id)).toHaveAttribute("data-type", "still");
  });

  test("runs a three-frame burst", async ({ hostPage, request }) => {
    await openRoute(hostPage, "capture");
    const before = await mediaCount(request);

    await hostPage.getByTestId("capture-burst-count").fill("3");
    await hostPage.getByTestId("capture-burst-interval").fill("50");
    await hostPage.getByTestId("capture-burst").click();

    await expect(hostPage.getByTestId("capture-status")).toContainText(
      /Burst complete: 3 frames \(burst-mock-\d+\)\./,
    );
    await expect(hostPage.getByTestId("capture-burst-progress")).toHaveText(
      "3 / 3",
    );
    await expect
      .poll(() => mediaCount(request), { timeout: 20_000 })
      .toBe(before + 3);
  });

  test("records a video and indexes it on stop", async ({
    hostPage,
    request,
  }) => {
    await openRoute(hostPage, "capture");
    const recording = hostPage.getByTestId("recording-state");

    // A failed earlier attempt may have left a recording running.
    if ((await recording.textContent())?.trim() === "Recording") {
      await hostPage.getByTestId("capture-record-stop").click();
      await expect(recording).toHaveText("Idle");
    }

    const before = await mediaCount(request, "video");
    await hostPage.getByTestId("capture-record-start").click();
    await expect(hostPage.getByTestId("capture-status")).toContainText(
      "Recording m-mock-recording.",
    );
    await expect(recording).toHaveText("Recording");
    await expect(hostPage.getByTestId("capture-record-stop")).toBeEnabled();

    await hostPage.waitForTimeout(1_200);
    await hostPage.getByTestId("capture-record-stop").click();

    await expect(hostPage.getByTestId("capture-status")).toHaveText(
      /Recording stopped after \d{2}:\d{2}\./,
    );
    await expect(recording).toHaveText("Idle");
    await expect
      .poll(() => mediaCount(request, "video"), { timeout: 20_000 })
      .toBe(before + 1);

    await openRoute(hostPage, "media");
    await hostPage.getByTestId("media-filter-type").selectOption("video");
    await expect
      .poll(() => hostPage.getByTestId("media-card").count())
      .toBeGreaterThanOrEqual(2);
    await expect(
      hostPage.locator('li.media-card[data-type="video"]').first(),
    ).toContainText("00:0");
  });
});
