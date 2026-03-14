import { invoke } from "@tauri-apps/api/core";

type TaskLogRequest = {
  taskId: string;
  mediaPath: string;
};

export async function readMainTaskLog(request: TaskLogRequest): Promise<string> {
  return invoke<string>("read_task_log", {
    request: {
      ...request,
      channel: "main",
    },
  });
}

export async function clearMainTaskLogs(request: TaskLogRequest): Promise<void> {
  await invoke("clear_task_logs", {
    request: {
      ...request,
      channel: "main",
    },
  });
}
