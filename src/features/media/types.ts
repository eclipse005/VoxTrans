export type Provider = "cpu" | "cuda";

export type SavedSettings = {
  provider: Provider;
  chunkTargetSeconds: number;
};

export type QueueStatus = "pending" | "queued" | "processing" | "done" | "error";

export type SubtitleCue = {
  id: string;
  startMs: number;
  endMs: number;
  text: string;
};

export type QueueItem = {
  id: string;
  path: string;
  name: string;
  mediaKind: "audio" | "video";
  sizeBytes: number;
  status: QueueStatus;
  progress: number;
  segmentCurrent: number;
  segmentTotal: number;
  resultText: string;
  resultSrt: string;
  rtfx: number | null;
  error: string;
};

export type WordToken = {
  start: number;
  end: number;
  word: string;
};

export type SegmentWithWords = {
  start: number;
  end: number;
  text: string;
  words: WordToken[];
};

export type TranscribeResponse = {
  words: WordToken[];
  segmentTotal: number;
  audioDurationSec: number;
  transcribeElapsedSec: number;
  rtfx: number;
  executionProvider: string;
  ortRuntime: string;
};

export type BuildSegmentsResponse = {
  text: string;
  srt: string;
  srtOutputPath: string;
  segments: SegmentWithWords[];
};

export type SubtitleLoadResponse = {
  srtPath: string;
  draftPath: string;
  content: string;
  usingDraft: boolean;
  warnings: string[];
};

export type SubtitleSaveResponse = {
  srtPath: string;
  warnings: string[];
};
