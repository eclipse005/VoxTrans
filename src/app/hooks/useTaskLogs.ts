import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
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
  const { t } = useTranslation(["toasts", "tasks", "models"]);
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
      const message = error instanceof Error ? error.message : t("tasks:logs.loadFailed");
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
  }, [logTaskContext, logChannel, pushToast, t]);

  const openLogs = useCallback(() => {
    if (!activeQueueItem) {
      pushToast(t("tasks:logs.selectTaskFirst"), "error");
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
  }, [activeQueueItem, dispatch, pushToast, t]);

  const clearLogs = useCallback(async () => {
    if (!logTaskContext) return;
    const confirmed = window.confirm(
      t("tasks:logs.clearConfirm", { channel: logChannel.toUpperCase() }),
    );
    if (!confirmed) return;
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
      pushToast(t("tasks:logs.cleared", { channel: logChannel.toUpperCase() }), "success");
    } catch (error) {
      const message = error instanceof Error ? error.message : t("tasks:logs.clearFailed");
      pushToast(message, "error");
    }
  }, [logChannel, logTaskContext, pushToast, t]);

  const openLogDir = useCallback(async () => {
    try {
      await openTaskLogDir({
        taskId: logTaskContext?.taskId ?? "",
        mediaPath: logTaskContext?.mediaPath ?? "",
      });
    } catch (error) {
      const message = error instanceof Error ? error.message : t("tasks:logs.openDirFailed");
      pushToast(message, "error");
    }
  }, [logTaskContext, pushToast, t]);

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
