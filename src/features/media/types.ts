export const PROVIDER_IDS = ["cpu", "directml"] as const;
export type Provider = (typeof PROVIDER_IDS)[number];
export type ModelTarget = "asr" | "demucs";
export type AsrModel = "parakeet-tdt-0.6b-v2";
export type DemucsModel = "htdemucs_ft";

export type TerminologyTerm = {
  id: string;
  origin: string;
  target: string;
  note: string;
};

export type TerminologyGroup = {
  id: string;
  name: string;
  terms: TerminologyTerm[];
};

export type SavedSettings = {
  provider: Provider;
  chunkTargetSeconds: number;
  subtitleMaxWordsPerSegment: number;
  subtitleLengthReference: number;
  asrModel: AsrModel;
  demucsModel: DemucsModel;
  enableVocalSeparation: boolean;
  translateApiKey: string;
  translateBaseUrl: string;
  translateModel: string;
  llmConcurrency: number;
  terminologyGroups: TerminologyGroup[];
  enableTerminology: boolean;
  enablePunctuationOptimization: boolean;
  enableAsrCorrection: boolean;
  enableSubtitleBeautify: boolean;
};

export type QueueStatus = "pending" | "queued" | "processing" | "done" | "error";
export type TranscribeStatus = QueueStatus;
export type TranscribePhase =
  | "downloading"
  | "initializing"
  | "separating"
  | "recognizing"
  | "punctuate"
  | "correct"
  | "segment"
  | "summarize"
  | "translate"
  | "qa";

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
  transcribePhaseDetail: string;
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

export type TranslateToken = {
  start: number;
  end: number;
  word: string;
};

export type TranslateTerminologyEntry = {
  source: string;
  target: string;
  note: string;
  group: string;
};

export type TranslateSegment = {
  startMs: number;
  endMs: number;
  sourceText: string;
  translatedText: string;
};

export type TranslatePipelineRequest = {
  taskId: string;
  mediaPath: string;
  sourceLang: string;
  targetLang: string;
  tokens: TranslateToken[];
  translateApiKey?: string;
  translateBaseUrl?: string;
  translateModel?: string;
  llmConcurrency?: number;
  terminologyEntries?: TranslateTerminologyEntry[];
};

export type TranslatePipelineResponse = {
  sourceSrt: string;
  targetSrt: string;
  bilingualSrtSourceFirst: string;
  bilingualSrtTargetFirst: string;
  segments: TranslateSegment[];
  styleTopicSummary?: string;
  styleToneStrategy?: string;
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
  vadElapsedSec: number;
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

export type SubtitleSaveRequest = {
  taskId: string;
  content: string;
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

export type ModelDownloadStateSnapshot = {
  phase: "idle" | "downloading" | "completed" | "failed" | "cancelled";
  downloadedBytes: number;
  totalBytes: number;
  speedBytesPerSec: number;
  message: string;
};

export type ModelStatusResponse = {
  target: ModelTarget;
  model: string;
  modelDir: string;
  requiredFiles: string[];
  missingFiles: string[];
  ready: boolean;
  download: ModelDownloadStateSnapshot;
};
