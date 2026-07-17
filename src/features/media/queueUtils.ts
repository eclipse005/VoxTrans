import {
  normalizeSourceLanguage,
  normalizeTargetLanguage,
} from "./languages";
import {
  normalizeTaskProgress,
  type LanguageTag,
  type QueueItem,
  type TargetLanguage,
  type TaskProgress,
  type TaskStageProgress,
} from "./types";
import { normalizeTranscribeStatus } from "./stateMachine";

export type QueueRunMode = "transcribe" | "transcribe_translate" | "translate_srt";

type MediaKind = "audio" | "video" | "subtitle";

function normalizeMediaKind(value: unknown): MediaKind {
  if (value === "video") return "video";
  if (value === "subtitle") return "subtitle";
  return "audio";
}

export function isSubtitleQueueItem(item: { mediaKind?: string; path?: string; name?: string }): boolean {
  if (item.mediaKind === "subtitle") return true;
  const path = (item.path ?? "").replace(/\\/g, "/");
  const name = item.name ?? "";
  const leaf = path.includes("/") ? path.slice(path.lastIndexOf("/") + 1) : path;
  return /\.srt$/i.test(leaf) || /\.srt$/i.test(name);
}

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
  terminologyGroupId?: string;
  reviewSource?: boolean;
  reviewTarget?: boolean;
  resumeFrom?: string;
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
    mediaKind: normalizeMediaKind(payload.mediaKind),
    sizeBytes: payload.sizeBytes,
    sourceLang: normalizeSourceLanguage(payload.sourceLang ?? current.sourceLang),
    targetLang: normalizeTargetLanguage(payload.targetLang ?? current.targetLang),
    transcribeStatus: normalizeTranscribeStatus(payload.transcribeStatus),
    taskProgress: keepCurrentStage ? current.taskProgress : nextProgress,
    transcribeError: payload.transcribeError || "",
    resultText: payload.resultText || "",
    resultSrt: payload.resultSrt || "",
    // Prefer payload JSON when present (including "[]"); only fall back if
    // the field is missing so a partial-shaped event cannot wipe a stream.
    subtitleSegmentsJson: typeof payload.subtitleSegmentsJson === "string"
      ? payload.subtitleSegmentsJson
      : (current.subtitleSegmentsJson || ""),
    terminologyGroupId: payload.terminologyGroupId || current.terminologyGroupId,
    // Task-level review flags: prefer event payload, else keep current so
    // progress ticks never wipe checkboxes the user already set.
    reviewSource: typeof payload.reviewSource === "boolean"
      ? payload.reviewSource
      : Boolean(current.reviewSource),
    reviewTarget: typeof payload.reviewTarget === "boolean"
      ? payload.reviewTarget
      : Boolean(current.reviewTarget),
    resumeFrom: typeof payload.resumeFrom === "string"
      ? payload.resumeFrom
      : (current.resumeFrom ?? ""),
  };
}

export function toEnqueuePayload(
  item: QueueItem,
  mode: QueueRunMode,
): {
  id: string;
  mediaPath: string;
  name: string;
  mediaKind: "audio" | "video" | "subtitle";
  sizeBytes: number;
  intent: "TRANSCRIBE" | "TRANSCRIBE_TRANSLATE" | "TRANSLATE_SRT";
  sourceLang: LanguageTag;
  targetLang: TargetLanguage;
  maxRetries: number;
  terminologyGroupId: string;
} {
  const srt = isSubtitleQueueItem(item);
  return {
    id: item.id,
    mediaPath: item.path,
    name: item.name,
    mediaKind: srt ? "subtitle" : item.mediaKind,
    sizeBytes: item.sizeBytes,
    intent: srt ? "TRANSLATE_SRT" : toIntent(mode),
    sourceLang: normalizeSourceLanguage(item.sourceLang),
    targetLang: normalizeTargetLanguage(item.targetLang),
    maxRetries: 0,
    terminologyGroupId: item.terminologyGroupId ?? "",
  };
}

function toIntent(mode: QueueRunMode): "TRANSCRIBE" | "TRANSCRIBE_TRANSLATE" | "TRANSLATE_SRT" {
  if (mode === "translate_srt") return "TRANSLATE_SRT";
  if (mode === "transcribe_translate") return "TRANSCRIBE_TRANSLATE";
  return "TRANSCRIBE";
}
