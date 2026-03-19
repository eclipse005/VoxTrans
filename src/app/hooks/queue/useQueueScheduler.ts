import { useCallback, useEffect, useMemo, useRef } from "react";
import { deleteTaskSummaries } from "../../api/workspace";
import type { QueueItem } from "../../../features/media/types";
import type { AppAction } from "../../state/appReducer";
import type { QueueRunMode } from "./useQueueRunner";
import {
  clearQueueItems,
  removeQueueItem,
  setQueuedState,
} from "../../state/queueDomainActions";

type DispatchState = (action: AppAction) => void;
type PushToast = (message: string, tone?: "info" | "success" | "error") => void;

type UseQueueSchedulerArgs = {
  queue: QueueItem[];
  dispatch: DispatchState;
  pushToast: PushToast;
  runBatch: (items: Array<{ item: QueueItem; mode: QueueRunMode }>) => Promise<void>;
  setTaskMode: (taskId: string, mode: QueueRunMode) => void;
  takeTaskMode: (taskId: string) => QueueRunMode;
};

export function useQueueScheduler({
  queue,
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
    const pendingCount = queue.filter((item) => item.transcribeStatus === "pending").length;
    if (!pendingCount) {
      pushToast("没有待处理文件", "error");
      return;
    }

    const queuedIds = queue
      .filter((q) => q.transcribeStatus === "pending")
      .map((q) => q.id);

    for (const id of queuedIds) {
      setTaskMode(id, "transcribe");
      setQueuedState(dispatch, id);
    }

    pushToast(`开始批量处理，共 ${pendingCount} 个文件`, "info");
  }, [dispatch, pushToast, queue, setTaskMode]);

  const processSingle = useCallback(async (item: QueueItem) => {
    if (item.transcribeStatus === "processing" || item.transcribeStatus === "queued") return;
    setTaskMode(item.id, "transcribe");
    setQueuedState(dispatch, item.id);
    if (queueBusy) {
      pushToast(`已加入排队：${item.name}`, "info");
    }
  }, [dispatch, pushToast, queueBusy, setTaskMode]);

  const processSingleTranscribeTranslate = useCallback(async (item: QueueItem) => {
    if (item.transcribeStatus === "processing" || item.transcribeStatus === "queued") return;
    setTaskMode(item.id, item.transcribeStatus === "done" ? "translate_only" : "transcribe_translate");
    setQueuedState(dispatch, item.id);
    if (queueBusy) {
      pushToast(`已加入排队：${item.name}`, "info");
    }
  }, [dispatch, pushToast, queueBusy, setTaskMode]);

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

  const removeItem = useCallback((id: string) => {
    const item = queue.find((q) => q.id === id);
    if (item) {
      void deleteTaskSummaries({ taskId: item.id, mediaPath: item.path });
    }
    removeQueueItem(dispatch, id);
  }, [dispatch, queue]);

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
