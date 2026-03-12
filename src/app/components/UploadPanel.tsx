import type { CSSProperties } from "react";
import type { UploadTab } from "../types";
import { DownloadIcon, UploadIcon, YoutubeIcon } from "./Icons";

type UploadPanelProps = {
  activeTab: UploadTab;
  dragActive: boolean;
  youtubeUrl: string;
  onTabChange: (tab: UploadTab) => void;
  onPickFiles: () => void | Promise<void>;
  onYoutubeUrlChange: (value: string) => void;
  onYoutubeDownload: () => void;
};

export default function UploadPanel({
  activeTab,
  dragActive,
  youtubeUrl,
  onTabChange,
  onPickFiles,
  onYoutubeUrlChange,
  onYoutubeDownload,
}: UploadPanelProps) {
  const tabIndex = activeTab === "local" ? 0 : 1;
  const tabIndicatorStyle = { ["--upload-tab-index" as string]: tabIndex } as CSSProperties;
  const showYoutubeProgress = false;
  const youtubeTitle = "";
  const youtubeSpeed = "";
  const youtubeSize = "";

  return (
    <div className="apple-animate-on-scroll apple-delay-100 upload-section animated">
      <div className="sidebar-title-group">
        <h3 className="sidebar-title">媒体导入</h3>
      </div>

      <div className="upload-tabs" style={tabIndicatorStyle}>
        <div className="upload-tab-indicator" />
        <button className={`tab-button ${activeTab === "local" ? "active" : ""}`} onClick={() => onTabChange("local")}>
          <UploadIcon />
          本地文件
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
                <h3 className="upload-title">拖拽或点击上传</h3>
                <p className="upload-hint">支持多选音视频文件</p>
              </div>
              <button className="apple-button upload-select-btn" type="button">
                上传文件
              </button>
            </div>
          </div>
        </div>

        <div className={`upload-panel-content ${activeTab === "youtube" ? "active" : ""}`} aria-hidden={activeTab !== "youtube"}>
          <div className="youtube-download-area youtube-download-compact">
            <div className="youtube-headline">
              <YoutubeIcon />
              <span>YouTube 下载</span>
            </div>

            <div className="youtube-input-group">
              <input
                type="text"
                className="youtube-url-input"
                placeholder="粘贴 YouTube 链接"
                value={youtubeUrl}
                onChange={(e) => onYoutubeUrlChange(e.target.value)}
                autoComplete="off"
              />
              <button
                className="youtube-download-icon-btn"
                type="button"
                onClick={onYoutubeDownload}
                disabled={!youtubeUrl.trim()}
                aria-label="下载视频"
                title="下载视频"
              >
                <DownloadIcon />
              </button>
            </div>
            {showYoutubeProgress ? (
              <div className="youtube-progress-shell" aria-label="下载进度">
                {youtubeTitle ? (
                  <div className="youtube-progress-head">
                    <span className="youtube-progress-title" title={youtubeTitle}>{youtubeTitle}</span>
                  </div>
                ) : null}
                <div className="youtube-progress-track">
                  <div className="youtube-progress-fill" style={{ width: "0%" }} />
                </div>
                {youtubeSpeed || youtubeSize ? (
                  <div className="youtube-progress-meta">
                    <span>{youtubeSpeed}</span>
                    <span>{youtubeSize}</span>
                  </div>
                ) : null}
              </div>
            ) : null}
          </div>
        </div>
      </div>
    </div>
  );
}
