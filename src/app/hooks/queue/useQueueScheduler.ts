import { useCallback, useEffect, useMemo } from "react";
import { deleteTaskSummaries } from "../../api/workspace";
import type { QueueItem } from "../../../features/media/types";
import type { AppAction } from "../../state/appReducer";
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
  runTranscribe: (item: QueueItem) => Promise<void>;
};

export function useQueueScheduler({
  queue,
  dispatch,
  pushToast,
  runTranscribe,
}: UseQueueSchedulerArgs) {
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
    const next = queue.find((item) => item.transcribeStatus === "queued");
    if (!next) return;
    void runTranscribe(next);
  }, [hasProcessingTask, queue, runTranscribe]);

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
      setQueuedState(dispatch, id);
    }

    pushToast(`开始批量处理，共 ${pendingCount} 个文件`, "info");
  }, [dispatch, pushToast, queue]);

  const processSingle = useCallback(async (item: QueueItem) => {
    if (item.transcribeStatus === "processing" || item.transcribeStatus === "queued") return;
    setQueuedState(dispatch, item.id);
    if (queueBusy) {
      pushToast(`已加入排队：${item.name}`, "info");
    }
  }, [dispatch, pushToast, queueBusy]);

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
    clearQueue,
    removeItem,
  };
}

