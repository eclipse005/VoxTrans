import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { deleteTasks, enqueueTaskRun } from "../../api/workspace";
import {
  createEmptyTaskProgress,
  type QueueItem,
} from "../../../features/media/types";
import {
  normalizeSourceLanguage,
  normalizeTargetLanguage,
} from "../../../features/media/languages";
import {
  isSubtitleQueueItem,
  toEnqueuePayload,
  type QueueRunMode,
} from "../../../features/media/queueUtils";
import type { AppAction } from "../../state/appReducer";
import {
  clearQueueItems,
  patchQueueItem,
  removeQueueItem,
} from "../../state/queueDomainActions";
import { reportError, toUserErrorMessage } from "../../utils/errors";
import { deleteRemoteBeforeLocalMutation } from "./queueDeleteCommit";

type DispatchState = (action: AppAction) => void;
type PushToast = (message: string, tone?: "info" | "success" | "error") => void;

type UseQueueSchedulerArgs = {
  queue: QueueItem[];
  dispatch: DispatchState;
  pushToast: PushToast;
  runQueuedByTaskIds: (taskIds: string[]) => Promise<void>;
};

export type QueueBatchMode = "transcribe" | "transcribe_translate";

export function useQueueScheduler({
  queue,
  dispatch,
  pushToast,
  runQueuedByTaskIds,
}: UseQueueSchedulerArgs) {
  const { t } = useTranslation(["toasts", "tasks"]);
  const isYoutubePlaceholder = useCallback(
    (item: QueueItem) => item.path.startsWith("youtube://"),
    [],
  );
  const runBatchInFlightRef = useRef(false);
  const [scheduleTick, setScheduleTick] = useState(0);
  const hasProcessingTask = useMemo(
    () =>
      queue.some(
        (item) =>
          item.transcribeStatus === "processing" && !isYoutubePlaceholder(item),
      ),
    [isYoutubePlaceholder, queue],
  );
  const hasQueuedTask = useMemo(
    () =>
      queue.some(
        (item) =>
          item.transcribeStatus === "queued" && !isYoutubePlaceholder(item),
      ),
    [isYoutubePlaceholder, queue],
  );
  const queueBusy = hasProcessingTask || hasQueuedTask;

  const enqueueForMode = useCallback(
    async (item: QueueItem, mode: QueueRunMode): Promise<boolean> => {
      try {
        const payload = toEnqueuePayload(item, mode);
        await enqueueTaskRun(payload);
        // State is updated via task-state-changed event from backend
        return true;
      } catch (error) {
        reportError(error, "enqueueTaskRun");
        const message = toUserErrorMessage(error, "toasts.queue.enqueueFailed");
        patchQueueItem(dispatch, item.id, (prev) => ({
          ...prev,
          transcribeStatus: "error",
          taskProgress: createEmptyTaskProgress(),
          transcribeError: message,
        }));
        pushToast(
          t("toasts:queue.enqueueFailure", { name: item.name, error: message }),
          "error",
        );
        return false;
      }
    },
    [dispatch, pushToast],
  );

  useEffect(() => {
    if (hasProcessingTask) return;
    if (runBatchInFlightRef.current) return;
    const queuedItems = queue.filter(
      (item) =>
        item.transcribeStatus === "queued" && !isYoutubePlaceholder(item),
    );
    if (queuedItems.length === 0) return;
    runBatchInFlightRef.current = true;
    void runQueuedByTaskIds(queuedItems.map((item) => item.id))
      .catch(() => {
        pushToast(t("toasts:queue.batchExecuteFailed"), "error");
      })
      .finally(() => {
        runBatchInFlightRef.current = false;
        setScheduleTick((value) => value + 1);
      });
  }, [
    hasProcessingTask,
    isYoutubePlaceholder,
    queue,
    runQueuedByTaskIds,
    pushToast,
    scheduleTick,
  ]);

  const processQueue = useCallback(
    async (mode: QueueBatchMode = "transcribe") => {
      const retryableItems = queue.filter(
        (item) =>
          (item.transcribeStatus === "pending" ||
            item.transcribeStatus === "error") &&
          !isYoutubePlaceholder(item),
      );
      if (!retryableItems.length) {
        pushToast(t("toasts:queue.nothingPending"), "error");
        return;
      }

      let queuedCount = 0;
      let skippedSrtOnTranscribe = 0;
      for (const item of retryableItems) {
        // Batch "transcribe only" does not apply to SRT tasks — skip them.
        if (mode === "transcribe" && isSubtitleQueueItem(item)) {
          skippedSrtOnTranscribe += 1;
          continue;
        }
        const resolvedMode: QueueRunMode =
          mode === "transcribe" ? "transcribe" : "transcribe_translate";
        if (await enqueueForMode(item, resolvedMode)) {
          queuedCount += 1;
        }
      }

      if (skippedSrtOnTranscribe > 0) {
        pushToast(
          t("toasts:queue.srtSkippedOnTranscribeBatch", {
            count: skippedSrtOnTranscribe,
          }),
          "info",
        );
      }

      // Pure SRT queue under "transcribe only": skip is intentional, not a failure.
      if (queuedCount === 0) {
        if (skippedSrtOnTranscribe > 0) {
          return;
        }
        pushToast(t("toasts:queue.enqueueAllFailed"), "error");
        return;
      }

      const modeLabel =
        mode === "transcribe"
          ? t("tasks:queue.modeTranscribe")
          : t("tasks:queue.modeTranscribeTranslate");
      pushToast(
        t("toasts:queue.batchStarted", { mode: modeLabel, count: queuedCount }),
        "info",
      );
    },
    [enqueueForMode, isYoutubePlaceholder, pushToast, queue],
  );

  const processSingle = useCallback(
    async (item: QueueItem) => {
      if (isYoutubePlaceholder(item)) return;
      if (
        item.transcribeStatus === "processing" ||
        item.transcribeStatus === "queued"
      )
        return;
      if (isSubtitleQueueItem(item)) {
        pushToast(t("toasts:queue.srtUseTranslateOnly"), "info");
        return;
      }
      const mode: QueueRunMode = "transcribe";
      const ok = await enqueueForMode(item, mode);
      if (!ok) return;
      pushToast(
        queueBusy
          ? t("toasts:queue.addedToQueue", { name: item.name })
          : t("toasts:queue.started", { name: item.name }),
        "info",
      );
    },
    [enqueueForMode, isYoutubePlaceholder, pushToast, queueBusy, t],
  );

  const processSingleTranscribeTranslate = useCallback(
    async (item: QueueItem) => {
      if (isYoutubePlaceholder(item)) return;
      if (
        item.transcribeStatus === "processing" ||
        item.transcribeStatus === "queued"
      )
        return;
      // SRT items always map to TRANSLATE_SRT inside enqueueForMode.
      const mode: QueueRunMode = "transcribe_translate";
      const ok = await enqueueForMode(item, mode);
      if (!ok) return;
      pushToast(
        queueBusy
          ? t("toasts:queue.addedToQueue", { name: item.name })
          : t("toasts:queue.started", { name: item.name }),
        "info",
      );
    },
    [enqueueForMode, isYoutubePlaceholder, pushToast, queueBusy, t],
  );

  const clearQueue = useCallback(async (): Promise<boolean> => {
    if (queueBusy) {
      pushToast(t("toasts:queue.clearWhileBusy"), "error");
      return false;
    }
    try {
      await deleteRemoteBeforeLocalMutation(
        () => deleteTasks({ taskId: null, mediaPath: null }),
        () => clearQueueItems(dispatch),
      );
      pushToast(t("toasts:queue.cleared"), "info");
      return true;
    } catch (error) {
      reportError(error, "clearQueue");
      pushToast(toUserErrorMessage(error, "toasts.queue.clearFailed"), "error");
      return false;
    }
  }, [dispatch, pushToast, queueBusy, t]);

  const removeItem = useCallback(
    async (id: string) => {
      const item = queue.find((q) => q.id === id);
      if (!item) {
        return;
      }
      if (item.transcribeStatus === "processing" || item.transcribeStatus === "queued") {
        pushToast(t("toasts:queue.deleteWhileBusy"), "error");
        return;
      }
      try {
        await deleteRemoteBeforeLocalMutation(
          () => deleteTasks({ taskId: item.id, mediaPath: item.path }),
          () => removeQueueItem(dispatch, id),
        );
      } catch (error) {
        reportError(error, "removeItem");
        pushToast(toUserErrorMessage(error, "toasts.queue.deleteFailed"), "error");
      }
    },
    [dispatch, pushToast, queue, t],
  );

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
