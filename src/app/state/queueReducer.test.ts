import { describe, expect, it } from "vitest";

import { initialAppState } from "./appReducer";
import { reduceQueueState } from "./queueReducer";
import type { QueueItem } from "../../features/media/types";

describe("reduceQueueState", () => {
  it("updates activeId when replacing a queue item", () => {
    const state = {
      ...initialAppState,
      activeId: "yt-1",
      queue: [testQueueItem("yt-1")],
    };

    const nextState = reduceQueueState(state, {
      type: "replace_queue_item",
      previousId: "yt-1",
      item: testQueueItem("task-1"),
    });

    expect(nextState.queue.map((item) => item.id)).toEqual(["task-1"]);
    expect(nextState.activeId).toBe("task-1");
  });
});

function testQueueItem(id: string): QueueItem {
  return {
    id,
    path: `D:\\media\\${id}.mp4`,
    name: `${id}.mp4`,
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
  };
}
