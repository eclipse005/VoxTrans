import { useTranslation } from "react-i18next";
import type { ModelStatusResponse } from "../../../features/media/types";
import { CheckIcon, DownloadIcon, FolderIcon } from "../Icons";

type ModelDownloadTarget = "asr" | "align" | "demucs";

type ModelDownloadCardProps = {
  target: ModelDownloadTarget;
  title: string;
  description: string;
  modelName: string;
  selected: boolean;
  status: ModelStatusResponse | null;
  onSelect: () => void;
  onOpenModelDir: (target: ModelDownloadTarget, model?: string) => void | Promise<void>;
  onStartModelDownload: (target: ModelDownloadTarget, model?: string) => void | Promise<void>;
  onCancelModelDownload: (target: ModelDownloadTarget, model?: string) => void | Promise<void>;
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
  const { t } = useTranslation(["models"]);
  const ready = isReady(status);
  const downloading = status?.download.phase === "downloading";
  const percent = progressPercent(status);
  const sizeText = formatModelSizeText(status);
  const targetLabel = target === "asr" ? "ASR" : target === "align" ? "Align" : "Demucs";

  return (
    <div className="model-task-card">
      <div className="model-task-card-head">
        <h4 className="apple-heading-small">{title}</h4>
        <span className={`model-ready-pill ${ready ? "ready" : "not-ready"}`}>
          {ready ? t("models:card.ready") : t("models:card.notReady")}
        </span>
      </div>
      <p className="apple-body-small">{description}</p>
      <div className="model-inline-row">
        <div className="device-toggle-group model-inline-model" role="group" aria-label={`${targetLabel} ${t("models:card.versionLabel")}`}>
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
            title={t("models:card.openDir")}
            aria-label={t("models:card.openDirAria", { target: targetLabel })}
            onClick={() => { void onOpenModelDir(target, modelName); }}
          >
            <FolderIcon />
          </button>
          <button
            className={`file-list-icon-btn model-download-state-btn ${ready ? "is-ready" : downloading ? "is-downloading" : "is-idle"}`}
            type="button"
            title={downloading ? t("models:card.cancelDownload") : ready ? t("models:card.ready") : t("models:card.download")}
            aria-label={downloading
              ? t("models:card.cancelDownloadAria", { target: targetLabel })
              : ready
                ? t("models:card.readyAria", { target: targetLabel })
                : t("models:card.downloadAria", { target: targetLabel })}
            onClick={() => {
              if (downloading) {
                void onCancelModelDownload(target, modelName);
                return;
              }
              if (!ready) {
                void onStartModelDownload(target, modelName);
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
