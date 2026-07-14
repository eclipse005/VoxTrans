import { invoke } from "@tauri-apps/api/core";
import type {
  LanguageTag,
  QueueItem,
  TargetLanguage,
  WorkspaceStateResponse,
} from "../../features/media/types";

type DeleteTasksRequest = {
  taskId: string | null;
  mediaPath: string | null;
};

type ExecuteTaskRunRequest = {
  taskId: string;
  intent?: "TRANSCRIBE" | "TRANSCRIBE_TRANSLATE" | "TRANSLATE_SRT";
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
  mediaKind: "audio" | "video" | "subtitle";
  sizeBytes: number;
  intent: "TRANSCRIBE" | "TRANSCRIBE_TRANSLATE" | "TRANSLATE_SRT";
  sourceLang?: LanguageTag;
  targetLang?: TargetLanguage;
  maxRetries?: number;
  terminologyGroupId?: string;
};

type UpdateTaskLanguagesRequest = {
  taskId: string;
  sourceLang: LanguageTag;
  targetLang: TargetLanguage;
};

type UpdateTaskTerminologyRequest = {
  taskId: string;
  terminologyGroupId: string;
};

export type RegisterTaskUploadRequest = {
  id: string;
  mediaPath: string;
  name: string;
  mediaKind: "audio" | "video" | "subtitle";
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

export async function registerTaskUpload(request: RegisterTaskUploadRequest): Promise<QueueItem> {
  return invoke<QueueItem>("register_task_upload", { request });
}

export async function updateTaskLanguages(request: UpdateTaskLanguagesRequest): Promise<void> {
  await invoke("update_task_languages", { request });
}

export async function updateTaskTerminology(request: UpdateTaskTerminologyRequest): Promise<void> {
  await invoke("update_task_terminology", { request });
}
