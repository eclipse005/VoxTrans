import { type MouseEvent, useEffect, useMemo, useRef, useState } from "react";
import type { SubtitleCue } from "../../features/media/types";
import { formatSrtTime, parseSrtTime } from "../../features/media/srt";
import type { SubtitleSaveState } from "../types";
import { AlertIcon, EditIcon, ReplaceIcon, SearchIcon, TrashIcon } from "./Icons";

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
  onReplaceText: (findText: string, replaceText: string, scopeCueIds: string[] | null) => number;
  onDeleteCue: (cueId: string) => void;
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
  onClose,
}: SubtitleEditorModalProps) {
  const [timeErrorByCue, setTimeErrorByCue] = useState<Record<string, string>>({});
  const [selectedCueIds, setSelectedCueIds] = useState<string[]>([]);
  const [anchorCueId, setAnchorCueId] = useState<string>("");
  const [editingCueId, setEditingCueId] = useState<string>("");
  const [findText, setFindText] = useState("");
  const [replaceText, setReplaceText] = useState("");
  const [findStatus, setFindStatus] = useState("");
  const [findCursor, setFindCursor] = useState(-1);
  const [flashCueId, setFlashCueId] = useState("");
  const [isBatchAnimating, setIsBatchAnimating] = useState(false);
  const batchTimerRef = useRef<number | null>(null);
  const pendingSplitRef = useRef<Array<{ bornCueId: string; fromRect: DOMRect }>>([]);
  const pendingLayoutFromRectsRef = useRef<Map<string, DOMRect> | null>(null);
  const cardRefs = useRef<Record<string, HTMLElement | null>>({});

  const cueIds = useMemo(() => cues.map((cue) => cue.id), [cues]);
  const validSelectedCueIds = useMemo(() => {
    return selectedCueIds.filter((id) => cueIds.includes(id));
  }, [cueIds, selectedCueIds]);
  const primarySelectedCueId = validSelectedCueIds[0] ?? null;

  useEffect(() => {
    return () => {
      if (batchTimerRef.current != null) {
        window.clearTimeout(batchTimerRef.current);
      }
    };
  }, []);

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
        <div>
          <div className="subtitle-title-row">
            <h3 className="apple-heading-small">字幕编辑器</h3>
            <span className="subtitle-count-badge">{cues.length} 条</span>
            <span className={`subtitle-save-indicator subtitle-save-${saveState}`}>{saveStateLabel(saveState)}</span>
          </div>
          <p className="apple-body-small subtitle-editor-meta" title={`任务: ${taskName} · 输出: ${srtPath || "--"}`}>
            任务: {taskName} · 输出: {srtPath || "--"}
          </p>
        </div>
      </div>

      <div className="subtitle-editor-topbar">
        <div className="subtitle-find-replace subtitle-find-replace-inline">
          <input
            className="apple-input subtitle-find-input"
            value={findText}
            onChange={(e) => {
              setFindText(e.target.value);
              setFindCursor(-1);
            }}
            placeholder="查找文本"
          />
          <input
            className="apple-input subtitle-find-input"
            value={replaceText}
            onChange={(e) => setReplaceText(e.target.value)}
            placeholder="替换为"
          />
          <button
            className="subtitle-icon-btn subtitle-find-action-btn"
            onClick={handleFindNext}
            title="查找下一条"
            aria-label="查找下一条"
          >
            <SearchIcon />
          </button>
          <button
            className="subtitle-icon-btn subtitle-find-action-btn"
            onClick={handleReplaceAll}
            title="全部替换"
            aria-label="全部替换"
          >
            <ReplaceIcon />
          </button>
          <span className="subtitle-find-status">{findStatus}</span>
        </div>

        <div className="subtitle-row-actions">
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

      <div
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
              className={`subtitle-row-card ${validSelectedCueIds.includes(cue.id) ? "selected" : ""} ${flashCueId === cue.id ? "flash-hit" : ""}`}
              onClick={(event) => handleCueClick(cue.id, event)}
            >
              <div className="subtitle-row-head">
                <div className="subtitle-row-head-main">
                  <span className="subtitle-row-index">#{idx + 1}</span>
                  <span className="subtitle-row-time">{formatSrtTime(cue.startMs)}</span>
                  <span className="subtitle-time-arrow">→</span>
                  <span className="subtitle-row-time">{formatSrtTime(cue.endMs)}</span>
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
                  {cue.text || "(空文本)"}
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

  function handleFindNext() {
    const keyword = findText.trim().toLowerCase();
    if (!keyword) {
      setFindStatus("请输入查找内容");
      return;
    }

    const startIndex = findCursor >= 0 ? findCursor : (primarySelectedCueId ? cueIds.indexOf(primarySelectedCueId) : -1);
    for (let step = 1; step <= cues.length; step += 1) {
      const idx = (startIndex + step + cues.length) % cues.length;
      const cue = cues[idx];
      if (!cue) continue;

      const seq = String(idx + 1);
      const start = formatSrtTime(cue.startMs);
      const end = formatSrtTime(cue.endMs);
      const range = `${start} --> ${end}`;
      const haystack = [seq, `#${seq}`, start, end, range, cue.text]
        .join(" ")
        .toLowerCase();

      if (haystack.includes(keyword)) {
        setFindCursor(idx);
        setFindStatus(`定位到 #${idx + 1}`);
        window.requestAnimationFrame(() => {
          const node = cardRefs.current[cue.id];
          if (node) {
            node.scrollIntoView({ behavior: "smooth", block: "center" });
          }
        });
        setFlashCueId(cue.id);
        window.setTimeout(() => {
          setFlashCueId((old) => (old === cue.id ? "" : old));
        }, 900);
        return;
      }
    }

    setFindStatus("未找到匹配项");
  }

  function handleReplaceAll() {
    const keyword = findText.trim();
    if (!keyword) {
      setFindStatus("请输入查找内容");
      return;
    }

    const useSelectionScope = validSelectedCueIds.length > 1 ? validSelectedCueIds : null;
    const count = onReplaceText(keyword, replaceText, useSelectionScope);
    if (count > 0) {
      setFindStatus(`已替换 ${count} 处`);
    } else {
      setFindStatus("未替换任何内容");
    }
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
