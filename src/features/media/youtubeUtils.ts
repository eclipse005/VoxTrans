import {
  createTaskProgress,
  type QueueItem,
} from "./types";
import { DEFAULT_SOURCE_LANGUAGE, DEFAULT_TARGET_LANGUAGE } from "./languages";

const YOUTUBE_PLACEHOLDER_PREFIX = "youtube://pending/";

export function encodeYoutubePlaceholderPath(taskId: string, url: string): string {
  return `${YOUTUBE_PLACEHOLDER_PREFIX}${taskId}?url=${encodeURIComponent(url)}`;
}

export function decodeYoutubeUrlFromPath(path: string): string {
  if (!path.startsWith(YOUTUBE_PLACEHOLDER_PREFIX)) return "";
  const queryIndex = path.indexOf("?");
  if (queryIndex < 0) return "";
  const query = path.slice(queryIndex + 1);
  const params = new URLSearchParams(query);
  return (params.get("url") || "").trim();
}

export function isYoutubePlaceholderPath(path: string): boolean {
  return path.startsWith(YOUTUBE_PLACEHOLDER_PREFIX);
}

export function normalizeTitle(raw: string): string {
  const text = (raw || "").trim();
  if (!text) return "";
  const slashNormalized = text.replace(/\\/g, "/");
  const base = slashNormalized.split("/").pop() || slashNormalized;
  const withoutExt = base.replace(/\.[a-zA-Z0-9]{2,5}$/u, "");
  return withoutExt.replace(/\.f\d+$/u, "").trim();
}

export function createYoutubePlaceholderTask(
  taskId: string,
  path: string,
  name: string,
  sizeBytes: number,
  progress: number,
): QueueItem {
  return {
    id: taskId,
    path,
    name,
    mediaKind: "video",
    sizeBytes,
    sourceLang: DEFAULT_SOURCE_LANGUAGE,
    targetLang: DEFAULT_TARGET_LANGUAGE,
    transcribeStatus: "processing",
    taskProgress: createTaskProgress({
      code: "downloading",
      label: "下载中",
      detail: `${progress}%`,
      current: progress,
      total: 100,
    }),
    transcribeError: "",
    resultText: "",
    resultSrt: "",
    subtitleSegmentsJson: "[]",
  };
}

export function isCancelledMessage(message: string): boolean {
  const value = message.toLowerCase();
  return value.includes("取消") || value.includes("cancel");
}

export function clampPercent(value: number): number {
  if (!Number.isFinite(value)) return 0;
  return Math.max(0, Math.min(100, Math.round(value)));
}

export function parseSizeToBytes(raw: string): number {
  const text = (raw || "").trim();
  if (!text) return 0;
  const matched = text.match(/^(\d+(?:\.\d+)?)\s*([a-zA-Z]+)$/);
  if (!matched) return 0;
  const value = Number(matched[1]);
  if (!Number.isFinite(value) || value <= 0) return 0;
  const unit = matched[2].toLowerCase();
  const factorMap: Record<string, number> = {
    b: 1,
    kb: 1000,
    mb: 1000 ** 2,
    gb: 1000 ** 3,
    tb: 1000 ** 4,
    kib: 1024,
    mib: 1024 ** 2,
    gib: 1024 ** 3,
    tib: 1024 ** 4,
  };
  const factor = factorMap[unit];
  if (!factor) return 0;
  return Math.round(value * factor);
}
