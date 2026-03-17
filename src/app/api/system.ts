import { invoke } from "@tauri-apps/api/core";
import type { ModelTarget } from "../../features/media/types";

type OpenTaskOutputDirRequest = {
  taskId: string;
  mediaPath: string;
};

type OpenTaskLogDirRequest = {
  taskId: string;
};

export async function openTaskOutputDir(request: OpenTaskOutputDirRequest): Promise<void> {
  await invoke("open_task_output_dir", { request });
}

export async function openTaskLogDir(request: OpenTaskLogDirRequest): Promise<void> {
  await invoke("open_task_log_dir", { request });
}

export async function openOutputDir(): Promise<void> {
  await invoke("open_output_dir");
}

export async function openModelDir(target: ModelTarget): Promise<void> {
  await invoke("open_model_dir", {
    request: { target },
  });
}
