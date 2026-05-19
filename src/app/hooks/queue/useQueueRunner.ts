import { useCallback, useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  enqueueAndExecuteTaskBatch,
  executeTaskBatch,
} from "../../api/workspace";
import {
  normalizeTaskProgress,
  type SourceLanguage,
  type TargetLanguage,
  type TaskProgress,
  type TaskStageProgress,
  type QueueItem,
} from "../../../features/media/types";
import {
  normalizeSourceLanguage,
  normalizeTargetLanguage,
} from "../../../features/media/languages";
import type { AppAction } from "../../state/appReducer";
import {
  patchQueueItem,
} from "../../state/queueDomainActions";
import { reportError, toUserErrorMessage } from "../../utils/errors";

type DispatchState = (action: AppAction) => void;
type PushToast = (message: string, tone?: "info" | "success" | "error") => void;

// Task state changed event from backend - full state replacement
type TaskStateChangedEvent = {
  id: string;
  path: string;
  name: string;
  mediaKind: string;
  sizeBytes: number;
  sourceLang?: string;
  targetLang?: string;
  transcribeStatus: string;
  taskProgress: TaskProgress;
  transcribeError: string;
  resultText: string;
  resultSrt: string;
  subtitleSegmentsJson: string;
};

function stageOrder(stage: Partial<TaskStageProgress> | null | undefined): number {
  if (!stage) return 0;
  const value = Number(stage.order ?? 0);
  if (!Number.isFinite(value)) return 0;
  return Math.max(0, Math.round(value));
}

function stageRatio(stage: Partial<TaskStageProgress> | null | undefined): number {
  if (!stage) return 0;
  const current = Number(stage.current ?? 0);
  const total = Number(stage.total ?? 0);
  if (!Number.isFinite(current) || !Number.isFinite(total) || total <= 0) return 0;
  return Math.max(0, Math.min(1, current / total));
}

function shouldKeepCurrentProcessingStage(
  current: QueueItem,
  incoming: TaskStateChangedEvent,
): boolean {
  if (current.transcribeStatus !== "processing" || incoming.transcribeStatus !== "processing") {
    return false;
  }
  const currentOrder = stageOrder(current.taskProgress.stage);
  const incomingOrder = stageOrder(incoming.taskProgress?.stage);
  if (incomingOrder > 0 && currentOrder > incomingOrder) {
    return true;
  }
  if (incomingOrder > 0 && currentOrder === incomingOrder) {
    return stageRatio(incoming.taskProgress?.stage) < stageRatio(current.taskProgress.stage);
  }
  return false;
}

export type QueueRunMode = "transcribe" | "transcribe_translate";

type UseQueueRunnerArgs = {
  dispatch: DispatchState;
  pushToast: PushToast;
  isTaskPresent: (taskId: string) => boolean;
};

export function useQueueRunner({
  dispatch,
  pushToast,
  isTaskPresent,
}: UseQueueRunnerArgs) {
  // Listen for task-state-changed events from backend (single source of truth)
  useEffect(() => {
    let disposed = false;
    let unlistenTaskStateChanged: undefined | (() => void);

    listen<TaskStateChangedEvent>("task-state-changed", (event) => {
      const payload = event.payload;
      if (!payload?.id) return;
      if (!isTaskPresent(payload.id)) return;
      // Backend is the single source of truth - replace local state directly
      patchQueueItem(dispatch, payload.id, (current) => {
        const keepCurrentStage = shouldKeepCurrentProcessingStage(current, payload);
        const nextProgress = normalizeTaskProgress(payload.taskProgress);
        return {
          id: payload.id,
          path: payload.path,
          name: payload.name,
          mediaKind: payload.mediaKind as "audio" | "video",
          sizeBytes: payload.sizeBytes,
          sourceLang: normalizeSourceLanguage(payload.sourceLang ?? current.sourceLang),
          targetLang: normalizeTargetLanguage(payload.targetLang ?? current.targetLang),
          transcribeStatus: payload.transcribeStatus as QueueItem["transcribeStatus"],
          taskProgress: keepCurrentStage ? current.taskProgress : nextProgress,
          transcribeError: payload.transcribeError || "",
          resultText: payload.resultText || "",
          resultSrt: payload.resultSrt || "",
          subtitleSegmentsJson: payload.subtitleSegmentsJson || "",
        };
      });
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
      // State is updated via task-state-changed event from backend

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
    // State is updated via task-state-changed event from backend

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

function toEnqueuePayload(
  item: QueueItem,
  mode: QueueRunMode,
): {
  id: string;
  mediaPath: string;
  name: string;
  mediaKind: "audio" | "video";
  sizeBytes: number;
  intent: "TRANSCRIBE" | "TRANSCRIBE_TRANSLATE";
  sourceLang: SourceLanguage;
  targetLang: TargetLanguage;
  maxRetries: number;
} {
  return {
    id: item.id,
    mediaPath: item.path,
    name: item.name,
    mediaKind: item.mediaKind,
    sizeBytes: item.sizeBytes,
    intent: toIntent(mode),
    sourceLang: normalizeSourceLanguage(item.sourceLang),
    targetLang: normalizeTargetLanguage(item.targetLang),
    maxRetries: 0,
  };
}

function toIntent(mode: QueueRunMode): "TRANSCRIBE" | "TRANSCRIBE_TRANSLATE" {
  if (mode === "transcribe_translate") return "TRANSCRIBE_TRANSLATE";
  return "TRANSCRIBE";
}
