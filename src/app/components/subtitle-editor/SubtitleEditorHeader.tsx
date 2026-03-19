import { DownloadIcon, LogsIcon } from "../Icons";

type SubtitleEditorHeaderProps = {
  cueCount: number;
  taskName: string;
  onOpenSrtDir: () => void | Promise<void>;
  onExportSrt: () => void | Promise<void>;
  onOpenLogs: () => void | Promise<void>;
};

export default function SubtitleEditorHeader({
  cueCount,
  taskName,
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
        <div className="apple-body-small subtitle-editor-meta">
          <div className="subtitle-meta-row">
            <span className="subtitle-meta-label">任务:</span>
            <button
              type="button"
              className="subtitle-task-link"
              onClick={(e) => {
                e.stopPropagation();
                void onOpenSrtDir();
              }}
              aria-label="打开任务目录"
              title="打开任务目录"
            >
              <span title={taskName || "--"}>{taskName || "--"}</span>
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
