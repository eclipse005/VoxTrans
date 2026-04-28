import type { ModelStatusResponse } from "../../../features/media/types";
import { CheckIcon, DownloadIcon, FolderIcon } from "../Icons";

type ModelDownloadTarget = "asr" | "demucs";

type ModelDownloadCardProps = {
  target: ModelDownloadTarget;
  title: string;
  description: string;
  modelName: string;
  selected: boolean;
  status: ModelStatusResponse | null;
  onSelect: () => void;
  onOpenModelDir: (target: ModelDownloadTarget) => void | Promise<void>;
  onStartModelDownload: (target: ModelDownloadTarget) => void | Promise<void>;
  onCancelModelDownload: (target: ModelDownloadTarget) => void | Promise<void>;
};

function formatBytes(value: number): string {
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

function progressPercent(status: ModelStatusResponse | null): number {
  const total = status?.download.totalBytes ?? 0;
  const downloaded = status?.download.downloadedBytes ?? 0;
  if (total <= 0) return 0;
  return Math.max(0, Math.min(100, Math.round((downloaded / total) * 100)));
}

function isReady(status: ModelStatusResponse | null): boolean {
  if (!status) return false;
  return status.ready || status.download.phase === "completed";
}

function formatModelSizeText(status: ModelStatusResponse | null): string {
  if (!status) return "-";
  const downloaded = status.download.downloadedBytes;
  const total = status.download.totalBytes;
  if (total <= 0) return "-";
  if (status.download.phase === "downloading") {
    return `${formatBytes(downloaded)} / ${formatBytes(total)}`;
  }
  return formatBytes(total);
}

export function ModelDownloadCard({
  target,
  title,
  description,
  modelName,
  selected,
  status,
  onSelect,
  onOpenModelDir,
  onStartModelDownload,
  onCancelModelDownload,
}: ModelDownloadCardProps) {
  const ready = isReady(status);
  const downloading = status?.download.phase === "downloading";
  const percent = progressPercent(status);
  const sizeText = formatModelSizeText(status);
  const targetLabel = target === "asr" ? "ASR" : "Demucs";

  return (
    <div className="model-task-card">
      <div className="model-task-card-head">
        <h4 className="apple-heading-small">{title}</h4>
        <span className={`model-ready-pill ${ready ? "ready" : "not-ready"}`}>
          {ready ? "已就绪" : "未就绪"}
        </span>
      </div>
      <p className="apple-body-small">{description}</p>
      <div className="model-inline-row">
        <div className="device-toggle-group model-inline-model" role="group" aria-label={`${targetLabel} 版本`}>
          <button
            type="button"
            className={`device-toggle-btn ${selected ? "active" : ""}`}
            onClick={onSelect}
          >
            <span className="model-inline-label">
              <span>{modelName}</span>
              <span className="model-inline-size">{sizeText}</span>
            </span>
          </button>
        </div>
        <div className="model-task-actions model-task-actions-inline">
          <button
            className="file-list-icon-btn"
            type="button"
            title="打开目录"
            aria-label={`打开 ${targetLabel} 模型目录`}
            onClick={() => { void onOpenModelDir(target); }}
          >
            <FolderIcon />
          </button>
          <button
            className={`file-list-icon-btn model-download-state-btn ${ready ? "is-ready" : downloading ? "is-downloading" : "is-idle"}`}
            type="button"
            title={downloading ? "取消下载" : ready ? "已就绪" : "下载模型"}
            aria-label={downloading ? `取消 ${targetLabel} 下载` : ready ? `${targetLabel} 已就绪` : `下载 ${targetLabel} 模型`}
            onClick={() => {
              if (downloading) {
                void onCancelModelDownload(target);
                return;
              }
              if (!ready) {
                void onStartModelDownload(target);
              }
            }}
            disabled={ready}
            style={{ ["--ring-progress" as string]: `${percent}%` }}
          >
            {downloading ? (
              <span className="model-progress-ring-inner">{percent}%</span>
            ) : ready ? (
              <CheckIcon />
            ) : (
              <DownloadIcon />
            )}
          </button>
        </div>
      </div>
    </div>
  );
}
