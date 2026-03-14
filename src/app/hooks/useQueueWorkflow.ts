import { useCallback, useEffect, useRef } from "react";
import { useQueueInput } from "./queue/useQueueInput";
import { useQueueRunner } from "./queue/useQueueRunner";
import { useQueueScheduler } from "./queue/useQueueScheduler";
import type { QueueItem, SavedSettings } from "../../features/media/types";
import type { AppAction } from "../state/appReducer";

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

  const { runTranscribe } = useQueueRunner({
    settings,
    dispatch,
    pushToast,
    isTaskPresent,
  });

  const {
    queueCount,
    queueBusy,
    processQueue,
    processSingle,
    clearQueue,
    removeItem,
  } = useQueueScheduler({
    queue,
    dispatch,
    pushToast,
    runTranscribe,
  });

  return {
    queueCount,
    queueBusy,
    appendPaths,
    pickFiles,
    processQueue,
    processSingle,
    clearQueue,
    removeItem,
  };
}
