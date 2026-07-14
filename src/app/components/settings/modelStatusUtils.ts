import type { ModelStatusResponse } from "../../../features/media/types";

export function formatBytes(value: number): string {
  if (!Number.isFinite(value) || value <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  let size = value;
  let idx = 0;
  while (size >= 1024 && idx < units.length - 1) {
    size /= 1024;
    idx += 1;
  }
  return `${size.toFixed(idx === 0 ? 0 : 2)} ${units[idx]}`;
}

export function progressPercent(status: ModelStatusResponse | null): number {
  const total = status?.download.totalBytes ?? 0;
  const downloaded = status?.download.downloadedBytes ?? 0;
  if (total <= 0) return 0;
  return Math.max(0, Math.min(100, Math.round((downloaded / total) * 100)));
}

export function isModelReady(status: ModelStatusResponse | null): boolean {
  if (!status) return false;
  return status.ready || status.download.phase === "completed";
}

export function isModelDownloading(status: ModelStatusResponse | null): boolean {
  return status?.download.phase === "downloading";
}

export function formatModelSizeText(status: ModelStatusResponse | null): string {
  if (!status) return "—";
  const downloaded = status.download.downloadedBytes;
  const total = status.download.totalBytes;
  if (total <= 0) return "—";
  if (status.download.phase === "downloading") {
    return `${formatBytes(downloaded)} / ${formatBytes(total)}`;
  }
  return formatBytes(total);
}

export function formatDownloadSpeed(status: ModelStatusResponse | null): string | null {
  if (!status || status.download.phase !== "downloading") return null;
  const speed = status.download.speedBytesPerSec;
  if (!Number.isFinite(speed) || speed <= 0) return null;
  return `${formatBytes(speed)}/s`;
}
