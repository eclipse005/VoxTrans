import type { QueueStatus } from "./types";

export function fileName(path: string): string {
  return path.replace(/\\/g, "/").split("/").pop() ?? path;
}

export function detectMediaKind(path: string): "audio" | "video" {
  const ext = path.split(".").pop()?.toLowerCase() ?? "";
  const videoExts = new Set(["mp4", "mkv", "mov", "avi", "webm", "m4v"]);
  return videoExts.has(ext) ? "video" : "audio";
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
  if (status === "done") return "common:status.done";
  return "common:status.failed";
}
