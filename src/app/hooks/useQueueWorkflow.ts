import { useCallback, useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { useQueueInput } from "./queue/useQueueInput";
import { useQueueRunner } from "./queue/useQueueRunner";
import { useQueueScheduler } from "./queue/useQueueScheduler";
import { useYoutubeDownloadWorkflow } from "./useYoutubeDownloadWorkflow";
import { useYtDlpManager } from "./useYtDlpManager";
import {
  normalizeSourceLanguage,
  normalizeTargetLanguage,
} from "../../features/media/languages";
import type {
  LanguageTag,
  QueueItem,
  TargetLanguage,
} from "../../features/media/types";
import type { AppAction } from "../state/appReducer";
import type { QueueBatchMode } from "./queue/useQueueScheduler";
import { updateTaskLanguages, updateTaskTerminology } from "../api/workspace";
import { patchQueueItem } from "../state/queueDomainActions";
import { reportError, toUserErrorMessage } from "../utils/errors";

type DispatchState = (action: AppAction) => void;
type PushToast = (message: string, tone?: "info" | "success" | "error") => void;

type UseQueueWorkflowArgs = {
  queue: QueueItem[];
  dispatch: DispatchState;
  pushToast: PushToast;
  activeTerminologyGroupId: string;
  getReviewFlushJson?: (taskId: string) => string | undefined;
};

export function useQueueWorkflow({
  queue,
  dispatch,
  pushToast,
  activeTerminologyGroupId,
  getReviewFlushJson,
}: UseQueueWorkflowArgs) {
  const { t } = useTranslation(["tasks", "toasts"]);
  const queueRef = useRef(queue);

  useEffect(() => {
    queueRef.current = queue;
  }, [queue]);

  const isTaskPresent = useCallback(
    (taskId: string) => queueRef.current.some((item) => item.id === taskId),
    [],
  );

  const { appendPaths, pickFiles } = useQueueInput({
    dispatch,
    pushToast,
    activeTerminologyGroupId,
  });

  const { runQueuedByTaskIds } = useQueueRunner({
    dispatch,
    pushToast,
    isTaskPresent,
  });

  const {
    queueCount,
    queueBusy,
    processQueue: processQueueFromScheduler,
    processSingle: processSingleFromScheduler,
    processSingleTranscribeTranslate:
      processSingleTranscribeTranslateFromScheduler,
    clearQueue: clearQueueFromScheduler,
    removeItem: removeItemFromScheduler,
  } = useQueueScheduler({
    queue,
    dispatch,
    pushToast,
    runQueuedByTaskIds,
    getReviewFlushJson,
  });

  const {
    downloadYoutube,
    processSingle: processSingleFromYoutube,
    processSingleTranscribeTranslate:
      processSingleTranscribeTranslateFromYoutube,
    removeItem: removeItemFromYoutube,
    prepareClearYoutubeQueue,
  } = useYoutubeDownloadWorkflow({
    queue,
    dispatch,
    pushToast,
    isTaskPresent,
    processSingleFromScheduler,
    processSingleTranscribeTranslateFromScheduler,
  });

  const { ytDlpVersion, ytDlpUpdating, updateYtDlpBinary } = useYtDlpManager({
    pushToast,
  });

  const processSingle = useCallback(
    async (item: QueueItem) => {
      const handledByYoutube = await processSingleFromYoutube(item);
      if (handledByYoutube) return;
      await processSingleFromScheduler(item);
    },
    [processSingleFromScheduler, processSingleFromYoutube],
  );

  const processQueue = useCallback(
    async (mode: QueueBatchMode = "transcribe") => {
      await processQueueFromScheduler(mode);
    },
    [processQueueFromScheduler],
  );

  const processSingleTranscribeTranslate = useCallback(
    async (item: QueueItem) => {
      const handledByYoutube =
        await processSingleTranscribeTranslateFromYoutube(item);
      if (handledByYoutube) return;
      await processSingleTranscribeTranslateFromScheduler(item);
    },
    [
      processSingleTranscribeTranslateFromScheduler,
      processSingleTranscribeTranslateFromYoutube,
    ],
  );

  const removeItem = useCallback(
    async (id: string) => {
      const handledByYoutube = await removeItemFromYoutube(id);
      if (handledByYoutube) return;
      await removeItemFromScheduler(id);
    },
    [removeItemFromScheduler, removeItemFromYoutube],
  );

  const updateTaskLanguagesForItem = useCallback(
    async (
      item: QueueItem,
      sourceLang: LanguageTag,
      targetLang: TargetLanguage,
    ) => {
      if (
        item.transcribeStatus === "processing" ||
        item.transcribeStatus === "queued"
      ) {
        pushToast(t("toasts:queue.languageBusyError"), "error");
        return;
      }

      const nextSourceLang = normalizeSourceLanguage(sourceLang);
      const nextTargetLang = normalizeTargetLanguage(targetLang);
      const previousSourceLang = item.sourceLang;
      const previousTargetLang = item.targetLang;
      patchQueueItem(dispatch, item.id, (current) => ({
        ...current,
        sourceLang: nextSourceLang,
        targetLang: nextTargetLang,
      }));

      try {
        await updateTaskLanguages({
          taskId: item.id,
          sourceLang: nextSourceLang,
          targetLang: nextTargetLang,
        });
      } catch (error) {
        reportError(error, "updateTaskLanguages");
        patchQueueItem(dispatch, item.id, (current) => ({
          ...current,
          sourceLang: previousSourceLang,
          targetLang: previousTargetLang,
        }));
        pushToast(toUserErrorMessage(error, t("toasts:queue.languageSaveFailed")), "error");
      }
    },
    [dispatch, pushToast],
  );

  const updateTaskTerminologyForItem = useCallback(
    async (item: QueueItem, terminologyGroupId: string) => {
      if (
        item.transcribeStatus === "processing" ||
        item.transcribeStatus === "queued"
      ) {
        pushToast(t("toasts:queue.terminologyBusyError"), "error");
        return;
      }

      const nextGroupId = typeof terminologyGroupId === "string" ? terminologyGroupId : "";
      const previousGroupId = item.terminologyGroupId ?? "";
      patchQueueItem(dispatch, item.id, (current) => ({
        ...current,
        terminologyGroupId: nextGroupId,
      }));

      try {
        await updateTaskTerminology({
          taskId: item.id,
          terminologyGroupId: nextGroupId,
        });
      } catch (error) {
        reportError(error, "updateTaskTerminology");
        patchQueueItem(dispatch, item.id, (current) => ({
          ...current,
          terminologyGroupId: previousGroupId,
        }));
        pushToast(toUserErrorMessage(error, t("toasts:queue.terminologySaveFailed")), "error");
      }
    },
    [dispatch, pushToast],
  );

  const updateAllTaskLanguages = useCallback(
    async (sourceLang?: LanguageTag, targetLang?: TargetLanguage) => {
      const editableItems = queueRef.current.filter(
        (item) =>
          item.transcribeStatus !== "processing" &&
          item.transcribeStatus !== "queued",
      );
      if (editableItems.length === 0) {
        pushToast(t("toasts:queue.noEditableLanguageTasks"), "info");
        return;
      }

      const updates = editableItems
        .map((item) => {
          const nextSourceLang = sourceLang
            ? normalizeSourceLanguage(sourceLang)
            : normalizeSourceLanguage(item.sourceLang);
          const nextTargetLang = targetLang
            ? normalizeTargetLanguage(targetLang)
            : normalizeTargetLanguage(item.targetLang);
          return {
            item,
            nextSourceLang,
            nextTargetLang,
            previousSourceLang: item.sourceLang,
            previousTargetLang: item.targetLang,
          };
        })
        .filter(
          (entry) =>
            entry.previousSourceLang !== entry.nextSourceLang ||
            entry.previousTargetLang !== entry.nextTargetLang,
        );

      if (updates.length === 0) return;

      for (const update of updates) {
        patchQueueItem(dispatch, update.item.id, (current) => ({
          ...current,
          sourceLang: update.nextSourceLang,
          targetLang: update.nextTargetLang,
        }));
      }

      let failedCount = 0;
      // Run updates concurrently to avoid N sequential round-trips.
      // allSettled so a single failure doesn't cancel the others; each
      // failed update rolls back its own queue item optimistically.
      const results = await Promise.allSettled(
        updates.map((update) =>
          updateTaskLanguages({
            taskId: update.item.id,
            sourceLang: update.nextSourceLang,
            targetLang: update.nextTargetLang,
          }),
        ),
      );
      results.forEach((result, index) => {
        if (result.status === "rejected") {
          failedCount += 1;
          const update = updates[index];
          reportError(result.reason, "updateTaskLanguagesBatch");
          patchQueueItem(dispatch, update.item.id, (current) => ({
            ...current,
            sourceLang: update.previousSourceLang,
            targetLang: update.previousTargetLang,
          }));
        }
      });

      if (failedCount > 0) {
        pushToast(t("toasts:queue.languageSaveFailedCount", { count: failedCount }), "error");
      }
    },
    [dispatch, pushToast],
  );

  const clearQueue = useCallback(async () => {
    const youtubeClear = prepareClearYoutubeQueue();
    const cleared = await clearQueueFromScheduler();
    if (!cleared) {
      await youtubeClear.rollback();
      return;
    }
    await youtubeClear.commit();
  }, [clearQueueFromScheduler, prepareClearYoutubeQueue]);

  const downloadYoutubeAndResetInput = useCallback(
    async (url: string) => {
      const trimmed = url.trim();
      if (!trimmed) {
        await downloadYoutube(url);
        return;
      }
      await downloadYoutube(trimmed);
      dispatch({ type: "set_ui", payload: { youtubeUrl: "" } });
    },
    [dispatch, downloadYoutube],
  );

  return {
    queueCount,
    queueBusy,
    appendPaths,
    pickFiles,
    processQueue,
    processSingle,
    processSingleTranscribeTranslate,
    updateTaskLanguages: updateTaskLanguagesForItem,
    updateTaskTerminology: updateTaskTerminologyForItem,
    updateAllTaskLanguages,
    clearQueue,
    removeItem,
    downloadYoutube: downloadYoutubeAndResetInput,
    ytDlpVersion,
    ytDlpUpdating,
    updateYtDlpBinary,
  };
}
