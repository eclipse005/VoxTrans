export type Provider = "cpu" | "cuda";

export type SavedSettings = {
  provider: Provider;
  chunkTargetSeconds: number;
  subtitleMaxWordsPerSegment: number;
};

export type QueueStatus = "pending" | "queued" | "processing" | "done" | "error";
export type TranscribeStatus = QueueStatus;
export type TranscribePhase = "initializing" | "recognizing" | "segment";

export type SubtitleCue = {
  id: string;
  startMs: number;
  endMs: number;
  text: string;
  translatedText: string;
};

export type SubtitleSegment = {
  startMs: number;
  endMs: number;
  sourceText: string;
  translatedText: string;
};

export type QueueItem = {
  id: string;
  path: string;
  name: string;
  mediaKind: "audio" | "video";
  sizeBytes: number;
  transcribeStatus: TranscribeStatus;
  transcribeProgress: number;
  transcribeSegmentCurrent: number;
  transcribeSegmentTotal: number;
  transcribePhase?: TranscribePhase | "";
  transcribeError: string;
  resultText: string;
  resultSrt: string;
  subtitleSegmentsJson: string;
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
  segmentDurationsSec: number[];
  audioDurationSec: number;
  transcribeElapsedSec: number;
  executionProvider: string;
};

export type BuildSegmentsRequest = {
  taskId: string;
  audioPath: string;
  words: WordToken[];
  subtitleMaxWordsPerSegment: number;
};

export type BuildSegmentsResponse = {
  text: string;
  srt: string;
  srtOutputPath: string;
  segments: SegmentWithWords[];
};

export type SubtitleLoadRequest = {
  taskId: string;
  mediaPath: string;
  fallbackSrt?: string | null;
};

export type SubtitleLoadResponse = {
  srtPath: string;
  draftPath: string;
  content: string;
  usingDraft: boolean;
  warnings: string[];
};

export type SubtitleSaveRequest = {
  taskId: string;
  mediaPath: string;
  content: string;
  autosave: boolean;
};

export type SubtitleSaveResponse = {
  srtPath: string;
  warnings: string[];
};

export type UserPreferencesResponse = {
  settings: SavedSettings;
};

export type SaveAppSettingsRequest = {
  settings: SavedSettings;
};

export type WorkspaceStateResponse = {
  queue: QueueItem[];
};

export type TaskSummary = {
  id: string;
  mediaPath: string;
  name: string;
  mediaKind: "audio" | "video";
  sizeBytes: number;
  lastStatus: string;
  lastError: string;
  outputSrtPath: string;
  outputWordsJson: string;
  transcribeStatus: TranscribeStatus;
  transcribeError: string;
  transcriptSrt: string;
  subtitleSegmentsJson: string;
  transcribedAt: number | null;
  createdAt: number;
  updatedAt: number;
};

export type ModelDownloadStateSnapshot = {
  phase: "idle" | "downloading" | "completed" | "failed" | "cancelled";
  downloadedBytes: number;
  totalBytes: number;
  speedBytesPerSec: number;
  message: string;
};

export type ModelStatusResponse = {
  modelDir: string;
  requiredFiles: string[];
  missingFiles: string[];
  ready: boolean;
  download: ModelDownloadStateSnapshot;
};
