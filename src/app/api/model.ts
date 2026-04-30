import { invoke } from "@tauri-apps/api/core";
import type {
  AlignModel,
  AsrModel,
  DemucsModel,
  ModelStatusResponse,
  ModelTarget,
} from "../../features/media/types";

type ModelTargetRequest = {
  target: ModelTarget;
  model?: AsrModel | AlignModel | DemucsModel;
};

export async function getModelStatus(
  target: ModelTarget,
  model?: AsrModel | AlignModel | DemucsModel,
): Promise<ModelStatusResponse> {
  return invoke<ModelStatusResponse>("get_model_status", {
    request: { target, model } satisfies ModelTargetRequest,
  });
}

export async function startModelDownload(
  target: ModelTarget,
  model?: AsrModel | AlignModel | DemucsModel,
): Promise<void> {
  await invoke("start_model_download", {
    request: { target, model } satisfies ModelTargetRequest,
  });
}

export async function cancelModelDownload(
  target: ModelTarget,
  model?: AsrModel | AlignModel | DemucsModel,
): Promise<void> {
  await invoke("cancel_model_download", {
    request: { target, model } satisfies ModelTargetRequest,
  });
}
