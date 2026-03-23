import { useEffect, useRef, useState } from "react";
import type { MutableRefObject, ReactNode } from "react";
import { ChevronLeftIcon, ChevronRightIcon, FolderIcon, RefreshIcon, TrashIcon } from "./Icons";
import { useDialogA11y } from "./useDialogA11y";

type LogsModalProps = {
  visible: boolean;
  loading: boolean;
  totalTokens: number;
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
  totalTokens,
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
  const [searchText, setSearchText] = useState("");
  const [activeMatchIndex, setActiveMatchIndex] = useState(0);
  const bodyRef = useRef<HTMLDivElement | null>(null);
  const matchRefs = useRef<Array<HTMLElement | null>>([]);

  const normalizedQuery = searchText.trim().toLowerCase();
  const viewEntries = buildViewEntries(entries, normalizedQuery);
  const matchCount = viewEntries.reduce(
    (sum, entry) => sum + entry.timestampRanges.length + entry.eventRanges.length + entry.contentRanges.length,
    0,
  );
  const currentMatchIndex = matchCount === 0
    ? -1
    : (activeMatchIndex >= 0 && activeMatchIndex < matchCount ? activeMatchIndex : 0);

  useEffect(() => {
    matchRefs.current = [];
  }, [content, channel]);

  useEffect(() => {
    if (currentMatchIndex < 0) return;
    const target = matchRefs.current[currentMatchIndex];
    if (!target) return;
    target.scrollIntoView({ block: "center", behavior: "smooth" });
  }, [currentMatchIndex, normalizedQuery, content, channel]);

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
              <div className="logs-usage-stage">
                Tokens: {formatNumber(totalTokens)}
              </div>
              <div className="logs-search" role="search" aria-label="日志查找">
                <input
                  className="apple-input logs-search-input"
                  value={searchText}
                  onChange={(event) => {
                    setSearchText(event.target.value);
                    setActiveMatchIndex(0);
                  }}
                  placeholder="查找日志"
                  aria-label="查找日志"
                  onKeyDown={(event) => {
                    if (event.key !== "Enter") return;
                    event.preventDefault();
                    if (matchCount === 0) return;
                    setActiveMatchIndex((prev) => {
                      if (event.shiftKey) return prev <= 0 ? matchCount - 1 : prev - 1;
                      return prev >= matchCount - 1 ? 0 : prev + 1;
                    });
                  }}
                />
                <div className="logs-search-nav" role="group" aria-label="日志查找导航">
                  <button
                    type="button"
                    className="logs-search-btn"
                    onClick={() => {
                      if (matchCount === 0) return;
                      setActiveMatchIndex((prev) => (prev <= 0 ? matchCount - 1 : prev - 1));
                    }}
                    disabled={matchCount === 0}
                    aria-label="上一条匹配"
                    title="上一条匹配"
                  >
                    <ChevronLeftIcon />
                  </button>
                  <span className="logs-search-count" aria-live="polite">
                    {matchCount === 0 ? "0/0" : `${currentMatchIndex + 1}/${matchCount}`}
                  </span>
                  <button
                    type="button"
                    className="logs-search-btn"
                    onClick={() => {
                      if (matchCount === 0) return;
                      setActiveMatchIndex((prev) => (prev >= matchCount - 1 ? 0 : prev + 1));
                    }}
                    disabled={matchCount === 0}
                    aria-label="下一条匹配"
                    title="下一条匹配"
                  >
                    <ChevronRightIcon />
                  </button>
                </div>
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
              {viewEntries.map((entry, index) => {
                const entryKey = `${entry.timestamp}-${entry.event}-${index}`;
                const entryMatchTotal = entry.timestampRanges.length + entry.eventRanges.length + entry.contentRanges.length;
                const collapsed = normalizedQuery && entryMatchTotal > 0
                  ? false
                  : (collapsedMap[entryKey] ?? false);
                return (
                  <article key={entryKey} className="logs-entry">
                  <div className="logs-entry-head">
                    <span className="logs-entry-time">
                      {renderHighlightedText({
                        text: entry.timestamp,
                        ranges: entry.timestampRanges,
                        matchRefs,
                        activeMatchIndex: currentMatchIndex,
                      })}
                    </span>
                    <span className="logs-entry-event">
                      {renderHighlightedText({
                        text: entry.event,
                        ranges: entry.eventRanges,
                        matchRefs,
                        activeMatchIndex: currentMatchIndex,
                      })}
                    </span>
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
                    <pre className="logs-json">
                      {renderHighlightedText({
                        text: entry.displayText,
                        ranges: entry.contentRanges,
                        matchRefs,
                        activeMatchIndex: currentMatchIndex,
                      })}
                    </pre>
                  ) : !collapsed && entry.body ? (
                    <pre className="logs-text">
                      {renderHighlightedText({
                        text: entry.displayText,
                        ranges: entry.contentRanges,
                        matchRefs,
                        activeMatchIndex: currentMatchIndex,
                      })}
                    </pre>
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

type MatchRange = {
  start: number;
  end: number;
  globalIndex: number;
};

type HighlightRenderArgs = {
  text: string;
  ranges: MatchRange[];
  matchRefs: MutableRefObject<Array<HTMLElement | null>>;
  activeMatchIndex: number;
};

type LogEntry = {
  timestamp: string;
  event: string;
  body: string;
  payload: unknown | null;
};

type ViewLogEntry = LogEntry & {
  displayText: string;
  timestampRanges: MatchRange[];
  eventRanges: MatchRange[];
  contentRanges: MatchRange[];
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

function formatNumber(value: number): string {
  if (!Number.isFinite(value) || value <= 0) return "0";
  return new Intl.NumberFormat("en-US").format(Math.floor(value));
}

function buildViewEntries(entries: LogEntry[], normalizedQuery: string): ViewLogEntry[] {
  let nextMatchIndex = 0;
  return entries.map((entry) => {
    const displayText = entry.payload != null
      ? formatPayloadForDisplay(entry.payload)
      : entry.body
        ? decodeVisibleEscapes(entry.body)
        : "";
    const timestampRanges = findMatchRanges(entry.timestamp, normalizedQuery, nextMatchIndex);
    nextMatchIndex += timestampRanges.length;
    const eventRanges = findMatchRanges(entry.event, normalizedQuery, nextMatchIndex);
    nextMatchIndex += eventRanges.length;
    const contentRanges = findMatchRanges(displayText, normalizedQuery, nextMatchIndex);
    nextMatchIndex += contentRanges.length;
    return {
      ...entry,
      displayText,
      timestampRanges,
      eventRanges,
      contentRanges,
    };
  });
}

function findMatchRanges(text: string, normalizedQuery: string, baseIndex = 0): MatchRange[] {
  if (!normalizedQuery || !text) return [];
  const lowerText = text.toLowerCase();
  const ranges: MatchRange[] = [];
  let searchFrom = 0;
  while (searchFrom < lowerText.length) {
    const foundAt = lowerText.indexOf(normalizedQuery, searchFrom);
    if (foundAt < 0) break;
    ranges.push({
      start: foundAt,
      end: foundAt + normalizedQuery.length,
      globalIndex: baseIndex + ranges.length,
    });
    searchFrom = foundAt + normalizedQuery.length;
  }
  return ranges;
}

function renderHighlightedText({
  text,
  ranges,
  matchRefs,
  activeMatchIndex,
}: HighlightRenderArgs): ReactNode {
  if (!ranges.length) return text;
  const parts: ReactNode[] = [];
  let cursor = 0;
  for (const range of ranges) {
    if (cursor < range.start) {
      parts.push(text.slice(cursor, range.start));
    }
    const isActive = range.globalIndex === activeMatchIndex;
    parts.push(
      <mark
        key={`${range.start}-${range.end}-${range.globalIndex}`}
        ref={(node) => {
          matchRefs.current[range.globalIndex] = node;
        }}
        className={`logs-search-hit ${isActive ? "active" : ""}`}
      >
        {text.slice(range.start, range.end)}
      </mark>,
    );
    cursor = range.end;
  }
  if (cursor < text.length) {
    parts.push(text.slice(cursor));
  }
  return parts;
}
