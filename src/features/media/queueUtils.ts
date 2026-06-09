import {
  normalizeSourceLanguage,
  normalizeTargetLanguage,
} from "./languages";
import {
  normalizeTaskProgress,
  type QueueItem,
  type SourceLanguage,
  type TargetLanguage,
  type TaskProgress,
  type TaskStageProgress,
} from "./types";

export type QueueRunMode = "transcribe" | "transcribe_translate";

type TaskStateChangedEvent = {
  id: string;
  path: string;
  name: string;
  mediaKind: string;
  sizeBytes: number;
  sourceLang?: string;
  targetLang?: string;
  transcribeStatus: string;
  taskProgress: TaskProgress;
  transcribeError: string;
  resultText: string;
  resultSrt: string;
  subtitleSegmentsJson: string;
};

export function stageOrder(stage: Partial<TaskStageProgress> | null | undefined): number {
  if (!stage) return 0;
  const value = Number(stage.order ?? 0);
  if (!Number.isFinite(value)) return 0;
  return Math.max(0, Math.round(value));
}

export function stageRatio(stage: Partial<TaskStageProgress> | null | undefined): number {
  if (!stage) return 0;
  const current = Number(stage.current ?? 0);
  const total = Number(stage.total ?? 0);
  if (!Number.isFinite(current) || !Number.isFinite(total) || total <= 0) return 0;
  return Math.max(0, Math.min(1, current / total));
}

export function shouldKeepCurrentProcessingStage(
  current: QueueItem,
  incoming: TaskStateChangedEvent,
): boolean {
  if (current.transcribeStatus !== "processing" || incoming.transcribeStatus !== "processing") {
    return false;
  }
  const currentOrder = stageOrder(current.taskProgress.stage);
  const incomingOrder = stageOrder(incoming.taskProgress?.stage);
  if (incomingOrder > 0 && currentOrder > incomingOrder) {
    return true;
  }
  if (incomingOrder > 0 && currentOrder === incomingOrder) {
    // Same stage — accept the latest event; the backend is the source of
    // truth for progress.  Never second-guess monotonicty here because
    // multi-round stages legitimately increase total after a round finishes.
    return false;
  }
  return false;
}

export function mergeTaskStateChanged(current: QueueItem, payload: TaskStateChangedEvent): QueueItem {
  const keepCurrentStage = shouldKeepCurrentProcessingStage(current, payload);
  const nextProgress = normalizeTaskProgress(payload.taskProgress);
  return {
    id: payload.id,
    path: payload.path,
    name: payload.name,
    mediaKind: payload.mediaKind as "audio" | "video",
    sizeBytes: payload.sizeBytes,
    sourceLang: normalizeSourceLanguage(payload.sourceLang ?? current.sourceLang),
    targetLang: normalizeTargetLanguage(payload.targetLang ?? current.targetLang),
    transcribeStatus: payload.transcribeStatus as QueueItem["transcribeStatus"],
    taskProgress: keepCurrentStage ? current.taskProgress : nextProgress,
    transcribeError: payload.transcribeError || "",
    resultText: payload.resultText || "",
    resultSrt: payload.resultSrt || "",
    subtitleSegmentsJson: payload.subtitleSegmentsJson || "",
  };
}

export function toEnqueuePayload(
  item: QueueItem,
  mode: QueueRunMode,
): {
  id: string;
  mediaPath: string;
  name: string;
  mediaKind: "audio" | "video";
  sizeBytes: number;
  intent: "TRANSCRIBE" | "TRANSCRIBE_TRANSLATE";
  sourceLang: SourceLanguage;
  targetLang: TargetLanguage;
  maxRetries: number;
} {
  return {
    id: item.id,
    mediaPath: item.path,
    name: item.name,
    mediaKind: item.mediaKind,
    sizeBytes: item.sizeBytes,
    intent: toIntent(mode),
    sourceLang: normalizeSourceLanguage(item.sourceLang),
    targetLang: normalizeTargetLanguage(item.targetLang),
    maxRetries: 0,
  };
}

function toIntent(mode: QueueRunMode): "TRANSCRIBE" | "TRANSCRIBE_TRANSLATE" {
  if (mode === "transcribe_translate") return "TRANSCRIBE_TRANSLATE";
  return "TRANSCRIBE";
}
