import { invoke } from "@tauri-apps/api/core";
import type { AlignModel, AsrModel, DemucsModel, ModelTarget } from "../../features/media/types";

type OpenTaskOutputDirRequest = {
  taskId: string;
  mediaPath: string;
};

type OpenTaskLogDirRequest = {
  taskId: string;
  mediaPath?: string;
};

export async function openTaskOutputDir(request: OpenTaskOutputDirRequest): Promise<void> {
  await invoke("open_task_output_dir", { request });
}

export async function openTaskLogDir(request: OpenTaskLogDirRequest): Promise<void> {
  await invoke("open_task_log_dir", { request });
}

/**
 * Open the app-wide error log directory (`<output root>/logs`). Not scoped to a
 * task, so it works even before anything has run — this is the folder to ask a
 * user for when they report a problem.
 */
export async function openAppLogDir(): Promise<void> {
  await invoke("open_app_log_dir");
}

export async function openModelDir(target: ModelTarget, model: AsrModel | AlignModel | DemucsModel): Promise<void> {
  await invoke("open_model_dir", {
    request: { target, model },
  });
}

export async function listSystemFonts(): Promise<string[]> {
  return invoke<string[]>("list_system_fonts");
}

export async function openExternalUrl(url: string): Promise<void> {
  try {
    await invoke("open_external_url", { url });
  } catch (err) {
    console.error("failed to open external url:", url, err);
  }
}
