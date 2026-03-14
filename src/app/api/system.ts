import { invoke } from "@tauri-apps/api/core";

type OpenTaskOutputDirRequest = {
  taskId: string;
  mediaPath: string;
};

export async function openTaskOutputDir(request: OpenTaskOutputDirRequest): Promise<void> {
  await invoke("open_task_output_dir", { request });
}

export async function openOutputDir(): Promise<void> {
  await invoke("open_output_dir");
}

export async function openModelDir(): Promise<void> {
  await invoke("open_model_dir");
}
