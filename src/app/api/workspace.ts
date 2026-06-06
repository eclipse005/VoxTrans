import { invoke } from "@tauri-apps/api/core";
import type {
  QueueItem,
  SourceLanguage,
  TargetLanguage,
  WorkspaceStateResponse,
} from "../../features/media/types";

type DeleteTasksRequest = {
  taskId: string | null;
  mediaPath: string | null;
};

type ExecuteTaskRunRequest = {
  taskId: string;
  intent?: "TRANSCRIBE" | "TRANSCRIBE_TRANSLATE";
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

type WorkspaceTaskResponse = {
  item: QueueItem;
};

type EnqueueTaskRunRequest = {
  id: string;
  mediaPath: string;
  name: string;
  mediaKind: "audio" | "video";
  sizeBytes: number;
  intent: "TRANSCRIBE" | "TRANSCRIBE_TRANSLATE";
  sourceLang?: SourceLanguage;
  targetLang?: TargetLanguage;
  maxRetries?: number;
};

type UpdateTaskLanguagesRequest = {
  taskId: string;
  sourceLang: SourceLanguage;
  targetLang: TargetLanguage;
};

export type RegisterTaskUploadRequest = {
  id: string;
  mediaPath: string;
  name: string;
  mediaKind: "audio" | "video";
  sizeBytes: number;
};

export async function loadWorkspaceState(): Promise<WorkspaceStateResponse> {
  return invoke<WorkspaceStateResponse>("load_workspace_state");
}

export async function getTaskRunQueueItem(taskId: string): Promise<Partial<QueueItem>> {
  const detail = await invoke<WorkspaceTaskResponse>("load_workspace_task", {
    request: { taskId },
  });
  return detail.item;
}

export async function deleteTasks(request: DeleteTasksRequest): Promise<void> {
  await invoke("delete_tasks", { request });
}

export async function executeTaskBatch(
  request: ExecuteTaskBatchRequest,
): Promise<ExecuteTaskBatchResponse> {
  return invoke<ExecuteTaskBatchResponse>("execute_task_batch", { request });
}

export async function enqueueTaskRun(request: EnqueueTaskRunRequest): Promise<void> {
  await invoke("enqueue_task_run", { request });
}

export async function registerTaskUpload(request: RegisterTaskUploadRequest): Promise<void> {
  await invoke("register_task_upload", { request });
}

export async function updateTaskLanguages(request: UpdateTaskLanguagesRequest): Promise<void> {
  await invoke("update_task_languages", { request });
}
