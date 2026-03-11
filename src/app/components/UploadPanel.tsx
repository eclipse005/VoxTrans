import type { CSSProperties } from "react";
import type { UploadTab } from "../types";
import { DownloadIcon, UploadIcon, YoutubeIcon } from "./Icons";

type UploadPanelProps = {
  activeTab: UploadTab;
  dragActive: boolean;
  youtubeUrl: string;
  youtubeQuality: string;
  onTabChange: (tab: UploadTab) => void;
  onPickFiles: () => void | Promise<void>;
  onYoutubeUrlChange: (value: string) => void;
  onYoutubeQualityChange: (value: string) => void;
  onYoutubeDownload: () => void;
};

export default function UploadPanel({
  activeTab,
  dragActive,
  youtubeUrl,
  youtubeQuality,
  onTabChange,
  onPickFiles,
  onYoutubeUrlChange,
  onYoutubeQualityChange,
  onYoutubeDownload,
}: UploadPanelProps) {
  const tabIndex = activeTab === "local" ? 0 : 1;
  const tabIndicatorStyle = { ["--upload-tab-index" as string]: tabIndex } as CSSProperties;

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
                placeholder="粘贴 YouTube 链接，解析完成后右侧可选画质"
                value={youtubeUrl}
                onChange={(e) => onYoutubeUrlChange(e.target.value)}
                autoComplete="off"
              />
              <select
                className="quality-select"
                value={youtubeQuality}
                onChange={(e) => onYoutubeQualityChange(e.target.value)}
                disabled
              >
                <option value="">画质</option>
              </select>
            </div>

            <button className="download-button" onClick={onYoutubeDownload} disabled={!youtubeUrl.trim()}>
              <DownloadIcon />
              下载视频
            </button>
            <p className="youtube-note">下载流程后续接入</p>
          </div>
        </div>
      </div>
    </div>
  );
}
