import { useCallback, useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  executeTaskBatch,
} from "../../api/workspace";
import {
  createEmptyTaskProgress,
} from "../../../features/media/types";
import type { AppAction } from "../../state/appReducer";
import { patchQueueItem } from "../../state/queueDomainActions";
import {
  mergeTaskStateChanged,
  type QueueRunMode,
} from "../../../features/media/queueUtils";
import { toUserErrorMessage } from "../../utils/errors";

type DispatchState = (action: AppAction) => void;
type PushToast = (message: string, tone?: "info" | "success" | "error") => void;
type QueueFailure = {
  taskId: string;
  error: unknown;
};

type UseQueueRunnerArgs = {
  dispatch: DispatchState;
  pushToast: PushToast;
  isTaskPresent: (taskId: string) => boolean;
};

export type { QueueRunMode };

export function formatQueueFailureMessage(
  subject: string,
  error: unknown,
  prefix = "失败",
): string {
  return `${prefix}：${subject}，${toUserErrorMessage(error)}`;
}

export function applyQueueFailures(
  dispatch: DispatchState,
  failed: QueueFailure[],
  isTaskPresent: (taskId: string) => boolean,
): void {
  for (const failure of failed) {
    const taskId = failure.taskId.trim();
    if (!taskId || !isTaskPresent(taskId)) continue;
    const message = toUserErrorMessage(failure.error);
    patchQueueItem(dispatch, taskId, (item) => ({
      ...item,
      transcribeStatus: "error",
      taskProgress: createEmptyTaskProgress(),
      transcribeError: message,
    }));
  }
}

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
      patchQueueItem(dispatch, payload.id, (current) =>
        mergeTaskStateChanged(current, payload),
      );
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

  const runQueuedByTaskIds = useRunQueuedByTaskIds(
    dispatch,
    pushToast,
    isTaskPresent,
  );

  return {
    runQueuedByTaskIds,
  };
}

function useRunQueuedByTaskIds(
  dispatch: DispatchState,
  pushToast: PushToast,
  isTaskPresent: (taskId: string) => boolean,
) {
  return useCallback(
    async (taskIds: string[]) => {
      const items = taskIds
        .map((taskId) => taskId.trim())
        .filter((taskId) => taskId.length > 0 && isTaskPresent(taskId))
        .map((taskId) => ({ taskId }));
      if (!items.length) return;

      const response = await executeTaskBatch({ items });
      if (response.failed.length > 0) {
        applyQueueFailures(dispatch, response.failed, isTaskPresent);
        const first = response.failed[0];
        pushToast(
          formatQueueFailureMessage(first.taskId, first.error, "部分任务失败"),
          "error",
        );
      }
    },
    [dispatch, pushToast, isTaskPresent],
  );
}
