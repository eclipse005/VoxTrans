import { invoke } from "@tauri-apps/api/core";
import type { WorkspaceStateResponse } from "../../features/media/types";

type DeleteTaskSummariesRequest = {
  taskId: string | null;
  mediaPath: string | null;
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

type EnqueueTaskRunRequest = {
  id: string;
  mediaPath: string;
  name: string;
  mediaKind: "audio" | "video";
  sizeBytes: number;
  intent: "TRANSCRIBE" | "TRANSCRIBE_TRANSLATE" | "TRANSLATE_ONLY";
  sourceLang?: string;
  targetLang?: string;
  maxRetries?: number;
  settingsSnapshot?: Record<string, unknown>;
};

type RegisterTaskUploadRequest = {
  id: string;
  mediaPath: string;
  name: string;
  mediaKind: "audio" | "video";
  sizeBytes: number;
};

type EnqueueAndExecuteTaskBatchRequest = {
  items: EnqueueTaskRunRequest[];
};

type EvaluateTaskRequest = {
  taskId: string;
};

export type EvaluateTaskResponse = {
  id: number;
  taskId: string;
  overallScore: number;
  summary: string;
  metricsJson: string;
  outputPath: string;
  createdAt: number;
};

export async function loadWorkspaceState(): Promise<WorkspaceStateResponse> {
  return invoke<WorkspaceStateResponse>("load_workspace_state");
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

export async function enqueueTaskRun(request: EnqueueTaskRunRequest): Promise<void> {
  await invoke("enqueue_task_run", { request });
}

export async function registerTaskUpload(request: RegisterTaskUploadRequest): Promise<void> {
  await invoke("register_task_upload", { request });
}

export async function enqueueAndExecuteTaskBatch(
  request: EnqueueAndExecuteTaskBatchRequest,
): Promise<ExecuteTaskBatchResponse> {
  return invoke<ExecuteTaskBatchResponse>("enqueue_and_execute_task_batch", { request });
}

export async function evaluateTask(request: EvaluateTaskRequest): Promise<EvaluateTaskResponse> {
  return invoke<EvaluateTaskResponse>("evaluate_task", { request });
}
