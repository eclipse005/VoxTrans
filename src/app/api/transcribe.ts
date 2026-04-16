import { invoke } from "@tauri-apps/api/core";
import type {
  DemucsModel,
  Provider,
  TranscribeResponse,
} from "../../features/media/types";

type AppendTaskLogRequest = {
  taskId: string;
  mediaPath: string;
  channel: "main";
  message: string;
};

type TranscribeRequest = {
  taskId: string;
  audioPath: string;
  provider: Provider;
  chunkTargetSeconds: number;
};

type SeparateVocalsRequest = {
  taskId: string;
  audioPath: string;
  model: DemucsModel;
};

type SeparateVocalsResponse = {
  vocalsPath: string;
};

type SaveSrtRequest = {
  taskId?: string;
  mediaPath?: string;
  outputPath: string;
  content: string;
};

type ExportSrtRequest = {
  taskId: string;
  targetDir: string;
  taskName?: string;
  content: string;
};

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

export async function appendTaskLog(request: AppendTaskLogRequest): Promise<void> {
  await invoke("append_task_log", { request });
}

export async function getFileSize(path: string): Promise<number> {
  return invoke<number>("get_file_size", { path });
}

export async function transcribeMedia(request: TranscribeRequest): Promise<TranscribeResponse> {
  return invoke<TranscribeResponse>("transcribe", { request });
}

export async function separateVocals(request: SeparateVocalsRequest): Promise<SeparateVocalsResponse> {
  return invoke<SeparateVocalsResponse>("separate_vocals", { request });
}

export async function saveSrt(request: SaveSrtRequest): Promise<void> {
  await invoke("save_srt", { request });
}

export async function exportSrt(request: ExportSrtRequest): Promise<string> {
  return invoke<string>("export_srt", { request });
}

export async function exportTaskSrts(request: ExportTaskSrtsRequest): Promise<string[]> {
  return invoke<string[]>("export_task_srts", { request });
}
