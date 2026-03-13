import { useState } from "react";
import type { TaskLogChannel, TaskLlmUsageSummary } from "../../features/media/types";
import { useDialogA11y } from "./useDialogA11y";

type LogsModalProps = {
  visible: boolean;
  loading: boolean;
  taskName: string;
  activeChannel: TaskLogChannel;
  content: string;
  usageSummary: TaskLlmUsageSummary | null;
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
  usageSummary,
  onClose,
  onRefresh,
  onClear,
  onChannelChange,
}: LogsModalProps) {
  const dialogRef = useDialogA11y(visible, onClose);
  const entries = parseLogEntries(content);
  const [collapsedMap, setCollapsedMap] = useState<Record<string, boolean>>({});
  const [isMaximized, setIsMaximized] = useState(false);
  const usageBuckets = [...(usageSummary?.buckets ?? [])].sort((a, b) => a.updatedAt - b.updatedAt);
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

        <div className="logs-usage-card">
          <div className="logs-usage-row">
            <span className="logs-usage-label">总 Tokens</span>
            <span className="logs-usage-value">{formatNumber(usageSummary?.totalTokens ?? 0)}</span>
          </div>
          <div className="logs-usage-row">
            <span className="logs-usage-label">输入</span>
            <span className="logs-usage-value">{formatNumber(usageSummary?.promptTokens ?? 0)}</span>
          </div>
          <div className="logs-usage-row">
            <span className="logs-usage-label">输出</span>
            <span className="logs-usage-value">{formatNumber(usageSummary?.completionTokens ?? 0)}</span>
          </div>
          <div className="logs-usage-stages">
            {usageBuckets.length === 0 ? (
              <span className="logs-usage-stage-empty">暂无阶段 Token 记录</span>
            ) : (
              usageBuckets.map((bucket) => (
                <span key={bucket.stage} className="logs-usage-stage">
                  {toStageLabel(bucket.stage)}: {formatNumber(bucket.totalTokens)}
                </span>
              ))
            )}
          </div>
        </div>

        <div className="logs-body">
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
        </div>
      </div>
    </div>
  );
}

function formatNumber(value: number): string {
  return Math.max(0, value || 0).toLocaleString();
}

function toStageLabel(stage: string): string {
  if (stage === "hotword") return "热词矫正";
  if (stage === "punctuation") return "标点恢复";
  if (stage === "summary") return "总结";
  if (stage === "translate") return "翻译";
  return stage;
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
