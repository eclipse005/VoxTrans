import { useCallback, useEffect, useRef } from "react";
import { evaluateTask } from "../api/workspace";
import { useQueueInput } from "./queue/useQueueInput";
import { useQueueRunner } from "./queue/useQueueRunner";
import { useQueueScheduler } from "./queue/useQueueScheduler";
import { useYoutubeDownloadWorkflow } from "./useYoutubeDownloadWorkflow";
import { useYtDlpManager } from "./useYtDlpManager";
import type { QueueItem, SavedSettings } from "../../features/media/types";
import type { AppAction } from "../state/appReducer";
import type { QueueRunMode } from "./queue/useQueueRunner";
import { toUserErrorMessage } from "../utils/errors";

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
  const taskModeRef = useRef<Map<string, QueueRunMode>>(new Map());

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

  const { runBatch } = useQueueRunner({
    dispatch,
    pushToast,
    isTaskPresent,
    settings,
  });

  const setTaskMode = useCallback((taskId: string, mode: QueueRunMode) => {
    taskModeRef.current.set(taskId, mode);
  }, []);

  const takeTaskMode = useCallback((taskId: string): QueueRunMode => {
    const mode = taskModeRef.current.get(taskId) ?? "transcribe";
    taskModeRef.current.delete(taskId);
    return mode;
  }, []);

  const {
    queueCount,
    queueBusy,
    processQueue,
    processSingle: processSingleFromScheduler,
    processSingleTranscribeTranslate: processSingleTranscribeTranslateFromScheduler,
    clearQueue: clearQueueFromScheduler,
    removeItem: removeItemFromScheduler,
  } = useQueueScheduler({
    queue,
    settings,
    dispatch,
    pushToast,
    runBatch,
    setTaskMode,
    takeTaskMode,
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

  const evaluateItem = useCallback(async (item: QueueItem) => {
    if (item.transcribeStatus !== "done") {
      pushToast("请先完成转录/转译后再评估", "info");
      return;
    }
    try {
      pushToast("评估中，正在调用 LLM...", "info");
      await evaluateTask({ taskId: item.id });
      pushToast("评估完成，结果已写入任务目录", "success");
    } catch (error) {
      pushToast(toUserErrorMessage(error, "评估失败"), "error");
    }
  }, [pushToast]);

  const processSingle = useCallback(async (item: QueueItem) => {
    const handledByYoutube = await processSingleFromYoutube(item);
    if (handledByYoutube) return;
    await processSingleFromScheduler(item);
  }, [processSingleFromScheduler, processSingleFromYoutube]);

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
    evaluateItem,
    clearQueue,
    removeItem,
    downloadYoutube: downloadYoutubeAndResetInput,
    ytDlpVersion,
    ytDlpUpdating,
    updateYtDlpBinary,
  };
}
