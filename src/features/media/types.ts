export type Provider = "cpu" | "cuda";

export type SavedSettings = {
  provider: Provider;
  chunkTargetSeconds: number;
  autoPunc: boolean;
};

export type QueueStatus = "pending" | "queued" | "processing" | "done" | "error";
export type TranscribeStatus = QueueStatus;
export type TranslateStatus = "idle" | "queued" | "processing" | "done" | "error";
export type TranscribePhase = "initializing" | "recognizing" | "punctuation" | "hotword";

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
  translateStatus: TranslateStatus;
  translateProgress: number;
  translateError: string;
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
  ortRuntime: string;
};

export type BuildSegmentsRequest = {
  taskId: string;
  audioPath: string;
  words: WordToken[];
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

export type LlmTestConnectionRequest = {
  apiKey: string;
  baseUrl?: string | null;
  model: string;
  timeoutSecs?: number | null;
};

export type LlmTestConnectionResponse = {
  ok: boolean;
  message: string;
  finishReason?: string | null;
  model: string;
};

export type DbTermEntry = {
  id: string;
  source: string;
  target: string;
  note: string;
};

export type DbHotwordGroup = {
  id: string;
  name: string;
  keyterms: string[];
};

export type DbHotwordCorrection = {
  enabled: boolean;
  activeGroupId: string;
  groups: DbHotwordGroup[];
};

export type DbLlmSettings = {
  apiKey: string;
  apiBase: string;
  apiModel: string;
};

export type UserPreferencesResponse = {
  settings: SavedSettings;
  llm: DbLlmSettings;
  terms: DbTermEntry[];
  hotwordCorrection: DbHotwordCorrection;
};

export type SaveAppSettingsRequest = {
  settings: SavedSettings;
  llm: DbLlmSettings;
};

export type WorkspaceStateResponse = {
  queue: QueueItem[];
};

export type TaskLanguage = {
  sourceLang: string;
  targetLang: string;
};

export type TaskPipelineStatus = {
  transcribeStatus: TranscribeStatus;
  transcribeError: string;
  transcribedAt: number | null;
  translateStatus: TranslateStatus;
  translateError: string;
  translatedAt: number | null;
};

export type TaskAssets = {
  transcriptSrt: string;
  translatedSrt: string;
  translatedSrtPath: string;
  subtitleSegmentsJson: string;
  translateModel: string;
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
  createdAt: number;
  updatedAt: number;
} & TaskLanguage & TaskPipelineStatus & TaskAssets;

export type TaskLogChannel = "main" | "llm";

