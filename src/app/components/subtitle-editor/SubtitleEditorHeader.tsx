import { DownloadIcon, LogsIcon } from "../Icons";

type SubtitleEditorHeaderProps = {
  cueCount: number;
  taskName: string;
  srtPath: string;
  onOpenSrtDir: () => void | Promise<void>;
  onExportSrt: () => void | Promise<void>;
  onOpenLogs: () => void | Promise<void>;
};

export default function SubtitleEditorHeader({
  cueCount,
  taskName,
  srtPath,
  onOpenSrtDir,
  onExportSrt,
  onOpenLogs,
}: SubtitleEditorHeaderProps) {
  return (
    <div className="subtitle-editor-header">
      <div className="subtitle-header-main">
        <div className="subtitle-title-row">
          <h3 className="apple-heading-small">字幕编辑器</h3>
          <span className="subtitle-count-badge">{cueCount} 条</span>
        </div>
        <div className="apple-body-small subtitle-editor-meta" title={`任务: ${taskName} · 输出: ${srtPath || "--"}`}>
          <div className="subtitle-meta-row">
            <span className="subtitle-meta-label">任务:</span>
            <span className="subtitle-meta-value">{taskName || "--"}</span>
          </div>
          <div className="subtitle-meta-row">
            <span className="subtitle-meta-label">输出:</span>
            <button
              type="button"
              className="subtitle-output-link"
              onClick={(e) => {
                e.stopPropagation();
                void onOpenSrtDir();
              }}
              aria-label={srtPath ? "打开字幕输出目录" : "打开输出目录"}
              title={srtPath ? "打开字幕输出目录" : "打开输出目录"}
            >
              {srtPath || "--"}
            </button>
          </div>
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
          aria-label="导出 SRT"
          title="导出"
        >
          <DownloadIcon />
          <span>导出</span>
        </button>
        <button
          type="button"
          className="subtitle-header-icon-btn"
          onClick={(e) => {
            e.stopPropagation();
            void onOpenLogs();
          }}
          aria-label="打开任务日志"
          title="日志"
        >
          <LogsIcon />
          <span>日志</span>
        </button>
      </div>
    </div>
  );
}
