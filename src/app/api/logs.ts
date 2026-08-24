import { invoke } from "@tauri-apps/api/core";

export const LOG_CHANNELS = ["main", "agent", "llm"] as const;
export type LogChannel = (typeof LOG_CHANNELS)[number];

/** Key suffix for `tasks:logs.channel${...}` translations (Main/Agent/Llm). */
export function channelLabel(channel: LogChannel): string {
  return channel.charAt(0).toUpperCase() + channel.slice(1);
}

type TaskLogRequest = {
  taskId: string;
  mediaPath?: string;
  channel: LogChannel;
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
