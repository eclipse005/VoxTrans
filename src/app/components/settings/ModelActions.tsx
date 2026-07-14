import { useTranslation } from "react-i18next";
import type { ModelStatusResponse, ModelTarget } from "../../../features/media/types";
import { CheckIcon, DownloadIcon, FolderIcon } from "../Icons";
import {
  formatDownloadSpeed,
  formatModelSizeText,
  isModelDownloading,
  isModelReady,
  progressPercent,
} from "./modelStatusUtils";

type ModelActionsProps = {
  target: ModelTarget;
  modelName: string;
  status: ModelStatusResponse | null;
  onOpenModelDir: (target: ModelTarget, model?: string) => void | Promise<void>;
  onStartModelDownload: (target: ModelTarget, model?: string) => void | Promise<void>;
  onCancelModelDownload: (target: ModelTarget, model?: string) => void | Promise<void>;
  /** When true, ready models still offer re-download on click. */
  allowRedownload?: boolean;
};

export function ModelActions({
  target,
  modelName,
  status,
  onOpenModelDir,
  onStartModelDownload,
  onCancelModelDownload,
  allowRedownload = false,
}: ModelActionsProps) {
  const { t } = useTranslation(["models"]);
  const ready = isModelReady(status);
  const downloading = isModelDownloading(status);
  const percent = progressPercent(status);
  const sizeText = formatModelSizeText(status);
  const speedText = formatDownloadSpeed(status);
  const targetLabel = target === "asr" ? "ASR" : target === "align" ? "Align" : "Demucs";

  return (
    <div className="model-actions">
      <div className="model-actions-meta" aria-live="polite">
        <span className="model-actions-size">{sizeText}</span>
        {speedText ? <span className="model-actions-speed">{speedText}</span> : null}
      </div>
      <div className="model-task-actions model-task-actions-inline">
        <button
          className="file-list-icon-btn"
          type="button"
          title={t("models:card.openDir")}
          aria-label={t("models:card.openDirAria", { target: targetLabel })}
          onClick={() => {
            void onOpenModelDir(target, modelName);
          }}
        >
          <FolderIcon />
        </button>
        <button
          className={`file-list-icon-btn model-download-state-btn ${
            ready ? "is-ready" : downloading ? "is-downloading" : "is-idle"
          }`}
          type="button"
          title={
            downloading
              ? t("models:card.cancelDownload")
              : ready
                ? allowRedownload
                  ? t("models:card.redownload")
                  : t("models:card.ready")
                : t("models:card.download")
          }
          aria-label={
            downloading
              ? t("models:card.cancelDownloadAria", { target: targetLabel })
              : ready
                ? allowRedownload
                  ? t("models:card.redownloadAria", { target: targetLabel })
                  : t("models:card.readyAria", { target: targetLabel })
                : t("models:card.downloadAria", { target: targetLabel })
          }
          onClick={() => {
            if (downloading) {
              void onCancelModelDownload(target, modelName);
              return;
            }
            if (!ready || allowRedownload) {
              void onStartModelDownload(target, modelName);
            }
          }}
          disabled={ready && !allowRedownload}
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
  );
}
