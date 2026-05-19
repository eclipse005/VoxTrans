import { describe, expect, it } from "vitest";

import {
  applyQueueFailures,
  formatQueueFailureMessage,
} from "./useQueueRunner";
import type { AppAction } from "../../state/appReducer";
import type { QueueItem } from "../../../features/media/types";

describe("formatQueueFailureMessage", () => {
  it("normalizes structured backend errors in single task failure messages", () => {
    const message = formatQueueFailureMessage(
      "示例.mp4",
      JSON.stringify({
        code: "TASK_NOT_FOUND",
        message: "task not found: task-1",
      }),
    );

    expect(message).toBe("失败：示例.mp4，任务不存在，请刷新任务列表");
  });

  it("normalizes structured backend errors in batch failure messages", () => {
    const message = formatQueueFailureMessage(
      "task-1",
      {
        code: "TASK_BUSY",
        message: "task is processing or queued",
      },
      "部分任务失败",
    );

    expect(message).toBe("部分任务失败：task-1，任务正在处理中，请稍后再试");
  });

  it("marks failed queued tasks as error to stop scheduler retries", () => {
    const actions: AppAction[] = [];
    const dispatch = (action: AppAction) => actions.push(action);

    applyQueueFailures(
      dispatch,
      [
        {
          taskId: "task-1",
          error: JSON.stringify({
            code: "IO_ERROR",
            message: "cannot hydrate workspace",
          }),
        },
      ],
      (taskId) => taskId === "task-1",
    );

    expect(actions).toHaveLength(1);
    expect(actions[0]).toMatchObject({
      type: "patch_queue_item",
      id: "task-1",
    });

    const action = actions[0];
    if (action.type !== "patch_queue_item") {
      throw new Error("expected patch_queue_item action");
    }
    const updated = action.updater(testQueueItem("task-1"));

    expect(updated.transcribeStatus).toBe("error");
    expect(updated.transcribeError).toBe("文件读写失败，请检查磁盘空间");
    expect(updated.taskProgress.stage.code).toBe("");
  });
});

function testQueueItem(id: string): QueueItem {
  return {
    id,
    path: "D:\\media\\demo.mp4",
    name: "demo.mp4",
    mediaKind: "video",
    sizeBytes: 1,
    sourceLang: "en",
    targetLang: "zh-CN",
    transcribeStatus: "queued",
    taskProgress: {
      stage: {
        code: "preparing",
        label: "准备中",
        order: 1,
        detail: "",
        current: 1,
        total: 1,
      },
    },
    transcribeError: "",
    resultText: "",
    resultSrt: "",
    subtitleSegmentsJson: "[]",
  };
}
