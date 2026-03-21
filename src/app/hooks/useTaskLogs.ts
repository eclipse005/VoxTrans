import { useCallback, useEffect, useMemo, useState } from "react";
import { clearTaskLogs, getTaskTotalTokens, readTaskLog } from "../api/logs";
import { openTaskLogDir } from "../api/system";
import type { QueueItem } from "../../features/media/types";
import type { AppAction } from "../state/appReducer";
import type { ToastTone } from "../types";

type DispatchState = (action: AppAction) => void;
type PushToast = (message: string, tone?: ToastTone) => void;

type TaskLogContext = {
  taskId: string;
  taskName: string;
  mediaPath: string;
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
  const [logChannel, setLogChannel] = useState<"main" | "llm">("main");
  const [totalTokens, setTotalTokens] = useState(0);

  const loadLogs = useCallback(async () => {
    if (!logTaskContext) return;
    setLoadingLogs(true);
    try {
      const content = await readTaskLog({
        taskId: logTaskContext.taskId,
        mediaPath: logTaskContext.mediaPath,
        channel: logChannel,
      });
      setLogContent(content || "");
    } catch (error) {
      const message = error instanceof Error ? error.message : "加载日志失败";
      pushToast(message, "error");
      setLoadingLogs(false);
      return;
    }

    try {
      const tokens = await getTaskTotalTokens(logTaskContext.taskId);
      setTotalTokens(Number.isFinite(tokens) ? Math.max(0, Math.floor(tokens)) : 0);
    } catch {
      setTotalTokens(0);
    } finally {
      setLoadingLogs(false);
    }
  }, [logTaskContext, logChannel, pushToast]);

  const openLogs = useCallback(() => {
    if (!activeQueueItem) {
      pushToast("请先在左侧选中一个任务", "error");
      return;
    }
    setLogTaskContext({
      taskId: activeQueueItem.id,
      taskName: activeQueueItem.name,
      mediaPath: activeQueueItem.path,
    });
    setLogChannel("main");
    setLogContent("");
    setTotalTokens(0);
    dispatch({ type: "set_ui", payload: { showLogs: true } });
  }, [activeQueueItem, dispatch, pushToast]);

  const clearLogs = useCallback(async () => {
    if (!logTaskContext) return;
    try {
      await clearTaskLogs({
        taskId: logTaskContext.taskId,
        mediaPath: logTaskContext.mediaPath,
        channel: logChannel,
      });
      setLogContent("");
      try {
        const tokens = await getTaskTotalTokens(logTaskContext.taskId);
        setTotalTokens(Number.isFinite(tokens) ? Math.max(0, Math.floor(tokens)) : 0);
      } catch {
        // Ignore token refresh failure; keep previous visible value.
      }
      pushToast(`${logChannel.toUpperCase()} 日志已清空`, "success");
    } catch (error) {
      const message = error instanceof Error ? error.message : "清空日志失败";
      pushToast(message, "error");
    }
  }, [logChannel, logTaskContext, pushToast]);

  const openLogDir = useCallback(async () => {
    try {
      await openTaskLogDir({
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
    logChannel,
    loadingLogs,
    totalTokens,
    taskName,
    loadLogs,
    setLogChannel,
    openLogs,
    clearLogs,
    openLogDir,
  };
}
