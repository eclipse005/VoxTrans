import { useRef, useState } from "react";
import { FolderIcon, RefreshIcon, TrashIcon } from "./Icons";
import { useDialogA11y } from "./useDialogA11y";

type LogsModalProps = {
  visible: boolean;
  loading: boolean;
  taskName: string;
  content: string;
  channel: "main" | "llm";
  onChannelChange: (channel: "main" | "llm") => void;
  onClose: () => void;
  onRefresh: () => void | Promise<void>;
  onClear: () => void | Promise<void>;
  onOpenDir: () => void | Promise<void>;
};

export default function LogsModal({
  visible,
  loading,
  taskName,
  content,
  channel,
  onChannelChange,
  onClose,
  onRefresh,
  onClear,
  onOpenDir,
}: LogsModalProps) {
  const dialogRef = useDialogA11y(visible, onClose);
  const entries = parseLogEntries(content);
  const [collapsedMap, setCollapsedMap] = useState<Record<string, boolean>>({});
  const [isMaximized, setIsMaximized] = useState(false);
  const bodyRef = useRef<HTMLDivElement | null>(null);

  if (!visible) return null;

  return (
    <div className="modal-overlay">
      <div
        ref={dialogRef}
        className={`modal-content modal-content-logs ${isMaximized ? "modal-content-logs-maximized" : ""}`}
        role="dialog"
        aria-modal="true"
        aria-labelledby="logs-modal-title"
        tabIndex={-1}
      >
        <button
          className="modal-maximize"
          onClick={() => {
            setIsMaximized((prev) => {
              const next = !prev;
              if (next && dialogRef.current) {
                // Clear inline dimensions left by CSS resize so maximize styles can take effect.
                dialogRef.current.style.width = "";
                dialogRef.current.style.height = "";
              }
              return next;
            });
          }}
          aria-label={isMaximized ? "还原窗口大小" : "最大化窗口"}
          title={isMaximized ? "还原" : "最大化"}
        >
          {isMaximized ? "❐" : "□"}
        </button>
        <button className="modal-close" onClick={onClose} aria-label="关闭日志">×</button>
        <div className="logs-header">
          <div className="logs-title-block">
            <div className="logs-title-row">
              <h3 id="logs-modal-title" className="apple-heading-small">运行日志</h3>
              <div className="logs-channel-toggle" role="tablist" aria-label="日志频道">
                <button
                  type="button"
                  className={`logs-channel-btn ${channel === "main" ? "active" : ""}`}
                  onClick={() => onChannelChange("main")}
                >
                  MAIN
                </button>
                <button
                  type="button"
                  className={`logs-channel-btn ${channel === "llm" ? "active" : ""}`}
                  onClick={() => onChannelChange("llm")}
                >
                  LLM
                </button>
              </div>
              <div className="logs-actions">
                <button
                  className="file-list-icon-btn"
                  onClick={() => { void onOpenDir(); }}
                  disabled={loading}
                  title="打开日志目录"
                  aria-label="打开日志目录"
                >
                  <FolderIcon />
                </button>
                <button
                  className="file-list-icon-btn"
                  onClick={() => { void onRefresh(); }}
                  disabled={loading}
                  title="刷新"
                  aria-label="刷新日志"
                >
                  <RefreshIcon />
                </button>
                <button
                  className="file-list-icon-btn file-list-icon-btn-danger"
                  onClick={() => { void onClear(); }}
                  disabled={loading || content.trim().length === 0}
                  title="清空"
                  aria-label="清空日志"
                >
                  <TrashIcon />
                </button>
              </div>
            </div>
            <div className="logs-task-name" title={taskName}>{taskName}</div>
          </div>
        </div>

        <div
          ref={bodyRef}
          className="logs-body"
        >
          {loading ? <div className="logs-empty">加载中...</div> : null}
          {!loading && content.trim().length === 0 ? <div className="logs-empty">暂无日志</div> : null}
          {!loading && content.trim().length > 0 ? (
            <div className="logs-entries">
              {entries.map((entry, index) => {
                const entryKey = `${entry.timestamp}-${entry.event}-${index}`;
                const collapsed = collapsedMap[entryKey] ?? false;
                return (
                  <article key={entryKey} className="logs-entry">
                  <div className="logs-entry-head">
                    <span className="logs-entry-time">{entry.timestamp}</span>
                    <span className="logs-entry-event">{entry.event}</span>
                    <span className="logs-entry-spacer" />
                    <button
                      type="button"
                      className="logs-entry-toggle"
                      aria-label={collapsed ? "展开日志详情" : "收起日志详情"}
                      onClick={() => {
                        setCollapsedMap((prev) => ({ ...prev, [entryKey]: !collapsed }));
                      }}
                    >
                      {collapsed ? "▸" : "▾"}
                    </button>
                  </div>
                  {!collapsed && entry.payload != null ? (
                    <pre className="logs-json">{formatPayloadForDisplay(entry.payload)}</pre>
                  ) : !collapsed && entry.body ? (
                    <pre className="logs-text">{decodeVisibleEscapes(entry.body)}</pre>
                  ) : null}
                </article>
                );
              })}
            </div>
          ) : null}
          {!loading && content.trim().length > 0 ? (
            <button
              type="button"
              className="logs-jump-bottom"
              title="跳转到底部"
              aria-label="跳转到底部"
              onClick={() => {
                const node = bodyRef.current;
                if (!node) return;
                node.scrollTo({ top: node.scrollHeight, behavior: "smooth" });
              }}
            >
              ▼
            </button>
          ) : null}
        </div>
      </div>
    </div>
  );
}

type LogEntry = {
  timestamp: string;
  event: string;
  body: string;
  payload: unknown | null;
};

function parseLogEntries(content: string): LogEntry[] {
  const trimmed = content.trim();
  if (!trimmed) return [];
  const blocks = trimmed.split(/\n(?=\[\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}\]\s)/g);
  const out: LogEntry[] = [];
  for (const block of blocks) {
    const normalized = block.trim();
    if (!normalized) continue;
    const firstNewline = normalized.indexOf("\n");
    const header = firstNewline >= 0 ? normalized.slice(0, firstNewline).trim() : normalized;
    const body = firstNewline >= 0 ? normalized.slice(firstNewline + 1).trim() : "";
    const match = header.match(/^\[(.+?)\]\s(.+)$/);
    const timestamp = match?.[1] ?? "";
    const event = match?.[2] ?? header;
    let payload: unknown | null = null;
    if (body.startsWith("{") || body.startsWith("[")) {
      try {
        payload = JSON.parse(body);
      } catch {
        payload = null;
      }
    }
    out.push({
      timestamp,
      event,
      body,
      payload,
    });
  }
  return out;
}

function formatPayloadForDisplay(payload: unknown): string {
  return decodeVisibleEscapes(JSON.stringify(payload, null, 2));
}

function decodeVisibleEscapes(text: string): string {
  return text
    .replace(/\\r\\n/g, "\n")
    .replace(/\\n/g, "\n")
    .replace(/\\"/g, "\"")
    .replace(/\\\\/g, "\\");
}
