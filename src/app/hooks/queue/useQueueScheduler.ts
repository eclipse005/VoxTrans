import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { deleteTasks, enqueueTaskRun } from "../../api/workspace";
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

export type QueueBatchMode = "transcribe" | "transcribe_translate";

export function useQueueScheduler({
  queue,
  settings,
  dispatch,
  pushToast,
  runBatch,
  setTaskMode,
  takeTaskMode,
}: UseQueueSchedulerArgs) {
  const isYoutubePlaceholder = useCallback((item: QueueItem) => (
    item.path.startsWith("youtube://")
  ), []);
  const runBatchInFlightRef = useRef(false);
  const [scheduleTick, setScheduleTick] = useState(0);
  const hasProcessingTask = useMemo(
    () => queue.some((item) => item.transcribeStatus === "processing" && !isYoutubePlaceholder(item)),
    [isYoutubePlaceholder, queue],
  );
  const hasQueuedTask = useMemo(
    () => queue.some((item) => item.transcribeStatus === "queued" && !isYoutubePlaceholder(item)),
    [isYoutubePlaceholder, queue],
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
        intent: mode === "transcribe_translate"
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
        transcribePhaseDetail: "",
        transcribeError: message,
      }));
      pushToast(`失败：${item.name}，${message}`, "error");
      return false;
    }
  }, [dispatch, pushToast, setTaskMode, settings]);

  useEffect(() => {
    if (hasProcessingTask) return;
    if (runBatchInFlightRef.current) return;
    const queuedItems = queue.filter((item) => item.transcribeStatus === "queued" && !isYoutubePlaceholder(item));
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
        setScheduleTick((value) => value + 1);
      });
  }, [hasProcessingTask, isYoutubePlaceholder, queue, runBatch, takeTaskMode, pushToast, scheduleTick]);

  const processQueue = useCallback(async (mode: QueueBatchMode = "transcribe") => {
    const retryableItems = queue.filter((item) => (
      (item.transcribeStatus === "pending" || item.transcribeStatus === "error")
      && !isYoutubePlaceholder(item)
    ));
    if (!retryableItems.length) {
      pushToast("没有待处理文件", "error");
      return;
    }

    let queuedCount = 0;
    for (const item of retryableItems) {
      const resolvedMode = mode === "transcribe" ? "transcribe" : "transcribe_translate";
      if (await enqueueForMode(item, resolvedMode)) {
        queuedCount += 1;
      }
    }

    if (queuedCount === 0) {
      pushToast("没有可处理文件，入队均失败", "error");
      return;
    }

    const modeLabel = mode === "transcribe" ? "转录" : "转译";
    pushToast(`开始批量${modeLabel}，共 ${queuedCount} 个文件`, "info");
  }, [enqueueForMode, isYoutubePlaceholder, pushToast, queue]);

  const processSingle = useCallback(async (item: QueueItem) => {
    if (isYoutubePlaceholder(item)) return;
    if (item.transcribeStatus === "processing" || item.transcribeStatus === "queued") return;
    const ok = await enqueueForMode(item, "transcribe");
    if (!ok) return;
    if (queueBusy) {
      pushToast(`已加入排队：${item.name}`, "info");
    }
  }, [enqueueForMode, isYoutubePlaceholder, pushToast, queueBusy]);

  const processSingleTranscribeTranslate = useCallback(async (item: QueueItem) => {
    if (isYoutubePlaceholder(item)) return;
    if (item.transcribeStatus === "processing" || item.transcribeStatus === "queued") return;
    const ok = await enqueueForMode(item, "transcribe_translate");
    if (!ok) return;
    if (queueBusy) {
      pushToast(`已加入排队：${item.name}`, "info");
    }
  }, [enqueueForMode, isYoutubePlaceholder, pushToast, queueBusy]);

  const clearQueue = useCallback(async () => {
    if (queueBusy) {
      pushToast("正在处理时不能清空队列", "error");
      return;
    }
    clearQueueItems(dispatch);
    try {
      await deleteTasks({ taskId: null, mediaPath: null });
    } catch {
      // Queue is already cleared in UI; ignore backend cleanup failure.
    }
    pushToast("队列已清空", "info");
  }, [dispatch, pushToast, queueBusy]);

  const removeItem = useCallback(async (id: string) => {
    const item = queue.find((q) => q.id === id);
    if (!item) {
      return;
    }
    try {
      await deleteTasks({ taskId: item.id, mediaPath: item.path });
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
