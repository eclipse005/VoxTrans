import { useCallback, useEffect, useMemo, useRef } from "react";
import { deleteTaskSummaries, enqueueTaskRun } from "../../api/workspace";
import type { QueueItem, SavedSettings } from "../../../features/media/types";
import type { AppAction } from "../../state/appReducer";
import type { QueueRunMode } from "./useQueueRunner";
import {
  clearQueueItems,
  patchQueueItem,
  removeQueueItem,
  setQueuedState,
} from "../../state/queueDomainActions";
import { reportError, toUserErrorMessage } from "../../utils/errors";

type DispatchState = (action: AppAction) => void;
type PushToast = (message: string, tone?: "info" | "success" | "error") => void;

type UseQueueSchedulerArgs = {
  queue: QueueItem[];
  settings: SavedSettings;
  dispatch: DispatchState;
  pushToast: PushToast;
  runBatch: (items: Array<{ item: QueueItem; mode: QueueRunMode }>) => Promise<void>;
  setTaskMode: (taskId: string, mode: QueueRunMode) => void;
  takeTaskMode: (taskId: string) => QueueRunMode;
};

export function useQueueScheduler({
  queue,
  settings,
  dispatch,
  pushToast,
  runBatch,
  setTaskMode,
  takeTaskMode,
}: UseQueueSchedulerArgs) {
  const runBatchInFlightRef = useRef(false);
  const hasProcessingTask = useMemo(
    () => queue.some((item) => item.transcribeStatus === "processing"),
    [queue],
  );
  const hasQueuedTask = useMemo(
    () => queue.some((item) => item.transcribeStatus === "queued"),
    [queue],
  );
  const queueBusy = hasProcessingTask || hasQueuedTask;

  const enqueueForMode = useCallback(async (item: QueueItem, mode: QueueRunMode): Promise<boolean> => {
    try {
      await enqueueTaskRun({
        id: item.id,
        mediaPath: item.path,
        name: item.name,
        mediaKind: item.mediaKind,
        sizeBytes: item.sizeBytes,
        intent: mode === "translate_only"
          ? "TRANSLATE_ONLY"
          : mode === "transcribe_translate"
            ? "TRANSCRIBE_TRANSLATE"
            : "TRANSCRIBE",
        sourceLang: "auto",
        targetLang: "zh-CN",
        maxRetries: 0,
        settingsSnapshot: buildSettingsSnapshot(settings),
      });
      setTaskMode(item.id, mode);
      setQueuedState(dispatch, item.id);
      return true;
    } catch (error) {
      reportError(error, "enqueueTaskRun");
      const message = toUserErrorMessage(error, "任务入队失败");
      patchQueueItem(dispatch, item.id, (prev) => ({
        ...prev,
        transcribeStatus: "error",
        transcribeProgress: 0,
        transcribeSegmentCurrent: 0,
        transcribeSegmentTotal: 0,
        transcribePhase: "",
        transcribeError: message,
      }));
      pushToast(`失败：${item.name}，${message}`, "error");
      return false;
    }
  }, [dispatch, pushToast, setTaskMode, settings]);

  useEffect(() => {
    if (hasProcessingTask) return;
    if (runBatchInFlightRef.current) return;
    const queuedItems = queue.filter((item) => item.transcribeStatus === "queued");
    if (queuedItems.length === 0) return;
    runBatchInFlightRef.current = true;
    const batch = queuedItems.map((item) => ({
      item,
      mode: takeTaskMode(item.id),
    }));
    void runBatch(batch)
      .catch(() => {
        pushToast("批处理执行失败，请重试", "error");
      })
      .finally(() => {
        runBatchInFlightRef.current = false;
      });
  }, [hasProcessingTask, queue, runBatch, takeTaskMode, pushToast]);

  const processQueue = useCallback(async () => {
    const pendingItems = queue.filter((item) => item.transcribeStatus === "pending");
    if (!pendingItems.length) {
      pushToast("没有待处理文件", "error");
      return;
    }

    let queuedCount = 0;
    for (const item of pendingItems) {
      if (await enqueueForMode(item, "transcribe")) {
        queuedCount += 1;
      }
    }

    if (queuedCount === 0) {
      pushToast("没有可处理文件，入队均失败", "error");
      return;
    }

    pushToast(`开始批量处理，共 ${queuedCount} 个文件`, "info");
  }, [enqueueForMode, pushToast, queue]);

  const processSingle = useCallback(async (item: QueueItem) => {
    if (item.transcribeStatus === "processing" || item.transcribeStatus === "queued") return;
    const ok = await enqueueForMode(item, "transcribe");
    if (!ok) return;
    if (queueBusy) {
      pushToast(`已加入排队：${item.name}`, "info");
    }
  }, [enqueueForMode, pushToast, queueBusy]);

  const processSingleTranscribeTranslate = useCallback(async (item: QueueItem) => {
    if (item.transcribeStatus === "processing" || item.transcribeStatus === "queued") return;
    const mode = resolveTranslateMode(item);
    const ok = await enqueueForMode(item, mode);
    if (!ok) return;
    if (queueBusy) {
      pushToast(`已加入排队：${item.name}`, "info");
    }
  }, [enqueueForMode, pushToast, queueBusy]);

  const clearQueue = useCallback(async () => {
    if (queueBusy) {
      pushToast("正在处理时不能清空队列", "error");
      return;
    }
    clearQueueItems(dispatch);
    try {
      await deleteTaskSummaries({ taskId: null, mediaPath: null });
    } catch {
      // Queue is already cleared in UI; ignore history cleanup failure.
    }
    pushToast("队列已清空", "info");
  }, [dispatch, pushToast, queueBusy]);

  const removeItem = useCallback(async (id: string) => {
    const item = queue.find((q) => q.id === id);
    if (!item) {
      return;
    }
    try {
      await deleteTaskSummaries({ taskId: item.id, mediaPath: item.path });
      removeQueueItem(dispatch, id);
    } catch (error) {
      reportError(error, "removeItem");
      pushToast(toUserErrorMessage(error, "删除任务失败"), "error");
    }
  }, [dispatch, pushToast, queue]);

  return {
    queueCount: queue.length,
    queueBusy,
    processQueue,
    processSingle,
    processSingleTranscribeTranslate,
    clearQueue,
    removeItem,
  };
}

function resolveTranslateMode(item: QueueItem): QueueRunMode {
  const segments = parseSubtitleSegments(item.subtitleSegmentsJson);
  const hasSource = segments.some((segment) => segment.sourceText.trim().length > 0);
  const hasTranslated = segments.some((segment) => segment.translatedText.trim().length > 0);
  if (hasSource && !hasTranslated) {
    return "translate_only";
  }
  return "transcribe_translate";
}

function parseSubtitleSegments(raw?: string): Array<{ sourceText: string; translatedText: string }> {
  if (!raw?.trim()) return [];
  try {
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed.map((segment) => ({
      sourceText: typeof segment?.sourceText === "string" ? segment.sourceText : "",
      translatedText: typeof segment?.translatedText === "string" ? segment.translatedText : "",
    }));
  } catch {
    return [];
  }
}

function buildSettingsSnapshot(settings: SavedSettings): Record<string, unknown> {
  return {
    provider: settings.provider,
    chunkTargetSeconds: settings.chunkTargetSeconds,
    subtitleMaxWordsPerSegment: settings.subtitleMaxWordsPerSegment,
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
  };
}
