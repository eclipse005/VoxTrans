import type { TaskLogChannel } from "../../features/media/types";
import { useDialogA11y } from "./useDialogA11y";

type LogsModalProps = {
  visible: boolean;
  loading: boolean;
  taskName: string;
  activeChannel: TaskLogChannel;
  content: string;
  onClose: () => void;
  onRefresh: () => void | Promise<void>;
  onClear: () => void | Promise<void>;
  onChannelChange: (channel: TaskLogChannel) => void;
};

export default function LogsModal({
  visible,
  loading,
  taskName,
  activeChannel,
  content,
  onClose,
  onRefresh,
  onClear,
  onChannelChange,
}: LogsModalProps) {
  const dialogRef = useDialogA11y(visible, onClose);
  if (!visible) return null;

  return (
    <div className="modal-overlay">
      <div
        ref={dialogRef}
        className="modal-content modal-content-logs"
        role="dialog"
        aria-modal="true"
        aria-labelledby="logs-modal-title"
        tabIndex={-1}
      >
        <button className="modal-close" onClick={onClose} aria-label="关闭日志">×</button>
        <div className="logs-header">
          <div className="logs-title-block">
            <h3 id="logs-modal-title" className="apple-heading-small">运行日志</h3>
            <div className="logs-task-name" title={taskName}>{taskName}</div>
          </div>
          <div className="logs-actions">
            <button className="apple-button apple-button-secondary" onClick={() => { void onRefresh(); }} disabled={loading}>
              刷新
            </button>
            <button className="apple-button apple-button-secondary" onClick={() => { void onClear(); }} disabled={loading || content.trim().length === 0}>
              清空日志
            </button>
          </div>
        </div>

        <div className="logs-tabs" role="tablist" aria-label="日志文件切换">
          <button
            type="button"
            role="tab"
            aria-selected={activeChannel === "main"}
            className={`logs-tab-btn ${activeChannel === "main" ? "active" : ""}`}
            onClick={() => onChannelChange("main")}
          >
            主流程
          </button>
          <button
            type="button"
            role="tab"
            aria-selected={activeChannel === "llm"}
            className={`logs-tab-btn ${activeChannel === "llm" ? "active" : ""}`}
            onClick={() => onChannelChange("llm")}
          >
            LLM 交互
          </button>
        </div>

        <div className="logs-body">
          {loading ? <div className="logs-empty">加载中...</div> : null}
          {!loading && content.trim().length === 0 ? <div className="logs-empty">暂无日志</div> : null}
          {!loading && content.trim().length > 0 ? (
            <pre className="logs-text">{content}</pre>
          ) : null}
        </div>
      </div>
    </div>
  );
}
