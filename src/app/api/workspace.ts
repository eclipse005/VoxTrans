import { invoke } from "@tauri-apps/api/core";
import type { QueueItem, WorkspaceStateResponse } from "../../features/media/types";

type DeleteTaskSummariesRequest = {
  taskId: string | null;
  mediaPath: string | null;
};

type SaveQueueStateRequest = {
  queue: QueueItem[];
};

type ExecuteTaskRunRequest = {
  taskId: string;
  intent?: "TRANSCRIBE" | "TRANSCRIBE_TRANSLATE" | "TRANSLATE_ONLY";
};

type ExecuteTaskBatchRequest = {
  items: ExecuteTaskRunRequest[];
};

type ExecuteTaskBatchResponse = {
  succeededTaskIds: string[];
  failed: Array<{
    taskId: string;
    error: string;
  }>;
};

export async function loadWorkspaceState(): Promise<WorkspaceStateResponse> {
  return invoke<WorkspaceStateResponse>("load_workspace_state");
}

export async function saveQueueState(request: SaveQueueStateRequest): Promise<void> {
  await invoke("save_queue_state", { request });
}

export async function deleteTaskSummaries(request: DeleteTaskSummariesRequest): Promise<void> {
  await invoke("delete_task_summaries", { request });
}

export async function executeTaskRun(request: ExecuteTaskRunRequest): Promise<void> {
  await invoke("execute_task_run", { request });
}

export async function executeTaskBatch(
  request: ExecuteTaskBatchRequest,
): Promise<ExecuteTaskBatchResponse> {
  return invoke<ExecuteTaskBatchResponse>("execute_task_batch", { request });
}
