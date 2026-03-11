import type { TaskEventRecord } from "../../features/media/types";
import { useDialogA11y } from "./useDialogA11y";

type LogsModalProps = {
  visible: boolean;
  loading: boolean;
  events: TaskEventRecord[];
  onClose: () => void;
  onRefresh: () => void | Promise<void>;
  onClear: () => void | Promise<void>;
};

function formatEventTime(createdAt: number): string {
  const tsMs = createdAt > 1_000_000_000_000 ? createdAt : createdAt * 1000;
  return new Date(tsMs).toLocaleString();
}

export default function LogsModal({ visible, loading, events, onClose, onRefresh, onClear }: LogsModalProps) {
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
          <h3 id="logs-modal-title" className="apple-heading-small">运行日志</h3>
          <div className="logs-actions">
            <button className="apple-button apple-button-secondary" onClick={() => { void onRefresh(); }} disabled={loading}>
              刷新
            </button>
            <button className="apple-button apple-button-secondary" onClick={() => { void onClear(); }} disabled={loading || events.length === 0}>
              清空日志
            </button>
          </div>
        </div>

        <div className="logs-body">
          {loading ? <div className="logs-empty">加载中...</div> : null}
          {!loading && events.length === 0 ? <div className="logs-empty">暂无日志</div> : null}
          {!loading && events.length > 0 ? (
            <div className="logs-list">
              {events.map((event) => (
                <article key={event.id} className="logs-item">
                  <div className="logs-item-head">
                    <span className="logs-event-type">{event.eventType}</span>
                    <span className="logs-time">{formatEventTime(event.createdAt)}</span>
                  </div>
                  <div className="logs-item-meta">任务: {event.taskId || "全局"}</div>
                  <pre className="logs-payload">{JSON.stringify(event.payload ?? {}, null, 2)}</pre>
                </article>
              ))}
            </div>
          ) : null}
        </div>
      </div>
    </div>
  );
}

