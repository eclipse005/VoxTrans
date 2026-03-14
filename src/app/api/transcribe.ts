import { invoke } from "@tauri-apps/api/core";
import type { BuildSegmentsResponse, TranscribeResponse, WordToken } from "../../features/media/types";

type AppendTaskLogRequest = {
  taskId: string;
  mediaPath: string;
  channel: "main";
  message: string;
};

type TranscribeRequest = {
  taskId: string;
  audioPath: string;
  provider: "cpu" | "cuda";
  chunkTargetSeconds: number;
};

type RunPostAsrPipelineRequest = {
  taskId: string;
  audioPath: string;
  words: WordToken[];
  subtitleMaxWordsPerSegment: number;
};

export type PostAsrPipelineResponse = {
  text: string;
  srt: string;
  srtOutputPath: string;
  segments: BuildSegmentsResponse["segments"];
  words: TranscribeResponse["words"];
};

type SaveSrtRequest = {
  outputPath: string;
  content: string;
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

export async function runPostAsrPipeline(
  request: RunPostAsrPipelineRequest,
): Promise<PostAsrPipelineResponse> {
  return invoke<PostAsrPipelineResponse>("run_post_asr_pipeline", { request });
}

export async function saveSrt(request: SaveSrtRequest): Promise<void> {
  await invoke("save_srt", { request });
}
