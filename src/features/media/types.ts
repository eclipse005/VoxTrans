export type Provider = "cpu" | "cuda";

export type SavedSettings = {
  provider: Provider;
  chunkTargetSeconds: number;
};

export type QueueStatus = "pending" | "processing" | "done" | "error";

export type QueueItem = {
  id: string;
  path: string;
  name: string;
  mediaKind: "audio" | "video";
  sizeBytes: number;
  status: QueueStatus;
  progress: number;
  resultText: string;
  resultSrt: string;
  rtfx: number | null;
  error: string;
};

export type TranscribeResponse = {
  text: string;
  srt: string;
  audioDurationSec: number;
  transcribeElapsedSec: number;
  rtfx: number;
  executionProvider: string;
  ortRuntime: string;
};
