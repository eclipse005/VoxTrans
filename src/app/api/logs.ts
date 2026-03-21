import { invoke } from "@tauri-apps/api/core";

type TaskLogRequest = {
  taskId: string;
  mediaPath?: string;
  channel: "main" | "llm";
};

export async function readTaskLog(request: TaskLogRequest): Promise<string> {
  return invoke<string>("read_task_log", {
    request,
  });
}

export async function clearTaskLogs(request: TaskLogRequest): Promise<void> {
  await invoke("clear_task_logs", {
    request,
  });
}

export async function getTaskTotalTokens(taskId: string): Promise<number> {
  return invoke<number>("get_task_total_tokens", {
    taskId,
  });
}
