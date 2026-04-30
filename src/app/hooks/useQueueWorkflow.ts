import { useCallback, useEffect, useRef } from "react";
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
  QueueItem,
  SavedSettings,
  SourceLanguage,
  TargetLanguage,
} from "../../features/media/types";
import type { AppAction } from "../state/appReducer";
import type { QueueBatchMode } from "./queue/useQueueScheduler";
import { updateTaskLanguages } from "../api/workspace";
import { patchQueueItem } from "../state/queueDomainActions";
import { reportError, toUserErrorMessage } from "../utils/errors";

type DispatchState = (action: AppAction) => void;
type PushToast = (message: string, tone?: "info" | "success" | "error") => void;

type UseQueueWorkflowArgs = {
  queue: QueueItem[];
  settings: SavedSettings;
  dispatch: DispatchState;
  pushToast: PushToast;
};

export function useQueueWorkflow({
  queue,
  settings,
  dispatch,
  pushToast,
}: UseQueueWorkflowArgs) {
  const queueRef = useRef(queue);

  useEffect(() => {
    queueRef.current = queue;
  }, [queue]);

  const isTaskPresent = useCallback((taskId: string) => (
    queueRef.current.some((item) => item.id === taskId)
  ), []);

  const { appendPaths, pickFiles } = useQueueInput({
    dispatch,
    pushToast,
  });

  const { runQueuedByTaskIds } = useQueueRunner({
    dispatch,
    pushToast,
    isTaskPresent,
    settings,
  });

  const {
    queueCount,
    queueBusy,
    processQueue: processQueueFromScheduler,
    processSingle: processSingleFromScheduler,
    processSingleTranscribeTranslate: processSingleTranscribeTranslateFromScheduler,
    clearQueue: clearQueueFromScheduler,
    removeItem: removeItemFromScheduler,
  } = useQueueScheduler({
    queue,
    settings,
    dispatch,
    pushToast,
    runQueuedByTaskIds,
  });

  const {
    downloadYoutube,
    processSingle: processSingleFromYoutube,
    processSingleTranscribeTranslate: processSingleTranscribeTranslateFromYoutube,
    removeItem: removeItemFromYoutube,
    clearYoutubeQueue,
  } = useYoutubeDownloadWorkflow({
    queue,
    dispatch,
    pushToast,
    isTaskPresent,
    processSingleFromScheduler,
    processSingleTranscribeTranslateFromScheduler,
  });

  const {
    ytDlpVersion,
    ytDlpUpdating,
    updateYtDlpBinary,
  } = useYtDlpManager({ pushToast });

  const processSingle = useCallback(async (item: QueueItem) => {
    const handledByYoutube = await processSingleFromYoutube(item);
    if (handledByYoutube) return;
    await processSingleFromScheduler(item);
  }, [processSingleFromScheduler, processSingleFromYoutube]);

  const processQueue = useCallback(async (mode: QueueBatchMode = "transcribe") => {
    await processQueueFromScheduler(mode);
  }, [processQueueFromScheduler]);

  const processSingleTranscribeTranslate = useCallback(async (item: QueueItem) => {
    const handledByYoutube = await processSingleTranscribeTranslateFromYoutube(item);
    if (handledByYoutube) return;
    await processSingleTranscribeTranslateFromScheduler(item);
  }, [processSingleTranscribeTranslateFromScheduler, processSingleTranscribeTranslateFromYoutube]);

  const removeItem = useCallback(async (id: string) => {
    const handledByYoutube = await removeItemFromYoutube(id);
    if (handledByYoutube) return;
    await removeItemFromScheduler(id);
  }, [removeItemFromScheduler, removeItemFromYoutube]);

  const updateTaskLanguagesForItem = useCallback(async (
    item: QueueItem,
    sourceLang: SourceLanguage,
    targetLang: TargetLanguage,
  ) => {
    if (item.transcribeStatus === "processing" || item.transcribeStatus === "queued") {
      pushToast("任务正在处理或排队，不能修改语言", "error");
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
      pushToast(toUserErrorMessage(error, "语言设置保存失败"), "error");
    }
  }, [dispatch, pushToast]);

  const updateAllTaskLanguages = useCallback(async (
    sourceLang?: SourceLanguage,
    targetLang?: TargetLanguage,
  ) => {
    const editableItems = queueRef.current.filter((item) => (
      item.transcribeStatus !== "processing" && item.transcribeStatus !== "queued"
    ));
    if (editableItems.length === 0) {
      pushToast("没有可修改语言的任务", "info");
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
      .filter((entry) => (
        entry.previousSourceLang !== entry.nextSourceLang
        || entry.previousTargetLang !== entry.nextTargetLang
      ));

    if (updates.length === 0) return;

    for (const update of updates) {
      patchQueueItem(dispatch, update.item.id, (current) => ({
        ...current,
        sourceLang: update.nextSourceLang,
        targetLang: update.nextTargetLang,
      }));
    }

    let failedCount = 0;
    for (const update of updates) {
      try {
        await updateTaskLanguages({
          taskId: update.item.id,
          sourceLang: update.nextSourceLang,
          targetLang: update.nextTargetLang,
        });
      } catch (error) {
        failedCount += 1;
        reportError(error, "updateTaskLanguagesBatch");
        patchQueueItem(dispatch, update.item.id, (current) => ({
          ...current,
          sourceLang: update.previousSourceLang,
          targetLang: update.previousTargetLang,
        }));
      }
    }

    if (failedCount > 0) {
      pushToast(`有 ${failedCount} 个任务语言保存失败`, "error");
    }
  }, [dispatch, pushToast]);

  const clearQueue = useCallback(async () => {
    await clearYoutubeQueue();
    await clearQueueFromScheduler();
  }, [clearQueueFromScheduler, clearYoutubeQueue]);

  const downloadYoutubeAndResetInput = useCallback(async (url: string) => {
    const trimmed = url.trim();
    if (!trimmed) {
      await downloadYoutube(url);
      return;
    }
    await downloadYoutube(trimmed);
    dispatch({ type: "set_ui", payload: { youtubeUrl: "" } });
  }, [dispatch, downloadYoutube]);

  return {
    queueCount,
    queueBusy,
    appendPaths,
    pickFiles,
    processQueue,
    processSingle,
    processSingleTranscribeTranslate,
    updateTaskLanguages: updateTaskLanguagesForItem,
    updateAllTaskLanguages,
    clearQueue,
    removeItem,
    downloadYoutube: downloadYoutubeAndResetInput,
    ytDlpVersion,
    ytDlpUpdating,
    updateYtDlpBinary,
  };
}
