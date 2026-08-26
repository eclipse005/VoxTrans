import { describe, expect, it, vi, beforeEach } from "vitest";
import { act, renderHook } from "@testing-library/react";

import { useQueueRunner } from "./useQueueRunner";
import type { AppAction } from "../../state/appReducer";
import type { QueueItem } from "../../../features/media/types";

// The hook's only async entry point is runQueuedByTaskIds, which calls the
// backend through executeTaskBatch (invoke) and subscribes to Tauri events.
// Both are mocked here; these tests exercise the production path end-to-end
// instead of importing the internal helpers directly.
const mockInvoke = vi.hoisted(() => vi.fn());
const mockListen = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/api/core", () => ({
  invoke: mockInvoke,
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: mockListen,
}));

beforeEach(() => {
  mockInvoke.mockReset();
  mockListen.mockReset().mockResolvedValue(() => {});
});

function renderRunner(overrides?: {
  isTaskPresent?: (taskId: string) => boolean;
}) {
  const actions: AppAction[] = [];
  const toasts: Array<{ message: string; tone?: "info" | "success" | "error" }> = [];
  const dispatch = (action: AppAction) => actions.push(action);
  const pushToast = (message: string, tone?: "info" | "success" | "error") => {
    toasts.push({ message, tone });
  };

  const utils = renderHook(() =>
    useQueueRunner({
      dispatch,
      pushToast,
      isTaskPresent: overrides?.isTaskPresent ?? (() => true),
    }),
  );

  return { ...utils, actions, toasts };
}

describe("useQueueRunner", () => {
  it("shows a normalized error toast when a batch task fails", async () => {
    mockInvoke.mockResolvedValueOnce({
      succeededTaskIds: [],
      failed: [
        {
          taskId: "示例.mp4",
          error: JSON.stringify({
            code: "TASK_BUSY",
            message: "task is processing or queued",
          }),
        },
      ],
    });

    const { result, toasts } = renderRunner();

    await act(async () => {
      await result.current.runQueuedByTaskIds(["示例.mp4"]);
    });

    expect(toasts).toEqual([
      {
        message: "部分任务失败：示例.mp4，任务正在处理中，请稍后再试",
        tone: "error",
      },
    ]);
  });

  it("marks failed queued tasks as error to stop scheduler retries", async () => {
    mockInvoke.mockResolvedValueOnce({
      succeededTaskIds: [],
      failed: [
        {
          taskId: "task-1",
          error: JSON.stringify({
            code: "IO_ERROR",
            message: "cannot hydrate workspace",
          }),
        },
      ],
    });

    const { result, actions } = renderRunner({
      isTaskPresent: (taskId) => taskId === "task-1",
    });

    await act(async () => {
      await result.current.runQueuedByTaskIds(["task-1"]);
    });

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

  it("skips blank and absent task ids without calling the backend", async () => {
    const { result } = renderRunner({ isTaskPresent: () => false });

    await act(async () => {
      await result.current.runQueuedByTaskIds(["", "  ", "missing"]);
    });

    expect(mockInvoke).not.toHaveBeenCalled();
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
