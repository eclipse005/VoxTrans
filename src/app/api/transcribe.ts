import { invoke } from "@tauri-apps/api/core";

export type ExportSrtItem =
  | "source"
  | "target"
  | "bilingualSourceFirst"
  | "bilingualTargetFirst";

type ExportTaskSrtsRequest = {
  taskId: string;
  targetDir: string;
  taskName?: string;
  items: ExportSrtItem[];
};

export async function getFileSize(path: string): Promise<number> {
  return invoke<number>("get_file_size", { path });
}

export async function exportTaskSrts(request: ExportTaskSrtsRequest): Promise<string[]> {
  return invoke<string[]>("export_task_srts", { request });
}
