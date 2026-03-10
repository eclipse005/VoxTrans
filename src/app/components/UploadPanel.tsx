import type { UploadTab } from "../types";
import { DownloadIcon, FileIcon, UploadIcon, UploadIconLarge, YoutubeIcon, YoutubeIconLarge } from "./Icons";

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
  return (
    <div className="apple-animate-on-scroll apple-delay-100 upload-section animated">
      <div className="upload-tabs">
        <button className={`tab-button ${activeTab === "local" ? "active" : ""}`} onClick={() => onTabChange("local")}>
          <UploadIcon />
          本地文件
        </button>
        <button className={`tab-button ${activeTab === "youtube" ? "active" : ""}`} onClick={() => onTabChange("youtube")}>
          <YoutubeIcon />
          YouTube
        </button>
      </div>

      {activeTab === "local" ? (
        <div
          className={`upload-area ${dragActive ? "drag-over" : ""}`}
          role="button"
          tabIndex={0}
          onClick={onPickFiles}
          onKeyDown={(e) => {
            if (e.key === "Enter" || e.key === " ") {
              e.preventDefault();
              void onPickFiles();
            }
          }}
        >
          <div className="upload-content">
            <div className="upload-icon">
              <UploadIconLarge />
            </div>
            <div className="upload-text">
              <h3 className="upload-title">拖拽上传音视频文件</h3>
              <p className="upload-hint">支持 .mp3 .wav .m4a .mp4 .webm .mkv .avi .mov 等格式</p>
            </div>
            <div className="upload-footer">
              <FileIcon />
              <span>支持多文件上传（也可点击上传）</span>
            </div>
          </div>
        </div>
      ) : (
        <div className="youtube-download-area">
          <div className="youtube-icon">
            <YoutubeIconLarge />
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

          <div className="download-progress">
            <div className="progress-info">
              <span>准备下载...</span>
              <span>0%</span>
            </div>
            <div className="progress-bar-bg">
              <div className="progress-bar-fill" style={{ width: "0%" }} />
            </div>
            <div className="progress-details">
              <span>-- MB/s</span>
              <span>剩余 --:--</span>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
