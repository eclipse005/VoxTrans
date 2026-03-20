import { useCallback, useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  enqueueAndExecuteTaskBatch,
  loadWorkspaceState,
} from "../../api/workspace";
import type {
  QueueItem,
  SavedSettings,
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
  applyTranslateProgress,
  setQueuedState,
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
  phaseDetail?: string;
};

type SeparateProgressEvent = {
  taskId: string;
  percent: number;
};

type TranslateProgressEvent = {
  taskId: string;
  currentBatch: number;
  totalBatches: number;
};

export type QueueRunMode = "transcribe" | "transcribe_translate" | "translate_only";

type UseQueueRunnerArgs = {
  dispatch: DispatchState;
  pushToast: PushToast;
  isTaskPresent: (taskId: string) => boolean;
  settings: SavedSettings;
};

export function useQueueRunner({
  dispatch,
  pushToast,
  isTaskPresent,
  settings,
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
        phaseDetail: payload.phaseDetail,
      });
      if (payload.phase === "summarize" || payload.phase === "translate" || payload.phase === "qa") {
        void (async () => {
          try {
            const workspace = await loadWorkspaceState();
            syncQueueItem(dispatch, isTaskPresent, workspace, payload.taskId);
          } catch {
            // Ignore sync error during phase update; task flow continues.
          }
        })();
      }
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
  }, [dispatch, isTaskPresent]);

  useEffect(() => {
    let disposed = false;
    let unlistenTranslateProgress: undefined | (() => void);
    listen<TranslateProgressEvent>("translate-progress", (event) => {
      const payload = event.payload;
      if (!payload?.taskId) return;
      applyTranslateProgress(dispatch, {
        taskId: payload.taskId,
        currentBatch: payload.currentBatch,
        totalBatches: payload.totalBatches,
      });
    })
      .then((fn) => {
        if (disposed) {
          fn();
          return;
        }
        unlistenTranslateProgress = fn;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      if (unlistenTranslateProgress) unlistenTranslateProgress();
    };
  }, [dispatch]);

  const runTask = useCallback(async (item: QueueItem, mode: QueueRunMode) => {
    if (!isTaskPresent(item.id)) return;
    setQueuedState(dispatch, item.id);
    if (!isTaskPresent(item.id)) return;
    setProcessingState(dispatch, item.id);
    if (!isTaskPresent(item.id)) return;

    try {
      const response = await enqueueAndExecuteTaskBatch({
        items: [toEnqueuePayload(item, mode, settings)],
      });
      const workspace = await loadWorkspaceState();
      syncQueueItem(dispatch, isTaskPresent, workspace, item.id);

      const failed = response.failed.find((entry) => entry.taskId === item.id);
      if (failed) {
        if (!isTaskPresent(item.id)) return;
        setErrorState(dispatch, item.id, failed.error || "任务执行失败");
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
      setErrorState(dispatch, item.id, errorMessage);
      pushToast(`失败：${item.name}，${errorMessage}`, "error");
    }
  }, [
    dispatch,
    isTaskPresent,
    pushToast,
    settings,
  ]);
  const runBatch = useRunBatch(dispatch, pushToast, isTaskPresent, settings);

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
  settings: SavedSettings,
) {
  return useCallback(async (tasks: BatchTask[]) => {
    const items = tasks.filter((task) => isTaskPresent(task.item.id));
    if (!items.length) return;
    for (const task of items) {
      if (!isTaskPresent(task.item.id)) continue;
      setProcessingState(dispatch, task.item.id);
    }

    const response = await enqueueAndExecuteTaskBatch({
      items: items.map((task) => toEnqueuePayload(task.item, task.mode, settings)),
    });
    const workspace = await loadWorkspaceState();
    for (const item of items) {
      syncQueueItem(dispatch, isTaskPresent, workspace, item.item.id);
    }

    for (const failure of response.failed) {
      if (!isTaskPresent(failure.taskId)) continue;
      setErrorState(dispatch, failure.taskId, failure.error || "任务执行失败");
    }

    if (response.failed.length > 0) {
      const first = response.failed[0];
      pushToast(`部分任务失败：${first.taskId}，${first.error}`, "error");
    }
  }, [dispatch, isTaskPresent, pushToast, settings]);
}

function toEnqueuePayload(
  item: QueueItem,
  mode: QueueRunMode,
  settings: SavedSettings,
): {
  id: string;
  mediaPath: string;
  name: string;
  mediaKind: "audio" | "video";
  sizeBytes: number;
  intent: "TRANSCRIBE" | "TRANSCRIBE_TRANSLATE" | "TRANSLATE_ONLY";
  sourceLang: string;
  targetLang: string;
  maxRetries: number;
  settingsSnapshot: Record<string, unknown>;
} {
  return {
    id: item.id,
    mediaPath: item.path,
    name: item.name,
    mediaKind: item.mediaKind,
    sizeBytes: item.sizeBytes,
    intent: toIntent(mode),
    sourceLang: "auto",
    targetLang: "zh-CN",
    maxRetries: 0,
    settingsSnapshot: buildSettingsSnapshot(settings),
  };
}

function syncQueueItem(
  dispatch: DispatchState,
  isTaskPresent: (taskId: string) => boolean,
  workspace: WorkspaceStateResponse,
  taskId: string,
): void {
  if (!isTaskPresent(taskId)) return;
  const synced = findQueueItem(workspace, taskId);
  if (!synced) return;
  patchQueueItem(dispatch, taskId, (prev) => ({
    ...prev,
    transcribeStatus: synced.transcribeStatus,
    transcribeProgress: synced.transcribeProgress,
    transcribeSegmentCurrent: synced.transcribeSegmentCurrent,
    transcribeSegmentTotal: synced.transcribeSegmentTotal,
    transcribePhase: synced.transcribePhase,
    transcribePhaseDetail: synced.transcribePhaseDetail,
    transcribeError: synced.transcribeError,
    resultText: synced.resultText,
    resultSrt: synced.resultSrt,
    subtitleSegmentsJson: synced.subtitleSegmentsJson,
  }));
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

function buildSettingsSnapshot(settings: SavedSettings): Record<string, unknown> {
  return {
    provider: settings.provider,
    chunkTargetSeconds: settings.chunkTargetSeconds,
    subtitleMaxWordsPerSegment: settings.subtitleMaxWordsPerSegment,
    subtitleLengthReference: settings.subtitleLengthReference,
    asrModel: settings.asrModel,
    demucsModel: settings.demucsModel,
    enableVocalSeparation: settings.enableVocalSeparation,
    translateApiKey: settings.translateApiKey,
    translateBaseUrl: settings.translateBaseUrl,
    translateModel: settings.translateModel,
    llmConcurrency: settings.llmConcurrency,
    terminologyGroups: settings.terminologyGroups,
    enableTerminology: settings.enableTerminology,
    enablePunctuationOptimization: settings.enablePunctuationOptimization,
    enableAsrCorrection: settings.enableAsrCorrection,
    enableSubtitleBeautify: settings.enableSubtitleBeautify,
  };
}
