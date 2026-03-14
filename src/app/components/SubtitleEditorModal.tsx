import { type MouseEvent, type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import type { SubtitleCue } from "../../features/media/types";
import { formatSrtTime, parseSrtTime } from "../../features/media/srt";
import type { SubtitleSaveState } from "../types";
import { AlertIcon, ChevronDownIcon, ChevronLeftIcon, ChevronRightIcon, EditIcon, LogsIcon, ReplaceIcon, TrashIcon } from "./Icons";

type SubtitleEditorModalProps = {
  visible: boolean;
  embedded?: boolean;
  taskName: string;
  srtPath: string;
  cues: SubtitleCue[];
  cueWarningsById: Record<string, string[]>;
  saveState: SubtitleSaveState;
  onUpdateCue: (cueId: string, patch: Partial<SubtitleCue>) => void;
  onAddCueAfter: (selectedCueId: string | null) => void;
  onMergeSelected: (selectedCueIds: string[]) => void;
  onSplitSelected: (selectedCueIds: string[]) => Array<{ sourceCueId: string; bornCueId: string }>;
  onReplaceText: (findText: string, replaceText: string, scopeCueIds: string[] | null, maxReplacements?: number) => number;
  onDeleteCue: (cueId: string) => void;
  onOpenSrtDir: () => void | Promise<void>;
  onOpenLogs: () => void | Promise<void>;
  onClose: () => void | Promise<void>;
};

function saveStateLabel(state: SubtitleSaveState): string {
  if (state === "saving") return "自动保存中...";
  if (state === "saved") return "已自动保存";
  if (state === "error") return "保存失败";
  return "等待编辑";
}

export default function SubtitleEditorModal({
  visible,
  embedded = false,
  taskName,
  srtPath,
  cues,
  cueWarningsById,
  saveState,
  onUpdateCue,
  onAddCueAfter,
  onMergeSelected,
  onSplitSelected,
  onReplaceText,
  onDeleteCue,
  onOpenSrtDir,
  onOpenLogs,
  onClose,
}: SubtitleEditorModalProps) {
  const [timeErrorByCue, setTimeErrorByCue] = useState<Record<string, string>>({});
  const [selectedCueIds, setSelectedCueIds] = useState<string[]>([]);
  const [anchorCueId, setAnchorCueId] = useState<string>("");
  const [editingCueId, setEditingCueId] = useState<string>("");
  const [findText, setFindText] = useState("");
  const [replaceText, setReplaceText] = useState("");
  const [findStatus, setFindStatus] = useState("");
  const [findCursor, setFindCursor] = useState(0);
  const [isReplaceMenuOpen, setIsReplaceMenuOpen] = useState(false);
  const [isBatchAnimating, setIsBatchAnimating] = useState(false);
  const batchTimerRef = useRef<number | null>(null);
  const scrollAnimRafRef = useRef<number | null>(null);
  const listContainerRef = useRef<HTMLDivElement | null>(null);
  const replaceMenuRef = useRef<HTMLDivElement | null>(null);
  const pendingSplitRef = useRef<Array<{ bornCueId: string; fromRect: DOMRect }>>([]);
  const pendingLayoutFromRectsRef = useRef<Map<string, DOMRect> | null>(null);
  const cardRefs = useRef<Record<string, HTMLElement | null>>({});

  const cueIds = useMemo(() => cues.map((cue) => cue.id), [cues]);
  const findKeyword = useMemo(() => findText.trim().toLowerCase(), [findText]);
  const matchCueIndexes = useMemo(() => {
    if (!findKeyword) return [] as number[];
    const indexes: number[] = [];
    for (let idx = 0; idx < cues.length; idx += 1) {
      const cue = cues[idx];
      if (!cue) continue;
      const seq = String(idx + 1);
      const start = formatSrtTime(cue.startMs);
      const end = formatSrtTime(cue.endMs);
      const range = `${start} --> ${end}`;
      const haystack = [seq, `#${seq}`, start, end, range, cue.text, cue.translatedText]
        .join(" ")
        .toLowerCase();
      if (haystack.includes(findKeyword)) {
        indexes.push(idx);
      }
    }
    return indexes;
  }, [cues, findKeyword]);
  const matchCueIdToCursor = useMemo(() => {
    const map = new Map<string, number>();
    for (let cursor = 0; cursor < matchCueIndexes.length; cursor += 1) {
      const cue = cues[matchCueIndexes[cursor]];
      if (!cue) continue;
      map.set(cue.id, cursor);
    }
    return map;
  }, [cues, matchCueIndexes]);
  const currentMatch = useMemo(() => {
    if (!findKeyword || matchCueIndexes.length === 0) return null;
    const cursor = Math.min(Math.max(findCursor, 0), matchCueIndexes.length - 1);
    const cueIndex = matchCueIndexes[cursor];
    const cue = cues[cueIndex];
    if (!cue) return null;
    return { cueId: cue.id, cueIndex, cursor };
  }, [cues, findCursor, findKeyword, matchCueIndexes]);
  const findCounterLabel = useMemo(() => {
    if (!findKeyword || matchCueIndexes.length === 0) return "0/0";
    const cursor = currentMatch ? currentMatch.cursor + 1 : 1;
    return `${cursor}/${matchCueIndexes.length}`;
  }, [currentMatch, findKeyword, matchCueIndexes.length]);
  const findStatusLabel = useMemo(() => {
    if (!findKeyword) return findStatus;
    if (matchCueIndexes.length === 0) return findStatus || "无匹配";
    return findStatus;
  }, [findKeyword, findStatus, matchCueIndexes.length]);
  const validSelectedCueIds = useMemo(() => {
    return selectedCueIds.filter((id) => cueIds.includes(id));
  }, [cueIds, selectedCueIds]);
  const primarySelectedCueId = validSelectedCueIds[0] ?? null;
  const renderHighlightedText = (text: string, fallback: string, cueId: string): ReactNode => {
    if (!text) return fallback;
    if (!findKeyword) return text;

    const lower = text.toLowerCase();
    const parts: ReactNode[] = [];
    let cursor = 0;
    let partIndex = 0;

    while (cursor < text.length) {
      const index = lower.indexOf(findKeyword, cursor);
      if (index < 0) break;
      if (index > cursor) {
        parts.push(text.slice(cursor, index));
      }
      const match = text.slice(index, index + findKeyword.length);
      parts.push(
        <mark
          key={`${cueId}-${partIndex}`}
          className={`subtitle-inline-hit ${currentMatch?.cueId === cueId ? "current" : ""}`}
        >
          {match}
        </mark>,
      );
      partIndex += 1;
      cursor = index + findKeyword.length;
    }

    if (parts.length === 0) return text;
    if (cursor < text.length) {
      parts.push(text.slice(cursor));
    }
    return parts;
  };

  const scrollToCueWithFixedDuration = (cueId: string, durationMs = 260) => {
    const container = listContainerRef.current;
    const node = cardRefs.current[cueId];
    if (!container || !node) return;

    if (scrollAnimRafRef.current != null) {
      window.cancelAnimationFrame(scrollAnimRafRef.current);
      scrollAnimRafRef.current = null;
    }

    const containerRect = container.getBoundingClientRect();
    const nodeRect = node.getBoundingClientRect();
    const nodeTopInContainer = nodeRect.top - containerRect.top + container.scrollTop;
    const targetCenterTop = nodeTopInContainer - (container.clientHeight - nodeRect.height) / 2;
    const maxTop = Math.max(container.scrollHeight - container.clientHeight, 0);
    const targetTop = Math.min(Math.max(targetCenterTop, 0), maxTop);
    const startTop = container.scrollTop;
    const delta = targetTop - startTop;
    if (Math.abs(delta) < 0.5) return;

    const start = performance.now();
    const easeOutCubic = (t: number) => 1 - (1 - t) * (1 - t) * (1 - t);

    const step = (now: number) => {
      const progress = Math.min((now - start) / durationMs, 1);
      container.scrollTop = startTop + delta * easeOutCubic(progress);
      if (progress < 1) {
        scrollAnimRafRef.current = window.requestAnimationFrame(step);
      } else {
        scrollAnimRafRef.current = null;
      }
    };

    scrollAnimRafRef.current = window.requestAnimationFrame(step);
  };

  useEffect(() => {
    if (!currentMatch) return;
    window.requestAnimationFrame(() => {
      scrollToCueWithFixedDuration(currentMatch.cueId);
    });
  }, [currentMatch]);

  useEffect(() => {
    return () => {
      if (batchTimerRef.current != null) {
        window.clearTimeout(batchTimerRef.current);
      }
      if (scrollAnimRafRef.current != null) {
        window.cancelAnimationFrame(scrollAnimRafRef.current);
        scrollAnimRafRef.current = null;
      }
    };
  }, []);

  useEffect(() => {
    if (!isReplaceMenuOpen) return;
    const onPointerDown = (event: PointerEvent) => {
      const target = event.target as Node | null;
      if (!target) return;
      if (replaceMenuRef.current?.contains(target)) return;
      setIsReplaceMenuOpen(false);
    };
    window.addEventListener("pointerdown", onPointerDown);
    return () => {
      window.removeEventListener("pointerdown", onPointerDown);
    };
  }, [isReplaceMenuOpen]);

  useEffect(() => {
    const layoutFromRects = pendingLayoutFromRectsRef.current;
    if (!layoutFromRects || layoutFromRects.size === 0) return;
    pendingLayoutFromRectsRef.current = null;

    let raf = 0;
    raf = window.requestAnimationFrame(() => {
      let hasLayoutAnimation = false;
      for (const cue of cues) {
        const node = cardRefs.current[cue.id];
        const fromRect = layoutFromRects.get(cue.id);
        if (!node || !fromRect) continue;

        const toRect = node.getBoundingClientRect();
        const dx = fromRect.left - toRect.left;
        const dy = fromRect.top - toRect.top;
        if (Math.abs(dx) < 0.5 && Math.abs(dy) < 0.5) continue;

        node.animate(
          [
            { transform: `translate(${dx}px, ${dy}px)` },
            { transform: "translate(0, 0)" },
          ],
          {
            duration: 340,
            easing: "cubic-bezier(0.22, 1, 0.36, 1)",
            fill: "both",
          },
        );
        hasLayoutAnimation = true;
      }

      if (batchTimerRef.current != null) {
        window.clearTimeout(batchTimerRef.current);
      }
      batchTimerRef.current = window.setTimeout(() => {
        setIsBatchAnimating(false);
        batchTimerRef.current = null;
      }, hasLayoutAnimation ? 360 : 0);
    });

    return () => {
      window.cancelAnimationFrame(raf);
    };
  }, [cues]);

  useEffect(() => {
    const pending = pendingSplitRef.current;
    if (pending.length === 0) return;
    pendingSplitRef.current = [];

    let raf1 = 0;
    let raf2 = 0;
    raf1 = window.requestAnimationFrame(() => {
      raf2 = window.requestAnimationFrame(() => {
        let hasAnimation = false;

        for (const item of pending) {
          const node = cardRefs.current[item.bornCueId];
          if (!node) continue;
          const targetRect = node.getBoundingClientRect();
          const dx = item.fromRect.left - targetRect.left;
          const dy = item.fromRect.top - targetRect.top;

          node.animate(
            [
              {
                transform: `translate(${dx}px, ${dy}px) scale(1, 0.62)`,
                opacity: 0.14,
                filter: "saturate(0.86)",
              },
              {
                transform: "translate(0, 3px) scale(1, 1.02)",
                opacity: 0.95,
                filter: "saturate(1.02)",
                offset: 0.8,
              },
              {
                transform: "translate(0, 0) scale(1, 1)",
                opacity: 1,
                filter: "saturate(1)",
              },
            ],
            {
              duration: 520,
              easing: "cubic-bezier(0.22, 1, 0.36, 1)",
              fill: "both",
            },
          );
          hasAnimation = true;
        }

        if (batchTimerRef.current != null) {
          window.clearTimeout(batchTimerRef.current);
        }
        batchTimerRef.current = window.setTimeout(() => {
          setIsBatchAnimating(false);
          batchTimerRef.current = null;
        }, hasAnimation ? 540 : 0);
      });
    });

    return () => {
      window.cancelAnimationFrame(raf1);
      window.cancelAnimationFrame(raf2);
    };
  }, [cues]);

  if (!visible) return null;

  const content = (
    <div className={embedded ? "subtitle-inline-content" : "modal-content modal-content-subtitle"} onClick={handleContainerClick}>
      {!embedded ? (
        <button className="modal-close" onClick={() => { void onClose(); }} aria-label="关闭">
          ×
        </button>
      ) : null}

      <div className="subtitle-editor-header">
        <div className="subtitle-header-main">
          <div className="subtitle-title-row">
            <h3 className="apple-heading-small">字幕编辑器</h3>
            <span className="subtitle-count-badge">{cues.length} 条</span>
            <span className={`subtitle-save-indicator subtitle-save-${saveState}`}>{saveStateLabel(saveState)}</span>
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

      <div className="subtitle-editor-topbar">
        <div className="subtitle-toolbar-shell">
          <div className="subtitle-find-block subtitle-find-replace-inline">
            <div className="subtitle-find-replace subtitle-find-shell">
            <input
              className="apple-input subtitle-find-input"
              value={findText}
              onChange={(e) => {
                setFindText(e.target.value);
                setFindStatus("");
              }}
              placeholder="查找文本"
            />
            <input
              className="apple-input subtitle-find-input"
              value={replaceText}
              onChange={(e) => setReplaceText(e.target.value)}
              placeholder="替换为"
            />
            <div className="subtitle-find-nav" role="group" aria-label="查找匹配导航">
              <button
                className="subtitle-find-nav-btn"
                type="button"
                onClick={handlePrevMatch}
                disabled={!findKeyword || matchCueIndexes.length === 0}
                aria-label="上一条匹配"
                title="上一条匹配"
              >
                <ChevronLeftIcon />
              </button>
              <span className="subtitle-find-count" aria-live="polite">{findCounterLabel}</span>
              <button
                className="subtitle-find-nav-btn"
                type="button"
                onClick={handleNextMatch}
                disabled={!findKeyword || matchCueIndexes.length === 0}
                aria-label="下一条匹配"
                title="下一条匹配"
              >
                <ChevronRightIcon />
              </button>
            </div>
            <div className="subtitle-find-split" ref={replaceMenuRef}>
              <button
                className="apple-button apple-button-secondary subtitle-find-text-btn subtitle-find-primary-btn subtitle-find-split-main"
                onClick={handleReplaceOne}
                title="替换当前命中并跳到下一条"
                aria-label="替换当前命中并跳到下一条"
                disabled={!findKeyword}
              >
                替换
              </button>
              <button
                className="apple-button apple-button-secondary subtitle-find-text-btn subtitle-find-split-toggle"
                type="button"
                onClick={() => setIsReplaceMenuOpen((old) => !old)}
                aria-label="打开替换菜单"
                aria-expanded={isReplaceMenuOpen}
                disabled={!findKeyword}
              >
                <ChevronDownIcon />
              </button>
              {isReplaceMenuOpen ? (
                <div className="subtitle-find-split-menu" role="menu" aria-label="替换菜单">
                  <button
                    type="button"
                    className="subtitle-find-split-menu-item"
                    role="menuitem"
                    onClick={handleReplaceAllFromMenu}
                  >
                    <ReplaceIcon />
                    全部替换
                  </button>
                </div>
              ) : null}
            </div>
              {findStatusLabel ? <span className="subtitle-find-status">{findStatusLabel}</span> : null}
            </div>
          </div>

          <div className="subtitle-row-actions subtitle-batch-actions">
            <button
              className="apple-button apple-button-secondary subtitle-mini-btn"
              onClick={(e) => {
                e.stopPropagation();
                onAddCueAfter(primarySelectedCueId);
              }}
              disabled={isBatchAnimating}
            >
              新增字幕段
            </button>
            <button
              className="apple-button apple-button-secondary subtitle-mini-btn"
              disabled={validSelectedCueIds.length < 2 || isBatchAnimating}
              onClick={(e) => {
                e.stopPropagation();
                onMergeSelected(validSelectedCueIds);
              }}
              title={validSelectedCueIds.length >= 2 ? `合并 ${validSelectedCueIds.length} 条` : "请选择至少两条字幕"}
            >
              {validSelectedCueIds.length >= 2 ? `合并(${validSelectedCueIds.length})` : "合并"}
            </button>
            <button
              className="apple-button apple-button-secondary subtitle-mini-btn"
              disabled={validSelectedCueIds.length < 1 || isBatchAnimating}
              onClick={(e) => {
                e.stopPropagation();
                const orderedIds = [...validSelectedCueIds].sort((a, b) => cueIds.indexOf(a) - cueIds.indexOf(b));
                const sourceRectByCueId = new Map<string, DOMRect>();
                for (const cueId of orderedIds) {
                  const node = cardRefs.current[cueId];
                  if (!node) continue;
                  sourceRectByCueId.set(cueId, node.getBoundingClientRect());
                }

                const splitResult = onSplitSelected(orderedIds);
                const pending = splitResult
                  .map((item) => {
                    const fromRect = sourceRectByCueId.get(item.sourceCueId);
                    if (!fromRect) return null;
                    return { bornCueId: item.bornCueId, fromRect };
                  })
                  .filter((item): item is { bornCueId: string; fromRect: DOMRect } => item !== null);

                if (pending.length > 0) {
                  setIsBatchAnimating(true);
                  pendingSplitRef.current = pending;
                }
              }}
              title={validSelectedCueIds.length >= 1 ? `拆分 ${validSelectedCueIds.length} 条` : "请选择字幕"}
            >
              {validSelectedCueIds.length >= 1 ? `拆分(${validSelectedCueIds.length})` : "拆分"}
            </button>
          </div>
        </div>
      </div>

      <div
        ref={listContainerRef}
        className="subtitle-all-editor"
        onClick={(event) => {
          if (event.target === event.currentTarget) {
            setSelectedCueIds([]);
            setAnchorCueId("");
          }
        }}
      >
        {cues.length === 0 ? (
          <div className="subtitle-cue-empty">暂无字幕段，点击上方“新增字幕段”开始编辑。</div>
        ) : (
          cues.map((cue, idx) => (
            <article
              key={cue.id}
              ref={(node) => {
                cardRefs.current[cue.id] = node;
              }}
              className={`subtitle-row-card ${validSelectedCueIds.includes(cue.id) ? "selected" : ""}`}
              onClick={(event) => handleCueClick(cue.id, event)}
            >
              <div className="subtitle-row-head">
                <div className="subtitle-row-head-main">
                  <span className="subtitle-row-index">{renderHighlightedText(`#${idx + 1}`, `#${idx + 1}`, cue.id)}</span>
                  <span className="subtitle-row-time">{renderHighlightedText(formatSrtTime(cue.startMs), formatSrtTime(cue.startMs), cue.id)}</span>
                  <span className="subtitle-time-arrow">→</span>
                  <span className="subtitle-row-time">{renderHighlightedText(formatSrtTime(cue.endMs), formatSrtTime(cue.endMs), cue.id)}</span>
                </div>
                <div className="subtitle-row-actions">
                  {(cueWarningsById[cue.id]?.length ?? 0) > 0 ? (
                    <span
                      className="subtitle-warning-badge"
                      title={cueWarningsById[cue.id].join("\n")}
                      aria-label={`该字幕存在 ${cueWarningsById[cue.id].length} 条格式问题`}
                    >
                      <AlertIcon />
                    </span>
                  ) : null}
                  <button
                    className="subtitle-icon-btn"
                    title={editingCueId === cue.id ? "收起编辑" : "编辑字幕"}
                    onClick={(e) => {
                      e.stopPropagation();
                      setSelectedCueIds([cue.id]);
                      setAnchorCueId(cue.id);
                      setEditingCueId((old) => (old === cue.id ? "" : cue.id));
                    }}
                  >
                    <EditIcon />
                  </button>
                  <button
                    className="subtitle-icon-btn subtitle-icon-btn-danger"
                    title="删除字幕段"
                    onClick={(e) => {
                      e.stopPropagation();
                      onDeleteCue(cue.id);
                    }}
                    disabled={cues.length <= 1}
                  >
                    <TrashIcon />
                  </button>
                </div>
              </div>

              <div className="subtitle-row-summary">
                <span className="subtitle-row-text-preview" title={cue.text || "(空文本)"}>
                  <span className="subtitle-row-text-value">{renderHighlightedText(cue.text, "(空文本)", cue.id)}</span>
                </span>
                <span className="subtitle-row-text-preview subtitle-row-text-preview-translation" title={cue.translatedText || "(暂无译文)"}>
                  <span className="subtitle-row-text-value">{renderHighlightedText(cue.translatedText, "(暂无译文)", cue.id)}</span>
                </span>
              </div>

              {editingCueId === cue.id ? (
                <>
                  <div className="subtitle-time-grid">
                    <label className="subtitle-time-field">
                      <span>开始</span>
                      <input
                        key={`start-${cue.id}-${cue.startMs}`}
                        className="apple-input"
                        defaultValue={formatSrtTime(cue.startMs)}
                        onBlur={(e) => applyStart(cue, e.currentTarget.value)}
                      />
                    </label>
                    <label className="subtitle-time-field">
                      <span>结束</span>
                      <input
                        key={`end-${cue.id}-${cue.endMs}`}
                        className="apple-input"
                        defaultValue={formatSrtTime(cue.endMs)}
                        onBlur={(e) => applyEnd(cue, e.currentTarget.value)}
                      />
                    </label>
                  </div>

                  {timeErrorByCue[cue.id] ? <div className="subtitle-time-error">{timeErrorByCue[cue.id]}</div> : null}

                  <textarea
                    className="subtitle-editor-textarea subtitle-row-textarea"
                    value={cue.text}
                    onChange={(e) => onUpdateCue(cue.id, { text: e.target.value })}
                    placeholder="输入该字幕段文本"
                  />
                  <textarea
                    className="subtitle-editor-textarea subtitle-row-textarea subtitle-row-textarea-translation"
                    value={cue.translatedText}
                    onChange={(e) => onUpdateCue(cue.id, { translatedText: e.target.value })}
                    placeholder="输入该字幕段译文（可选）"
                  />
                </>
              ) : null}
            </article>
          ))
        )}
      </div>
    </div>
  );

  const applyStart = (cue: SubtitleCue, value: string) => {
    const parsed = parseSrtTime(value);
    if (parsed == null) {
      setTimeErrorByCue((old) => ({ ...old, [cue.id]: "开始时间格式错误，使用 HH:MM:SS,mmm" }));
      return;
    }
    onUpdateCue(cue.id, { startMs: parsed, endMs: Math.max(parsed, cue.endMs) });
    setTimeErrorByCue((old) => ({ ...old, [cue.id]: "" }));
  };

  const applyEnd = (cue: SubtitleCue, value: string) => {
    const parsed = parseSrtTime(value);
    if (parsed == null) {
      setTimeErrorByCue((old) => ({ ...old, [cue.id]: "结束时间格式错误，使用 HH:MM:SS,mmm" }));
      return;
    }
    onUpdateCue(cue.id, { endMs: Math.max(parsed, cue.startMs) });
    setTimeErrorByCue((old) => ({ ...old, [cue.id]: "" }));
  };

  const handleCueClick = (cueId: string, event: MouseEvent<HTMLElement>) => {
    const isToggle = event.ctrlKey || event.metaKey;
    const isRange = event.shiftKey;

    if (isRange) {
      const startId = anchorCueId || primarySelectedCueId || cueId;
      const startIndex = cueIds.indexOf(startId);
      const endIndex = cueIds.indexOf(cueId);
      if (startIndex < 0 || endIndex < 0) {
        setSelectedCueIds([cueId]);
        setAnchorCueId(cueId);
        return;
      }
      const [from, to] = startIndex <= endIndex ? [startIndex, endIndex] : [endIndex, startIndex];
      setSelectedCueIds(cueIds.slice(from, to + 1));
      return;
    }

    if (isToggle) {
      setSelectedCueIds((old) => {
        if (old.includes(cueId)) return old.filter((id) => id !== cueId);
        return [...old, cueId];
      });
      setAnchorCueId(cueId);
      return;
    }

    setSelectedCueIds([cueId]);
    setAnchorCueId(cueId);
    const cursor = matchCueIdToCursor.get(cueId);
    if (cursor != null) {
      setFindCursor(cursor);
    }
  };

  function clearSelection() {
    setSelectedCueIds([]);
    setAnchorCueId("");
  }

  function handleContainerClick(event: MouseEvent<HTMLElement>) {
    const target = event.target as HTMLElement | null;
    if (!target) return;

    const insideCueCard = target.closest(".subtitle-row-card");
    if (insideCueCard) return;

    const isToolbarAction = target.closest(".subtitle-editor-topbar button");
    const isFindReplaceAction = target.closest(".subtitle-find-replace input, .subtitle-find-replace button");
    const isCloseAction = target.closest(".modal-close");
    if (isToolbarAction || isFindReplaceAction || isCloseAction) return;

    clearSelection();
  }

  function handleReplaceOne() {
    setIsReplaceMenuOpen(false);
    const keyword = findText.trim();
    if (!keyword) {
      setFindStatus("请输入查找内容");
      return;
    }
    if (!currentMatch) {
      setFindStatus("未找到匹配项");
      return;
    }

    const count = onReplaceText(keyword, replaceText, [currentMatch.cueId], 1);
    if (count <= 0) {
      setFindStatus("未替换任何内容");
      return;
    }

    setFindStatus("已替换 1 处");
    if (matchCueIndexes.length > 0) {
      setFindCursor((old) => {
        const next = old + 1;
        return next >= matchCueIndexes.length ? 0 : next;
      });
    }
  }

  function handleReplaceAllFromMenu() {
    const keyword = findText.trim();
    if (!keyword) {
      setFindStatus("请输入查找内容");
      setIsReplaceMenuOpen(false);
      return;
    }

    const count = onReplaceText(keyword, replaceText, null);
    if (count > 0) {
      setFindStatus(`已替换 ${count} 处`);
    } else {
      setFindStatus("未替换任何内容");
    }
    setIsReplaceMenuOpen(false);
  }

  function handlePrevMatch() {
    if (!findKeyword) {
      setFindStatus("请输入查找内容");
      return;
    }
    if (matchCueIndexes.length === 0) {
      setFindStatus("无匹配");
      return;
    }
    setFindCursor((old) => (old <= 0 ? matchCueIndexes.length - 1 : old - 1));
    setFindStatus("");
  }

  function handleNextMatch() {
    if (!findKeyword) {
      setFindStatus("请输入查找内容");
      return;
    }
    if (matchCueIndexes.length === 0) {
      setFindStatus("无匹配");
      return;
    }
    setFindCursor((old) => (old + 1 >= matchCueIndexes.length ? 0 : old + 1));
    setFindStatus("");
  }

  if (embedded) {
    return (
      <section className="subtitle-inline-root" role="region" aria-label="字幕编辑器">
        {content}
      </section>
    );
  }

  return (
    <div className="modal-overlay" role="dialog" aria-modal="true">
      {content}
    </div>
  );
}

