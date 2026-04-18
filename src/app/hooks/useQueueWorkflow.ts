import { useCallback, useEffect, useRef } from "react";
import { useQueueInput } from "./queue/useQueueInput";
import { useQueueRunner } from "./queue/useQueueRunner";
import { useQueueScheduler } from "./queue/useQueueScheduler";
import { useYoutubeDownloadWorkflow } from "./useYoutubeDownloadWorkflow";
import { useYtDlpManager } from "./useYtDlpManager";
import type { QueueItem, SavedSettings } from "../../features/media/types";
import type { AppAction } from "../state/appReducer";
import type { QueueBatchMode } from "./queue/useQueueScheduler";

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
    clearQueue,
    removeItem,
    downloadYoutube: downloadYoutubeAndResetInput,
    ytDlpVersion,
    ytDlpUpdating,
    updateYtDlpBinary,
  };
}
