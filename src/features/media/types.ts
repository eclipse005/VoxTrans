export const PROVIDER_IDS = ["cpu", "cuda"] as const;
export type Provider = (typeof PROVIDER_IDS)[number];
export type ModelTarget = "asr" | "align" | "demucs";
export type AsrModel = "Qwen3-ASR-0.6B" | "Qwen3-ASR-1.7B" | "cohere-transcribe-03-2026";
export type AlignModel = "Qwen3-ForcedAligner-0.6B";
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

type SubtitleLayoutStyle = {
  marginV: number;
  alignment: 1 | 2 | 3;
  bilingualLineGap: number;
};

export type SubtitleRenderStyle = {
  source: SubtitleLineStyle;
  target: SubtitleLineStyle;
  layout: SubtitleLayoutStyle;
};

export type SubtitleLengthPreset = "short" | "standard" | "loose";

export type SavedSettings = {
  provider: Provider;
  chunkTargetSeconds: number;
  subtitleLengthPreset: SubtitleLengthPreset;
  asrModel: AsrModel;
  alignModel: AlignModel;
  demucsModel: DemucsModel;
  enableVocalSeparation: boolean;
  translateApiKey: string;
  translateBaseUrl: string;
  translateModel: string;
  llmConcurrency: number;
  terminologyGroups: TerminologyGroup[];
  activeTerminologyGroupId: string;
  enableSubtitleBeautify: boolean;
  enableClickSound: boolean;
  autoBurnHardSubtitle: boolean;
  subtitleBurnMode: SubtitleBurnMode;
  subtitleRenderStyle: SubtitleRenderStyle;
  flatSrtOutput: boolean;
  flatSrtItems: SubtitleBurnMode[];
};

export type QueueStatus = "pending" | "queued" | "processing" | "done" | "error";
export type TranscribeStatus = QueueStatus;
export type SourceLanguage = "en" | "zh" | "yue" | "ja" | "ko" | "fr" | "de" | "it" | "es" | "pt";
export type TargetLanguage =
  | "zh-CN"
  | "zh-TW"
  | "en"
  | "ja"
  | "ko"
  | "fr"
  | "de"
  | "es"
  | "it"
  | "pt"
  | "ru"
  | "ar"
  | "vi"
  | "th"
  | "id"
  | "tr"
  | "nl"
  | "pl";
export type TaskStageCode =
  | "downloading"
  | "preparing"
  | "separating"
  | "recognizing"
  | "aligning"
  | "segmenting"
  | "summarizing"
  | "terminology"
  | "translating"
  | "subtitleLayout"
  | "finalCheck";

export type TaskStageProgress = {
  code: TaskStageCode | "";
  label: string;
  order: number;
  detail: string;
  current: number;
  total: number;
};

export type TaskProgress = {
  stage: TaskStageProgress;
};

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
  sourceLang: SourceLanguage;
  targetLang: TargetLanguage;
  transcribeStatus: TranscribeStatus;
  taskProgress: TaskProgress;
  transcribeError: string;
  resultText: string;
  resultSrt: string;
  subtitleSegmentsJson: string;
  // Per-task selected terminology group ("" = none). Optional in the type
  // because the backend sends it with #[serde(default)]; runtime always
  // normalizes it (useWorkspacePersistence / useQueueInput). UI guards `?? ""`.
  terminologyGroupId?: string;
};

const TASK_STAGE_SET = new Set<TaskStageCode>([
  "downloading",
  "preparing",
  "separating",
  "recognizing",
  "aligning",
  "segmenting",
  "summarizing",
  "terminology",
  "translating",
  "subtitleLayout",
  "finalCheck",
]);

const TASK_STAGE_ORDER: Record<TaskStageCode, number> = {
  downloading: 10,
  preparing: 20,
  separating: 25,
  recognizing: 30,
  aligning: 35,
  segmenting: 40,
  summarizing: 50,
  terminology: 60,
  translating: 70,
  subtitleLayout: 80,
  finalCheck: 90,
};

export function normalizeTaskStageCode(value: unknown): TaskStageCode | "" {
  if (typeof value !== "string") return "";
  if (TASK_STAGE_SET.has(value as TaskStageCode)) {
    return value as TaskStageCode;
  }
  return "";
}

function clampUnsigned(value: unknown): number {
  const n = Number(value);
  if (!Number.isFinite(n)) return 0;
  return Math.max(0, Math.round(n));
}

export function createEmptyTaskProgress(): TaskProgress {
  return {
    stage: {
      code: "",
      label: "",
      order: 0,
      detail: "",
      current: 0,
      total: 0,
    },
  };
}

export function createTaskProgress(input: {
  code?: TaskStageCode | "";
  label?: string;
  order?: number;
  detail?: string;
  current?: number;
  total?: number;
}): TaskProgress {
  const code = normalizeTaskStageCode(input.code);
  const order = clampUnsigned(input.order);
  return {
    stage: {
      code,
      label: typeof input.label === "string" ? input.label : "",
      order: order > 0 ? order : (code ? TASK_STAGE_ORDER[code] : 0),
      detail: typeof input.detail === "string" ? input.detail : "",
      current: clampUnsigned(input.current),
      total: clampUnsigned(input.total),
    },
  };
}

export function normalizeTaskProgress(value: unknown): TaskProgress {
  if (typeof value !== "object" || value === null) {
    return createEmptyTaskProgress();
  }
  const payload = value as Partial<TaskProgress>;
  const stage =
    typeof payload.stage === "object" && payload.stage !== null
      ? payload.stage as Partial<TaskStageProgress>
      : {};
  return createTaskProgress({
    code: normalizeTaskStageCode(stage.code),
    label: typeof stage.label === "string" ? stage.label : "",
    order: Number(stage.order ?? 0),
    detail: typeof stage.detail === "string" ? stage.detail : "",
    current: Number(stage.current ?? 0),
    total: Number(stage.total ?? 0),
  });
}

export type WordToken = {
  start: number;
  end: number;
  word: string;
};

export type TranscribeResponse = {
  words: WordToken[];
  text: string;
  alignedText: string;
  segmentTotal: number;
  segmentDurationsSec: number[];
  audioDurationSec: number;
  vadElapsedSec: number;
  transcribeElapsedSec: number;
  timingSec: TranscribeTimingSec;
  rtfX: number;
  rtfBreakdownX: TranscribeRtfBreakdownX;
  executionProvider: string;
};

type TranscribeTimingSec = {
  prepareElapsedSec: number;
  vadElapsedSec: number;
  tempWavWriteSec: number;
  asrLoadSec: number;
  asrTranscribeSec: number;
  qwenLoadSec: number;
  qwenAlignSec: number;
  punctuationMapSec: number;
  totalElapsedSec: number;
};

type TranscribeRtfBreakdownX = {
  total: number;
  asrStage: number;
  asrTranscribe: number;
  qwenStage: number;
  qwenAlign: number;
  modelOnly: number;
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
