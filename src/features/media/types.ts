// Settings-related types are re-exported from ts-rs generated bindings
// (single source of truth). Other domain types remain hand-written here.
export type { AlignModel } from "../../generated/bindings/AlignModel";
export type { AsrModel } from "../../generated/bindings/AsrModel";
export type { DefaultSettingsResponse } from "../../generated/bindings/DefaultSettingsResponse";
export type { DemucsModel } from "../../generated/bindings/DemucsModel";
export type { Provider } from "../../generated/bindings/Provider";
export type { SaveAppSettingsRequest } from "../../generated/bindings/SaveAppSettingsRequest";
export type { SavedSettings } from "../../generated/bindings/SavedSettings";
export type { SubtitleBorderStyle } from "../../generated/bindings/SubtitleBorderStyle";
export type { SubtitleBurnMode } from "../../generated/bindings/SubtitleBurnMode";
export type { SubtitleLayoutStyle } from "../../generated/bindings/SubtitleLayoutStyle";
export type { SubtitleLengthPreset } from "../../generated/bindings/SubtitleLengthPreset";
export type { SubtitleLineStyle } from "../../generated/bindings/SubtitleLineStyle";
export type { SubtitleRenderStyle } from "../../generated/bindings/SubtitleRenderStyle";
export type { TerminologyGroup } from "../../generated/bindings/TerminologyGroup";
export type { TerminologyTerm } from "../../generated/bindings/TerminologyTerm";
export type { UserPreferencesResponse } from "../../generated/bindings/UserPreferencesResponse";

// Runtime constants (not types) stay here.
export type ModelTarget = "asr" | "align" | "demucs";
export type QueueStatus = "pending" | "queued" | "processing" | "done" | "error";
export type TranscribeStatus = QueueStatus;
export type SourceLanguage = "en" | "zh" | "yue" | "ja" | "ko" | "fr" | "de" | "it" | "es" | "pt" | "ru";
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
  | "burning"
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
  "burning",
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
  burning: 95,
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
