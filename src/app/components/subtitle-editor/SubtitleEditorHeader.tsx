import { useTranslation } from "react-i18next";
import { DownloadIcon, LogsIcon } from "../Icons";

type SubtitleEditorHeaderProps = {
  canEdit: boolean;
  readOnlyReason?: string;
  cueCount: number;
  taskName: string;
  reviewSource?: boolean;
  reviewTarget?: boolean;
  reviewBanner?: string;
  onToggleReviewSource?: (value: boolean) => void;
  onToggleReviewTarget?: (value: boolean) => void;
  onOpenSrtDir: () => void | Promise<void>;
  onExportSrt: () => void | Promise<void>;
  onOpenLogs: () => void | Promise<void>;
};

export default function SubtitleEditorHeader({
  canEdit,
  readOnlyReason = "",
  cueCount,
  taskName,
  reviewSource = false,
  reviewTarget = false,
  reviewBanner = "",
  onToggleReviewSource,
  onToggleReviewTarget,
  onOpenSrtDir,
  onExportSrt,
  onOpenLogs,
}: SubtitleEditorHeaderProps) {
  const { t } = useTranslation(["subtitles", "common"]);
  return (
    <div className="subtitle-editor-header">
      <div className="subtitle-header-main">
        <div className="subtitle-title-row">
          <h3 className="apple-heading-small">{t("subtitles:header.title")}</h3>
          <span className="subtitle-count-badge">{t("subtitles:header.cueCount", { count: cueCount })}</span>
        </div>
        <div className="apple-body-small subtitle-editor-meta">
          <div className="subtitle-meta-row">
            <span className="subtitle-meta-label">{t("subtitles:header.taskLabel")}</span>
            <button
              type="button"
              className="subtitle-task-link"
              onClick={(e) => {
                e.stopPropagation();
                void onOpenSrtDir();
              }}
              aria-label={t("subtitles:header.openTaskDir")}
              title={t("subtitles:header.openTaskDir")}
            >
              <span title={taskName || "--"}>{taskName || "--"}</span>
            </button>
          </div>
          {onToggleReviewSource || onToggleReviewTarget ? (
            <div className="subtitle-meta-row subtitle-review-toggles">
              <label className="subtitle-review-toggle">
                <input
                  type="checkbox"
                  checked={reviewSource}
                  onChange={(e) => onToggleReviewSource?.(e.target.checked)}
                />
                <span>{t("subtitles:review.source")}</span>
              </label>
              <label className="subtitle-review-toggle">
                <input
                  type="checkbox"
                  checked={reviewTarget}
                  onChange={(e) => onToggleReviewTarget?.(e.target.checked)}
                />
                <span>{t("subtitles:review.target")}</span>
              </label>
            </div>
          ) : null}
          {reviewBanner ? (
            <div className="subtitle-meta-row subtitle-review-banner">
              <span>{reviewBanner}</span>
            </div>
          ) : null}
          {!canEdit && readOnlyReason ? (
            <div className="subtitle-meta-row">
              <span className="subtitle-meta-label">{t("subtitles:header.statusLabel")}</span>
              <span>{readOnlyReason}</span>
            </div>
          ) : null}
        </div>
      </div>
      <div className="subtitle-header-actions">
        <button
          type="button"
          className="subtitle-header-icon-btn"
          onClick={(e) => {
            e.stopPropagation();
            void onExportSrt();
          }}
          aria-label={t("subtitles:header.exportSrt")}
          title={t("subtitles:header.export")}
        >
          <DownloadIcon />
          <span>{t("subtitles:header.export")}</span>
        </button>
        <button
          type="button"
          className="subtitle-header-icon-btn"
          onClick={(e) => {
            e.stopPropagation();
            void onOpenLogs();
          }}
          aria-label={t("subtitles:header.openLogs")}
          title={t("subtitles:header.logs")}
        >
          <LogsIcon />
          <span>{t("subtitles:header.logs")}</span>
        </button>
      </div>
    </div>
  );
}
