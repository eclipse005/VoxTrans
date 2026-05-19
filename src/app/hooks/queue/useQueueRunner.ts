import { useCallback, useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  enqueueAndExecuteTaskBatch,
  executeTaskBatch,
} from "../../api/workspace";
import {
  type QueueItem,
} from "../../../features/media/types";
import type { AppAction } from "../../state/appReducer";
import {
  patchQueueItem,
} from "../../state/queueDomainActions";
import { reportError, toUserErrorMessage } from "../../utils/errors";
import {
  mergeTaskStateChanged,
  toEnqueuePayload,
  type QueueRunMode,
} from "../../../features/media/queueUtils";

type DispatchState = (action: AppAction) => void;
type PushToast = (message: string, tone?: "info" | "success" | "error") => void;

type UseQueueRunnerArgs = {
  dispatch: DispatchState;
  pushToast: PushToast;
  isTaskPresent: (taskId: string) => boolean;
};

export type { QueueRunMode };

export function useQueueRunner({
  dispatch,
  pushToast,
  isTaskPresent,
}: UseQueueRunnerArgs) {
  // Listen for task-state-changed events from backend (single source of truth)
  useEffect(() => {
    let disposed = false;
    let unlistenTaskStateChanged: undefined | (() => void);

    listen<{
      id: string;
      path: string;
      name: string;
      mediaKind: string;
      sizeBytes: number;
      sourceLang?: string;
      targetLang?: string;
      transcribeStatus: string;
      taskProgress: import("../../../features/media/types").TaskProgress;
      transcribeError: string;
      resultText: string;
      resultSrt: string;
      subtitleSegmentsJson: string;
    }>("task-state-changed", (event) => {
      const payload = event.payload;
      if (!payload?.id) return;
      if (!isTaskPresent(payload.id)) return;
      patchQueueItem(dispatch, payload.id, (current) => mergeTaskStateChanged(current, payload));
    })
      .then((fn) => {
        if (disposed) {
          fn();
          return;
        }
        unlistenTaskStateChanged = fn;
      })
      .catch(() => {});

    return () => {
      disposed = true;
      if (unlistenTaskStateChanged) unlistenTaskStateChanged();
    };
  }, [dispatch, isTaskPresent]);

  const runTask = useCallback(async (item: QueueItem, mode: QueueRunMode) => {
    if (!isTaskPresent(item.id)) return;

    try {
      const response = await enqueueAndExecuteTaskBatch({
        items: [toEnqueuePayload(item, mode)],
      });

      const failed = response.failed.find((entry) => entry.taskId === item.id);
      if (failed) {
        pushToast(`失败：${item.name}，${failed.error}`, "error");
        return;
      }
      if (!isTaskPresent(item.id)) return;

      pushToast(
        mode === "transcribe" ? `已完成：${item.name}` : `已完成转译：${item.name}`,
        "success",
      );
    } catch (err) {
      if (!isTaskPresent(item.id)) return;
      reportError(err, "runTask");
      const fallback = mode === "transcribe" ? "转录失败，请检查模型和运行时配置" : "转译失败，请检查翻译配置";
      const errorMessage = toUserErrorMessage(err, fallback);
      pushToast(`失败：${item.name}，${errorMessage}`, "error");
    }
  }, [
    isTaskPresent,
    pushToast,
  ]);
  const runBatch = useRunBatch(pushToast, isTaskPresent);
  const runQueuedByTaskIds = useRunQueuedByTaskIds(pushToast, isTaskPresent);

  return {
    runTask,
    runBatch,
    runQueuedByTaskIds,
  };
}

type BatchTask = {
  item: QueueItem;
  mode: QueueRunMode;
};

function useRunBatch(
  pushToast: PushToast,
  isTaskPresent: (taskId: string) => boolean,
) {
  return useCallback(async (tasks: BatchTask[]) => {
    const items = tasks.filter((task) => isTaskPresent(task.item.id));
    if (!items.length) return;

    const response = await enqueueAndExecuteTaskBatch({
      items: items.map((task) => toEnqueuePayload(task.item, task.mode)),
    });

    if (response.failed.length > 0) {
      const first = response.failed[0];
      pushToast(`部分任务失败：${first.taskId}，${first.error}`, "error");
    }
  }, [pushToast, isTaskPresent]);
}

function useRunQueuedByTaskIds(
  pushToast: PushToast,
  isTaskPresent: (taskId: string) => boolean,
) {
  return useCallback(async (taskIds: string[]) => {
    const items = taskIds
      .map((taskId) => taskId.trim())
      .filter((taskId) => taskId.length > 0 && isTaskPresent(taskId))
      .map((taskId) => ({ taskId }));
    if (!items.length) return;

    const response = await executeTaskBatch({ items });
    if (response.failed.length > 0) {
      const first = response.failed[0];
      pushToast(`部分任务失败：${first.taskId}，${first.error}`, "error");
    }
  }, [pushToast, isTaskPresent]);
}
