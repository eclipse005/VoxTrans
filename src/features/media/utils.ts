import type { QueueStatus } from "./types";

export function fileName(path: string): string {
  return path.replace(/\\/g, "/").split("/").pop() ?? path;
}

export type DetectedFileKind = "audio" | "video" | "subtitle";

const VIDEO_EXTS = new Set(["mp4", "mkv", "mov", "avi", "webm", "m4v"]);
const AUDIO_EXTS = new Set(["mp3", "wav", "m4a", "flac", "aac", "ogg", "opus"]);
const SUBTITLE_EXTS = new Set(["srt"]);

export function fileExtension(path: string): string {
  return path.split(".").pop()?.toLowerCase() ?? "";
}

export function detectMediaKind(path: string): DetectedFileKind {
  const ext = fileExtension(path);
  if (SUBTITLE_EXTS.has(ext)) return "subtitle";
  if (VIDEO_EXTS.has(ext)) return "video";
  if (AUDIO_EXTS.has(ext)) return "audio";
  // Unknown extension: treat as audio for backward compatibility of non-srt paths.
  return "audio";
}

export function isSupportedUploadPath(path: string): boolean {
  const ext = fileExtension(path);
  return VIDEO_EXTS.has(ext) || AUDIO_EXTS.has(ext) || SUBTITLE_EXTS.has(ext);
}

export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "--";
  const units = ["B", "KB", "MB", "GB"];
  let value = bytes;
  let unitIndex = 0;
  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }
  return `${value.toFixed(unitIndex === 0 ? 0 : 2)} ${units[unitIndex]}`;
}

export function statusLabel(status: QueueStatus): string {
  if (status === "pending") return "common:status.pending";
  if (status === "queued") return "common:status.queued";
  if (status === "processing") return "common:status.processing";
  if (status === "review_source") return "common:status.reviewSource";
  if (status === "review_target") return "common:status.reviewTarget";
  if (status === "done") return "common:status.done";
  return "common:status.failed";
}
