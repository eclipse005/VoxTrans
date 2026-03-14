import { useCallback, useEffect, useMemo, useState } from "react";
import { clearMainTaskLogs, readMainTaskLog } from "../api/logs";
import { openTaskOutputDir } from "../api/system";
import type { QueueItem } from "../../features/media/types";
import type { AppAction } from "../state/appReducer";
import type { ToastTone } from "../types";

type DispatchState = (action: AppAction) => void;
type PushToast = (message: string, tone?: ToastTone) => void;

type TaskLogContext = {
  taskId: string;
  mediaPath: string;
  taskName: string;
};

type UseTaskLogsArgs = {
  showLogs: boolean;
  activeQueueItem: QueueItem | null;
  dispatch: DispatchState;
  pushToast: PushToast;
};

export function useTaskLogs({
  showLogs,
  activeQueueItem,
  dispatch,
  pushToast,
}: UseTaskLogsArgs) {
  const [logTaskContext, setLogTaskContext] = useState<TaskLogContext | null>(null);
  const [logContent, setLogContent] = useState("");
  const [loadingLogs, setLoadingLogs] = useState(false);

  const loadLogs = useCallback(async () => {
    if (!logTaskContext) return;
    setLoadingLogs(true);
    try {
      const content = await readMainTaskLog({
        taskId: logTaskContext.taskId,
        mediaPath: logTaskContext.mediaPath,
      });
      setLogContent(content || "");
    } catch (error) {
      const message = error instanceof Error ? error.message : "加载日志失败";
      pushToast(message, "error");
    } finally {
      setLoadingLogs(false);
    }
  }, [logTaskContext, pushToast]);

  const openLogs = useCallback(() => {
    if (!activeQueueItem) {
      pushToast("请先在左侧选中一个任务", "error");
      return;
    }
    setLogTaskContext({
      taskId: activeQueueItem.id,
      mediaPath: activeQueueItem.path,
      taskName: activeQueueItem.name,
    });
    setLogContent("");
    dispatch({ type: "set_ui", payload: { showLogs: true } });
  }, [activeQueueItem, dispatch, pushToast]);

  const clearLogs = useCallback(async () => {
    if (!logTaskContext) return;
    try {
      await clearMainTaskLogs({
        taskId: logTaskContext.taskId,
        mediaPath: logTaskContext.mediaPath,
      });
      setLogContent("");
      pushToast("日志已清空", "success");
    } catch (error) {
      const message = error instanceof Error ? error.message : "清空日志失败";
      pushToast(message, "error");
    }
  }, [logTaskContext, pushToast]);

  const openLogDir = useCallback(async () => {
    try {
      await openTaskOutputDir({
        taskId: logTaskContext?.taskId ?? "",
        mediaPath: logTaskContext?.mediaPath ?? "",
      });
    } catch (error) {
      const message = error instanceof Error ? error.message : "打开日志目录失败";
      pushToast(message, "error");
    }
  }, [logTaskContext, pushToast]);

  useEffect(() => {
    if (!showLogs || !logTaskContext) return;
    void loadLogs();
  }, [showLogs, logTaskContext, loadLogs]);

  const taskName = useMemo(() => logTaskContext?.taskName || "", [logTaskContext]);

  return {
    logTaskContext,
    logContent,
    loadingLogs,
    taskName,
    loadLogs,
    openLogs,
    clearLogs,
    openLogDir,
  };
}
