import { invoke } from "@tauri-apps/api/core";
import type { ModelStatusResponse } from "../../features/media/types";

export async function getModelStatus(): Promise<ModelStatusResponse> {
  return invoke<ModelStatusResponse>("get_model_status");
}

export async function startModelDownload(): Promise<void> {
  await invoke("start_model_download");
}

export async function cancelModelDownload(): Promise<void> {
  await invoke("cancel_model_download");
}
