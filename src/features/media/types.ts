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

export type SubtitleBurnMode =
  | "source"
  | "target"
  | "bilingualSourceFirst"
  | "bilingualTargetFirst";

export type SubtitleLineStyle = {
  fontFamily: string;
  fontSize: number;
  primaryColor: string;
  outlineColor: string;
  backColor: string;
  outline: number;
  shadow: number;
  borderStyle: "outline" | "box";
  borderOpacity: number;
};

export type SubtitleLayoutStyle = {
  marginV: number;
  alignment: 1 | 2 | 3;
  bilingualLineGap: number;
};

export type SubtitleRenderStyle = {
  source: SubtitleLineStyle;
  target: SubtitleLineStyle;
  layout: SubtitleLayoutStyle;
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
  enableSubtitleBeautify: boolean;
  autoBurnHardSubtitle: boolean;
  subtitleBurnMode: SubtitleBurnMode;
  subtitleRenderStyle: SubtitleRenderStyle;
};

export type QueueStatus = "pending" | "queued" | "processing" | "done" | "error";
export type TranscribeStatus = QueueStatus;
export type TranscribePhase =
  | "downloading"
  | "initializing"
  | "separating"
  | "recognizing"
  | "punctuate"
  | "segment"
  | "translate";

export type SubtitleCue = {
  id: string;
  startMs: number;
  endMs: number;
  text: string;
  translatedText: string;
};

export type SubtitleWordAnchor = {
  startMs: number;
  endMs: number;
  word: string;
};

export type SubtitleSegment = {
  startMs: number;
  endMs: number;
  sourceText: string;
  translatedText: string;
  sourceWords: SubtitleWordAnchor[];
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
  segmentMode?: "transcribe" | "translate_source";
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
  subtitleSegmentsJson?: string;
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

export type TranslateAgentRuntimeEvent = {
  kind: string;
  toolName: string;
  parentToolName: string;
  phase: string;
  reason: string;
  detail: string;
  toolInputSummary: string;
  changedSegmentTotal: number;
};

export type TranslateAgentToolExecution = {
  name: string;
  status: string;
  changedSegmentTotal: number;
};

export type TranslateAgentToolPlanStep = {
  name: string;
  toolInputSummary: string;
};
