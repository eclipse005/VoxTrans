import { useCallback, useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  enqueueAndExecuteTaskBatch,
} from "../../api/workspace";
import type {
  QueueItem,
  SavedSettings,
  TranscribePhase,
} from "../../../features/media/types";
import type { AppAction } from "../../state/appReducer";
import {
  applyTranscribePhase,
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
  transcribeStatus: string;
  transcribeProgress: number;
  transcribeSegmentCurrent: number;
  transcribeSegmentTotal: number;
  transcribePhase: string;
  transcribePhaseDetail: string;
  transcribeError: string;
  resultText: string;
  resultSrt: string;
  subtitleSegmentsJson: string;
};

// Transcribe phase event for stage transitions
type TranscribePhaseEvent = {
  taskId: string;
  phase: TranscribePhase;
  phaseDetail?: string;
};

export type QueueRunMode = "transcribe" | "transcribe_translate";

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
  // Listen for task-state-changed events from backend (single source of truth)
  useEffect(() => {
    let disposed = false;
    let unlistenTaskStateChanged: undefined | (() => void);

    listen<TaskStateChangedEvent>("task-state-changed", (event) => {
      const payload = event.payload;
      if (!payload?.id) return;
      if (!isTaskPresent(payload.id)) return;
      // Backend is the single source of truth - replace local state directly
      patchQueueItem(dispatch, payload.id, () => ({
        id: payload.id,
        path: payload.path,
        name: payload.name,
        mediaKind: payload.mediaKind as "audio" | "video",
        sizeBytes: payload.sizeBytes,
        transcribeStatus: payload.transcribeStatus as QueueItem["transcribeStatus"],
        transcribeProgress: payload.transcribeProgress,
        transcribeSegmentCurrent: payload.transcribeSegmentCurrent,
        transcribeSegmentTotal: payload.transcribeSegmentTotal,
        transcribePhase: (payload.transcribePhase || "") as TranscribePhase | "",
        transcribePhaseDetail: payload.transcribePhaseDetail || "",
        transcribeError: payload.transcribeError || "",
        resultText: payload.resultText || "",
        resultSrt: payload.resultSrt || "",
        subtitleSegmentsJson: payload.subtitleSegmentsJson || "",
      }));
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

  // Listen for transcribe-phase events for stage transitions
  useEffect(() => {
    let disposed = false;
    let unlistenPhase: undefined | (() => void);

    listen<TranscribePhaseEvent>("transcribe-phase", (event) => {
      const payload = event.payload;
      if (!payload?.taskId) return;
      if (!isTaskPresent(payload.taskId)) return;
      // Update phase display - full state comes via task-state-changed
      applyTranscribePhase(dispatch, {
        taskId: payload.taskId,
        phase: payload.phase,
        phaseDetail: payload.phaseDetail,
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
  }, [dispatch, isTaskPresent]);

  const runTask = useCallback(async (item: QueueItem, mode: QueueRunMode) => {
    if (!isTaskPresent(item.id)) return;

    try {
      const response = await enqueueAndExecuteTaskBatch({
        items: [toEnqueuePayload(item, mode, settings)],
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
    settings,
  ]);
  const runBatch = useRunBatch(pushToast, isTaskPresent, settings);

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
  pushToast: PushToast,
  isTaskPresent: (taskId: string) => boolean,
  settings: SavedSettings,
) {
  return useCallback(async (tasks: BatchTask[]) => {
    const items = tasks.filter((task) => isTaskPresent(task.item.id));
    if (!items.length) return;

    const response = await enqueueAndExecuteTaskBatch({
      items: items.map((task) => toEnqueuePayload(task.item, task.mode, settings)),
    });
    // State is updated via task-state-changed event from backend

    if (response.failed.length > 0) {
      const first = response.failed[0];
      pushToast(`部分任务失败：${first.taskId}，${first.error}`, "error");
    }
  }, [pushToast, isTaskPresent, settings]);
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
  intent: "TRANSCRIBE" | "TRANSCRIBE_TRANSLATE";
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

function toIntent(mode: QueueRunMode): "TRANSCRIBE" | "TRANSCRIBE_TRANSLATE" {
  if (mode === "transcribe_translate") return "TRANSCRIBE_TRANSLATE";
  return "TRANSCRIBE";
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
    enableSubtitleBeautify: settings.enableSubtitleBeautify,
  };
}
