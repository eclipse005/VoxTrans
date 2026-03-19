import { useCallback, useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { executeTaskBatch, executeTaskRun, loadWorkspaceState } from "../../api/workspace";
import type {
  QueueItem,
  TranscribePhase,
  WorkspaceStateResponse,
} from "../../../features/media/types";
import type { AppAction } from "../../state/appReducer";
import {
  applyTranscribePhase,
  applyTranscribeProgress,
  applySeparationProgress,
  patchQueueItem,
  setErrorState,
  setProcessingState,
} from "../../state/queueDomainActions";
import { reportError, toUserErrorMessage } from "../../utils/errors";

type DispatchState = (action: AppAction) => void;
type PushToast = (message: string, tone?: "info" | "success" | "error") => void;

type TranscribeProgressEvent = {
  taskId: string;
  currentSegment: number;
  totalSegments: number;
};

type TranscribePhaseEvent = {
  taskId: string;
  phase: TranscribePhase;
};

type SeparateProgressEvent = {
  taskId: string;
  percent: number;
};

export type QueueRunMode = "transcribe" | "transcribe_translate" | "translate_only";

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
  useEffect(() => {
    let disposed = false;
    let unlistenProgress: undefined | (() => void);

    listen<TranscribeProgressEvent>("transcribe-progress", (event) => {
      const payload = event.payload;
      if (!payload?.taskId) return;
      applyTranscribeProgress(dispatch, {
        taskId: payload.taskId,
        currentSegment: payload.currentSegment,
        totalSegments: payload.totalSegments,
      });
    })
      .then((fn) => {
        if (disposed) {
          fn();
          return;
        }
        unlistenProgress = fn;
      })
      .catch(() => {});

    return () => {
      disposed = true;
      if (unlistenProgress) unlistenProgress();
    };
  }, [dispatch]);

  useEffect(() => {
    let disposed = false;
    let unlistenSeparation: undefined | (() => void);
    listen<SeparateProgressEvent>("separate-progress", (event) => {
      const payload = event.payload;
      if (!payload?.taskId) return;
      applySeparationProgress(dispatch, {
        taskId: payload.taskId,
        percent: payload.percent,
      });
    })
      .then((fn) => {
        if (disposed) {
          fn();
          return;
        }
        unlistenSeparation = fn;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      if (unlistenSeparation) unlistenSeparation();
    };
  }, [dispatch]);

  useEffect(() => {
    let disposed = false;
    let unlistenPhase: undefined | (() => void);
    listen<TranscribePhaseEvent>("transcribe-phase", (event) => {
      const payload = event.payload;
      if (!payload?.taskId) return;
      applyTranscribePhase(dispatch, {
        taskId: payload.taskId,
        phase: payload.phase,
      });
    })
      .then((fn) => {
        if (disposed) {
          fn();
          return;
        }
        unlistenPhase = fn;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      if (unlistenPhase) unlistenPhase();
    };
  }, [dispatch]);

  const runTask = useCallback(async (item: QueueItem, mode: QueueRunMode) => {
    if (!isTaskPresent(item.id)) return;
    setProcessingState(dispatch, item.id);
    if (!isTaskPresent(item.id)) return;

    try {
      await executeTaskRun({
        taskId: item.id,
        intent: toIntent(mode),
      });
      if (!isTaskPresent(item.id)) return;

      const workspace = await loadWorkspaceState();
      if (!isTaskPresent(item.id)) return;
      const synced = findQueueItem(workspace, item.id);
      if (synced) {
        patchQueueItem(dispatch, item.id, (prev) => ({
          ...prev,
          transcribeStatus: synced.transcribeStatus,
          transcribeProgress: synced.transcribeProgress,
          transcribeSegmentCurrent: synced.transcribeSegmentCurrent,
          transcribeSegmentTotal: synced.transcribeSegmentTotal,
          transcribePhase: "",
          transcribeError: synced.transcribeError,
          resultText: synced.resultText,
          resultSrt: synced.resultSrt,
          subtitleSegmentsJson: synced.subtitleSegmentsJson,
        }));
      }

      pushToast(mode === "transcribe" ? `已完成：${item.name}` : `已完成转译：${item.name}`, "success");
    } catch (err) {
      if (!isTaskPresent(item.id)) return;
      reportError(err, "runTask");
      const fallback = mode === "transcribe" ? "转录失败，请检查模型和运行时配置" : "转译失败，请检查翻译配置";
      const errorMessage = toUserErrorMessage(err, fallback);
      setErrorState(dispatch, item.id, errorMessage);
      pushToast(`失败：${item.name}，${errorMessage}`, "error");
    }
  }, [
    dispatch,
    isTaskPresent,
    pushToast,
  ]);
  const runBatch = useRunBatch(dispatch, pushToast, isTaskPresent);

  return {
    runTask,
    runBatch,
  };
}

type BatchTask = {
  item: QueueItem;
  mode: QueueRunMode;
};

function useRunBatch(
  dispatch: DispatchState,
  pushToast: PushToast,
  isTaskPresent: (taskId: string) => boolean,
) {
  return useCallback(async (tasks: BatchTask[]) => {
    const items = tasks.filter((task) => isTaskPresent(task.item.id));
    if (!items.length) return;

    const response = await executeTaskBatch({
      items: items.map((task) => ({
        taskId: task.item.id,
        intent: toIntent(task.mode),
      })),
    });
    const workspace = await loadWorkspaceState();
    for (const item of items) {
      if (!isTaskPresent(item.item.id)) continue;
      const synced = findQueueItem(workspace, item.item.id);
      if (!synced) continue;
      patchQueueItem(dispatch, item.item.id, (prev) => ({
        ...prev,
        transcribeStatus: synced.transcribeStatus,
        transcribeProgress: synced.transcribeProgress,
        transcribeSegmentCurrent: synced.transcribeSegmentCurrent,
        transcribeSegmentTotal: synced.transcribeSegmentTotal,
        transcribePhase: "",
        transcribeError: synced.transcribeError,
        resultText: synced.resultText,
        resultSrt: synced.resultSrt,
        subtitleSegmentsJson: synced.subtitleSegmentsJson,
      }));
    }
    if (response.failed.length > 0) {
      const first = response.failed[0];
      pushToast(`部分任务失败：${first.taskId}，${first.error}`, "error");
    }
  }, [dispatch, isTaskPresent, pushToast]);
}

function toIntent(mode: QueueRunMode): "TRANSCRIBE" | "TRANSCRIBE_TRANSLATE" | "TRANSLATE_ONLY" {
  if (mode === "translate_only") return "TRANSLATE_ONLY";
  if (mode === "transcribe_translate") return "TRANSCRIBE_TRANSLATE";
  return "TRANSCRIBE";
}

function findQueueItem(
  workspace: WorkspaceStateResponse,
  taskId: string,
): QueueItem | null {
  const queue = Array.isArray(workspace.queue) ? workspace.queue : [];
  return queue.find((item) => item.id === taskId) ?? null;
}
