import { invoke } from "@tauri-apps/api/core";
import type { QueueItem, WorkspaceStateResponse } from "../../features/media/types";

type DeleteTaskSummariesRequest = {
  taskId: string | null;
  mediaPath: string | null;
};

type SaveQueueStateRequest = {
  queue: QueueItem[];
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
