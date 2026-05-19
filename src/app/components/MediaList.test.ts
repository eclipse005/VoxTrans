import { describe, expect, it } from "vitest";

import { canDeleteQueueItem } from "../../features/media/queuePolicy";
import type { QueueItem } from "../../features/media/types";

describe("canDeleteQueueItem", () => {
  it("blocks busy local pipeline tasks", () => {
    expect(
      canDeleteQueueItem(
        testQueueItem({
          path: "D:\\media\\demo.mp4",
          transcribeStatus: "processing",
        }),
      ),
    ).toBe(false);
    expect(
      canDeleteQueueItem(
        testQueueItem({
          path: "D:\\media\\demo.mp4",
          transcribeStatus: "queued",
        }),
      ),
    ).toBe(false);
  });

  it("allows deleting busy YouTube placeholders so downloads can be cancelled", () => {
    expect(
      canDeleteQueueItem(
        testQueueItem({
          path: "youtube://pending/yt-1?url=https%3A%2F%2Fyoutube.com%2Fwatch%3Fv%3Ddemo",
          transcribeStatus: "processing",
        }),
      ),
    ).toBe(true);
  });
});

function testQueueItem(overrides: Partial<QueueItem>): QueueItem {
  return {
    id: "task-1",
    path: "D:\\media\\demo.mp4",
    name: "demo.mp4",
    mediaKind: "video",
    sizeBytes: 1,
    sourceLang: "en",
    targetLang: "zh-CN",
    transcribeStatus: "pending",
    taskProgress: {
      stage: {
        code: "",
        label: "",
        order: 0,
        detail: "",
        current: 0,
        total: 0,
      },
    },
    transcribeError: "",
    resultText: "",
    resultSrt: "",
    subtitleSegmentsJson: "[]",
    ...overrides,
  };
}
