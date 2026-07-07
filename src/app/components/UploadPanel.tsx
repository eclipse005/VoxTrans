import type { CSSProperties } from "react";
import { useTranslation } from "react-i18next";
import type { UploadTab } from "../types";
import { DownloadIcon, UpdateIcon, UploadIcon, YoutubeIcon } from "./Icons";

type UploadPanelProps = {
  activeTab: UploadTab;
  dragActive: boolean;
  youtubeUrl: string;
  ytDlpVersion: string;
  ytDlpUpdating: boolean;
  onTabChange: (tab: UploadTab) => void;
  onPickFiles: () => void | Promise<void>;
  onYoutubeUrlChange: (value: string) => void;
  onYoutubeDownload: () => void;
  onUpdateYtDlp: () => void;
};

export default function UploadPanel({
  activeTab,
  dragActive,
  youtubeUrl,
  ytDlpVersion,
  ytDlpUpdating,
  onTabChange,
  onPickFiles,
  onYoutubeUrlChange,
  onYoutubeDownload,
  onUpdateYtDlp,
}: UploadPanelProps) {
  const { t } = useTranslation(["tasks"]);
  const tabIndex = activeTab === "local" ? 0 : 1;
  const tabIndicatorStyle = { ["--upload-tab-index" as string]: tabIndex } as CSSProperties;

  return (
    <div className="apple-animate-on-scroll apple-delay-100 upload-section animated">
      <div className="sidebar-title-group">
        <h3 className="sidebar-title">{t("tasks:upload.title")}</h3>
      </div>

      <div className="upload-tabs" style={tabIndicatorStyle}>
        <div className="upload-tab-indicator" />
        <button className={`tab-button ${activeTab === "local" ? "active" : ""}`} onClick={() => onTabChange("local")}>
          <UploadIcon />
          {t("tasks:upload.localTab")}
        </button>
        <button className={`tab-button ${activeTab === "youtube" ? "active" : ""}`} onClick={() => onTabChange("youtube")}>
          <YoutubeIcon />
          YouTube
        </button>
      </div>

      <div className="upload-panel-body">
        <div className={`upload-panel-content ${activeTab === "local" ? "active" : ""}`} aria-hidden={activeTab !== "local"}>
          <div
            className={`upload-area upload-area-compact ${dragActive ? "drag-over" : ""}`}
            role="button"
            tabIndex={activeTab === "local" ? 0 : -1}
            onClick={activeTab === "local" ? onPickFiles : undefined}
            onKeyDown={(e) => {
              if ((e.key === "Enter" || e.key === " ") && activeTab === "local") {
                e.preventDefault();
                void onPickFiles();
              }
            }}
          >
            <div className="upload-content">
              <div className="upload-text">
                <h3 className="upload-title">{t("tasks:upload.dropTitle")}</h3>
                <p className="upload-hint">{t("tasks:upload.dropHint")}</p>
              </div>
              <button className="apple-button upload-select-btn" type="button">
                {t("tasks:upload.selectBtn")}
              </button>
            </div>
          </div>
        </div>

        <div className={`upload-panel-content ${activeTab === "youtube" ? "active" : ""}`} aria-hidden={activeTab !== "youtube"}>
          <div className="youtube-download-area youtube-download-compact">
            <div className="youtube-center-wrap">
              <div className="youtube-headline">
                <YoutubeIcon />
                <span>{t("tasks:youtube.title")}</span>
              </div>
              <div className="youtube-input-group">
                <input
                  type="text"
                  className="youtube-url-input"
                  placeholder={t("tasks:youtube.urlPlaceholder")}
                  value={youtubeUrl}
                  onChange={(e) => onYoutubeUrlChange(e.target.value)}
                  autoComplete="off"
                />
                <button
                  className="youtube-download-icon-btn"
                  type="button"
                  onClick={onYoutubeDownload}
                  disabled={!youtubeUrl.trim()}
                  aria-label={t("tasks:youtube.download")}
                  title={t("tasks:youtube.download")}
                >
                  <DownloadIcon />
                </button>
              </div>
              <div className="youtube-tools-row">
                <div className="youtube-tools-version-wrap">
                  <span className="youtube-tools-version">
                    yt-dlp: {ytDlpVersion || t("tasks:youtube.notDetected")}
                  </span>
                  <button
                    type="button"
                    className="youtube-update-icon-btn"
                    onClick={onUpdateYtDlp}
                    disabled={ytDlpUpdating}
                    aria-label={ytDlpUpdating ? t("tasks:youtube.updating") : t("tasks:youtube.update")}
                    title={ytDlpUpdating ? t("tasks:youtube.updatingShort") : t("tasks:youtube.update")}
                  >
                    <UpdateIcon />
                  </button>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
