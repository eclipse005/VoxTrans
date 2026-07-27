import {
  memo,
  useCallback,
  useLayoutEffect,
  useRef,
  useState,
  type ChangeEvent,
  type FocusEvent,
  type KeyboardEvent,
  type MouseEvent,
  type RefObject,
} from "react";
import { useTranslation } from "react-i18next";
import type { SubtitleCue } from "../../../features/media/types";
import { formatSrtTime } from "../../../features/media/srt";
import { AlertIcon, TrashIcon } from "../Icons";

const TEXTAREA_MIN_HEIGHT_PX = 52;

type SubtitleCueListProps = {
  canEdit: boolean;
  cues: SubtitleCue[];
  cueWarningsById: Record<string, string[]>;
  /** Message shown when there are no cues. Caller decides the wording from
   *  task state; the list only renders it (presentational component). */
  emptyText: string;
  selectedCueIds: string[];
  matchedCueIds: ReadonlySet<string>;
  currentMatchCueId: string | null;
  timeErrorByCue: Record<string, string>;
  listContainerRef: RefObject<HTMLDivElement | null>;
  cardRefs: RefObject<Record<string, HTMLElement | null>>;
  onClearSelection: () => void;
  onCueClick: (cueId: string, event: MouseEvent<HTMLElement>) => void;
  /** Focus into a field: keep multi-select if cue already selected. */
  onEnsureSelected: (cueId: string) => void;
  onDeleteCue: (cueId: string) => void;
  onApplyStart: (cue: SubtitleCue, value: string) => void;
  onApplyEnd: (cue: SubtitleCue, value: string) => void;
  onUpdateCue: (cueId: string, patch: Partial<SubtitleCue>) => void;
};

type SubtitleCueRowProps = {
  cue: SubtitleCue;
  index: number;
  canEdit: boolean;
  cuesLength: number;
  isSelected: boolean;
  hasFindHit: boolean;
  isFindCurrent: boolean;
  warnings: string[] | undefined;
  timeError: string;
  registerCardRef: (cueId: string, node: HTMLElement | null) => void;
  onCueClick: (cueId: string, event: MouseEvent<HTMLElement>) => void;
  onEnsureSelected: (cueId: string) => void;
  onDeleteCue: (cueId: string) => void;
  onApplyStart: (cue: SubtitleCue, value: string) => void;
  onApplyEnd: (cue: SubtitleCue, value: string) => void;
  onUpdateCue: (cueId: string, patch: Partial<SubtitleCue>) => void;
};

function resizeTextarea(el: HTMLTextAreaElement | null) {
  if (!el) return;
  el.style.height = "0px";
  el.style.height = `${Math.max(el.scrollHeight, TEXTAREA_MIN_HEIGHT_PX)}px`;
}

/**
 * Shared pointer policy for inline fields:
 * - modifier click: multi-select (do not start a text caret gesture)
 * - plain click: stop bubbling so the card does not double-handle selection;
 *   focus path calls ensureSelected
 */
function useInlineFieldPointer(cueId: string, onCueClick: SubtitleCueRowProps["onCueClick"]) {
  const onMouseDown = useCallback(
    (event: MouseEvent<HTMLElement>) => {
      if (event.shiftKey || event.ctrlKey || event.metaKey) {
        event.preventDefault();
        event.stopPropagation();
        onCueClick(cueId, event);
      }
    },
    [cueId, onCueClick],
  );

  const onClick = useCallback((event: MouseEvent<HTMLElement>) => {
    event.stopPropagation();
  }, []);

  return { onMouseDown, onClick };
}

/**
 * Edit-session time field.
 *
 * Canonical value is always `valueMs`. Display is `sessionDraft ?? committed`
 * so idle rows track props (including sibling endMs clamps) with no remount
 * and no sync effect. A session opens only on the first edit keystroke —
 * focus alone does not snapshot a stale value.
 */
function TimeField({
  valueMs,
  label,
  canEdit,
  invalid,
  describedBy,
  onApply,
  onMouseDown,
  onClick,
  onFocusCue,
}: {
  valueMs: number;
  label: string;
  canEdit: boolean;
  invalid: boolean;
  describedBy?: string;
  onApply: (value: string) => void;
  onMouseDown: (event: MouseEvent<HTMLElement>) => void;
  onClick: (event: MouseEvent<HTMLElement>) => void;
  onFocusCue: () => void;
}) {
  const committed = formatSrtTime(valueMs);
  // null = idle (derive from props); string = user has edited this focus cycle
  const [sessionDraft, setSessionDraft] = useState<string | null>(null);
  const sessionDraftRef = useRef<string | null>(null);
  const display = sessionDraft ?? committed;

  const handleFocus = useCallback(
    (event: FocusEvent<HTMLInputElement>) => {
      onFocusCue();
      if (canEdit) {
        event.currentTarget.select();
      }
    },
    [canEdit, onFocusCue],
  );

  const handleChange = useCallback((event: ChangeEvent<HTMLInputElement>) => {
    const next = event.currentTarget.value;
    sessionDraftRef.current = next;
    setSessionDraft(next);
  }, []);

  const handleBlur = useCallback(() => {
    const pending = sessionDraftRef.current;
    sessionDraftRef.current = null;
    setSessionDraft(null);
    if (!canEdit || pending === null) return;
    onApply(pending);
  }, [canEdit, onApply]);

  const handleKeyDown = useCallback((event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Enter") {
      event.preventDefault();
      event.currentTarget.blur();
      return;
    }
    if (event.key === "Escape") {
      event.preventDefault();
      // Drop any in-progress edit; blur sees null session and does not apply.
      sessionDraftRef.current = null;
      setSessionDraft(null);
      event.currentTarget.blur();
    }
  }, []);

  return (
    <input
      type="text"
      className="subtitle-time-input"
      value={display}
      aria-label={label}
      title={label}
      aria-invalid={invalid || undefined}
      aria-describedby={describedBy}
      spellCheck={false}
      readOnly={!canEdit}
      onMouseDown={onMouseDown}
      onClick={onClick}
      onFocus={handleFocus}
      onChange={handleChange}
      onBlur={handleBlur}
      onKeyDown={handleKeyDown}
    />
  );
}

const SubtitleCueRow = memo(function SubtitleCueRow({
  cue,
  index,
  canEdit,
  cuesLength,
  isSelected,
  hasFindHit,
  isFindCurrent,
  warnings,
  timeError,
  registerCardRef,
  onCueClick,
  onEnsureSelected,
  onDeleteCue,
  onApplyStart,
  onApplyEnd,
  onUpdateCue,
}: SubtitleCueRowProps) {
  const { t } = useTranslation(["subtitles", "common"]);
  const sourceRef = useRef<HTMLTextAreaElement | null>(null);
  const translationRef = useRef<HTMLTextAreaElement | null>(null);
  const warningList = warnings ?? [];
  const timeErrorId = timeError ? `subtitle-time-error-${cue.id}` : undefined;
  const fieldPointer = useInlineFieldPointer(cue.id, onCueClick);

  useLayoutEffect(() => {
    resizeTextarea(sourceRef.current);
  }, [cue.text]);

  useLayoutEffect(() => {
    resizeTextarea(translationRef.current);
  }, [cue.translatedText]);

  const handleCardRef = (node: HTMLElement | null) => {
    registerCardRef(cue.id, node);
  };

  const handleFocusCue = useCallback(() => {
    onEnsureSelected(cue.id);
  }, [cue.id, onEnsureSelected]);

  const handleApplyStart = useCallback(
    (value: string) => {
      onApplyStart(cue, value);
    },
    [cue, onApplyStart],
  );

  const handleApplyEnd = useCallback(
    (value: string) => {
      onApplyEnd(cue, value);
    },
    [cue, onApplyEnd],
  );

  const cardClassName = [
    "subtitle-row-card",
    isSelected ? "selected" : "",
    hasFindHit ? "has-find-hit" : "",
    isFindCurrent ? "is-find-current" : "",
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <article
      ref={handleCardRef}
      className={cardClassName}
      aria-current={isFindCurrent ? "true" : undefined}
      onClick={(event) => onCueClick(cue.id, event)}
    >
      <div className="subtitle-row-head">
        <div className="subtitle-row-head-main">
          <button
            type="button"
            className="subtitle-row-index"
            title={t("subtitles:cue.select")}
            aria-label={t("subtitles:cue.selectAria", { n: index + 1 })}
            aria-pressed={isSelected}
            onClick={(event) => {
              event.stopPropagation();
              onCueClick(cue.id, event);
            }}
          >
            #{index + 1}
          </button>
          <TimeField
            valueMs={cue.startMs}
            label={t("subtitles:cue.startTime")}
            canEdit={canEdit}
            invalid={Boolean(timeError)}
            describedBy={timeErrorId}
            onApply={handleApplyStart}
            onMouseDown={fieldPointer.onMouseDown}
            onClick={fieldPointer.onClick}
            onFocusCue={handleFocusCue}
          />
          <span className="subtitle-time-arrow" aria-hidden="true">
            →
          </span>
          <TimeField
            valueMs={cue.endMs}
            label={t("subtitles:cue.endTime")}
            canEdit={canEdit}
            invalid={Boolean(timeError)}
            describedBy={timeErrorId}
            onApply={handleApplyEnd}
            onMouseDown={fieldPointer.onMouseDown}
            onClick={fieldPointer.onClick}
            onFocusCue={handleFocusCue}
          />
        </div>
        <div className="subtitle-row-actions">
          {warningList.length > 0 ? (
            <span
              className="subtitle-warning-badge"
              title={warningList.join("\n")}
              aria-label={t("subtitles:cue.warningCount", { count: warningList.length })}
            >
              <AlertIcon />
            </span>
          ) : null}
          <button
            type="button"
            className="subtitle-icon-btn subtitle-icon-btn-danger"
            title={t("subtitles:cue.delete")}
            aria-label={t("subtitles:cue.delete")}
            onClick={(e) => {
              e.stopPropagation();
              onDeleteCue(cue.id);
            }}
            disabled={!canEdit || cuesLength <= 1}
          >
            <TrashIcon />
          </button>
        </div>
      </div>

      {timeError ? (
        <div id={timeErrorId} className="subtitle-time-error" role="alert">
          {timeError}
        </div>
      ) : null}

      <textarea
        ref={sourceRef}
        className="subtitle-editor-textarea subtitle-row-textarea"
        value={cue.text}
        onChange={(e) => onUpdateCue(cue.id, { text: e.target.value })}
        onMouseDown={fieldPointer.onMouseDown}
        onClick={fieldPointer.onClick}
        onFocus={handleFocusCue}
        placeholder={t("subtitles:cue.textPlaceholder")}
        aria-label={t("subtitles:cue.textAria", { n: index + 1 })}
        readOnly={!canEdit}
        rows={2}
      />
      <textarea
        ref={translationRef}
        className="subtitle-editor-textarea subtitle-row-textarea subtitle-row-textarea-translation"
        value={cue.translatedText}
        onChange={(e) => onUpdateCue(cue.id, { translatedText: e.target.value })}
        onMouseDown={fieldPointer.onMouseDown}
        onClick={fieldPointer.onClick}
        onFocus={handleFocusCue}
        placeholder={t("subtitles:cue.translationPlaceholder")}
        aria-label={t("subtitles:cue.translationAria", { n: index + 1 })}
        readOnly={!canEdit}
        rows={2}
      />
    </article>
  );
});

function SubtitleCueList({
  canEdit,
  cues,
  cueWarningsById,
  emptyText,
  selectedCueIds,
  matchedCueIds,
  currentMatchCueId,
  timeErrorByCue,
  listContainerRef,
  cardRefs,
  onClearSelection,
  onCueClick,
  onEnsureSelected,
  onDeleteCue,
  onApplyStart,
  onApplyEnd,
  onUpdateCue,
}: SubtitleCueListProps) {
  const registerCardRef = useCallback(
    (cueId: string, node: HTMLElement | null) => {
      // cardRefs is a useRef-owned registry; writing to it inside a ref
      // callback is the documented "map of refs" pattern, not prop mutation.
      // eslint-disable-next-line react-hooks/immutability
      cardRefs.current[cueId] = node;
    },
    [cardRefs],
  );

  return (
    <div
      ref={listContainerRef}
      className="subtitle-all-editor"
      onClick={(event) => {
        if (event.target === event.currentTarget) {
          onClearSelection();
        }
      }}
    >
      {cues.length === 0 ? (
        <div className="subtitle-cue-empty">{emptyText}</div>
      ) : (
        cues.map((cue, idx) => (
          <SubtitleCueRow
            key={cue.id}
            cue={cue}
            index={idx}
            canEdit={canEdit}
            cuesLength={cues.length}
            isSelected={selectedCueIds.includes(cue.id)}
            hasFindHit={matchedCueIds.has(cue.id)}
            isFindCurrent={currentMatchCueId === cue.id}
            warnings={cueWarningsById[cue.id]}
            timeError={timeErrorByCue[cue.id] ?? ""}
            registerCardRef={registerCardRef}
            onCueClick={onCueClick}
            onEnsureSelected={onEnsureSelected}
            onDeleteCue={onDeleteCue}
            onApplyStart={onApplyStart}
            onApplyEnd={onApplyEnd}
            onUpdateCue={onUpdateCue}
          />
        ))
      )}
    </div>
  );
}

export default memo(SubtitleCueList);
