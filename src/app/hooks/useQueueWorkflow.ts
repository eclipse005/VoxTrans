import { useCallback, useEffect, useRef } from "react";
import { useQueueInput } from "./queue/useQueueInput";
import { useQueueRunner } from "./queue/useQueueRunner";
import { useQueueScheduler } from "./queue/useQueueScheduler";
import type { QueueItem, SavedSettings } from "../../features/media/types";
import type { AppAction } from "../state/appReducer";
import type { QueueRunMode } from "./queue/useQueueRunner";

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
    processSingle,
    processSingleTranscribeTranslate,
    clearQueue,
    removeItem,
  } = useQueueScheduler({
    queue,
    settings,
    dispatch,
    pushToast,
    runBatch,
    setTaskMode,
    takeTaskMode,
  });

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
  };
}
