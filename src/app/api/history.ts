import { invoke } from "@tauri-apps/api/core";
import type { TaskSummary } from "../../features/media/types";

type ListTaskSummariesRequest = {
  limit?: number;
};

export async function listTaskSummaries(
  request: ListTaskSummariesRequest,
): Promise<TaskSummary[]> {
  return invoke<TaskSummary[]>("list_task_summaries", { request });
}
