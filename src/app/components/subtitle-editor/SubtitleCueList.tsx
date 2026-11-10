import {
  memo,
  useCallback,
  useMemo,
  type MouseEvent,
  type RefObject,
} from "react";
import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";
import type { SubtitleCue } from "../../../features/media/types";
import type { Diagnostic } from "../../../features/media/subtitleDiagnostics";
import { AlertIcon, TrashIcon } from "../Icons";
import { diagnosticLabel } from "./diagnosticLabels";
import { TimeField } from "./TimeField";

/// Labels a row needs that do not depend on the row. Resolved once per list
/// instead of once per row: a task can hold well over a thousand cues, and the
/// profiler put i18n key parsing (`formatLanguageCode`, `extractFromKey`) at the
/// top of the mount cost. Keys that interpolate the row index stay on `t`.
type SubtitleRowLabels = {
  select: string;
  startTime: string;
  endTime: string;
  delete: string;
  textPlaceholder: string;
  translationPlaceholder: string;
};

type SubtitleCueListProps = {
  canEdit: boolean;
  cues: SubtitleCue[];
  /** Problem list per cue id. Exception-only: a clean cue has no entry. */
  diagnosticsByCueId: Record<string, Diagnostic[]>;
  /** Display number per cue id. Filtering the list must not renumber cues. */
  cueNumbers?: Record<string, number>;
  /** Total cues in the draft. A filter can shrink `cues`, but the delete
   *  guard must keep counting the whole list. */
  totalCount?: number;
  /** Cue briefly highlighted after the quality bar locates it. */
  locatedCueId?: string | null;
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
  onCommitStart: (cue: SubtitleCue, ms: number) => void;
  onCommitEnd: (cue: SubtitleCue, ms: number) => void;
  onRejectStart: (cueId: string) => void;
  onRejectEnd: (cueId: string) => void;
  onClearTimeError: (cueId: string) => void;
  onUpdateCue: (cueId: string, patch: Partial<SubtitleCue>) => void;
};

type SubtitleCueRowProps = {
  cue: SubtitleCue;
  canEdit: boolean;
  cuesLength: number;
  isSelected: boolean;
  hasFindHit: boolean;
  isFindCurrent: boolean;
  /// Stable cue number (survives filtering); falls back to list position.
  cueNumber: number;
  diagnostics: Diagnostic[] | undefined;
  isLocated: boolean;
  timeError: string;
  /// Resolved once by the list; see `SubtitleRowLabels`.
  labels: SubtitleRowLabels;
  /// Still needed for the keys that interpolate this row's index.
  t: TFunction;
  registerCardRef: (cueId: string, node: HTMLElement | null) => void;
  onCueClick: (cueId: string, event: MouseEvent<HTMLElement>) => void;
  onEnsureSelected: (cueId: string) => void;
  onDeleteCue: (cueId: string) => void;
  onCommitStart: (cue: SubtitleCue, ms: number) => void;
  onCommitEnd: (cue: SubtitleCue, ms: number) => void;
  onRejectStart: (cueId: string) => void;
  onRejectEnd: (cueId: string) => void;
  onClearTimeError: (cueId: string) => void;
  onUpdateCue: (cueId: string, patch: Partial<SubtitleCue>) => void;
};

function stopCardClick(event: MouseEvent<HTMLElement>) {
  event.stopPropagation();
}

const SubtitleCueRow = memo(function SubtitleCueRow({
  cue,
  canEdit,
  cuesLength,
  isSelected,
  hasFindHit,
  isFindCurrent,
  cueNumber,
  diagnostics,
  isLocated,
  timeError,
  labels,
  t,
  registerCardRef,
  onCueClick,
  onEnsureSelected,
  onDeleteCue,
  onCommitStart,
  onCommitEnd,
  onRejectStart,
  onRejectEnd,
  onClearTimeError,
  onUpdateCue,
}: SubtitleCueRowProps) {
  const warningList = diagnostics ?? [];
  const hasError = warningList.some((diagnostic) => diagnostic.severity === "error");
  const timeErrorId = timeError ? `subtitle-time-error-${cue.id}` : undefined;

  const handleCardRef = (node: HTMLElement | null) => {
    registerCardRef(cue.id, node);
  };

  const handleFocusCue = useCallback(() => {
    onEnsureSelected(cue.id);
  }, [cue.id, onEnsureSelected]);

  const handleStartCommit = useCallback(
    (ms: number) => onCommitStart(cue, ms),
    [cue, onCommitStart],
  );
  const handleEndCommit = useCallback(
    (ms: number) => onCommitEnd(cue, ms),
    [cue, onCommitEnd],
  );
  const handleRejectStart = useCallback(
    () => onRejectStart(cue.id),
    [cue.id, onRejectStart],
  );
  const handleRejectEnd = useCallback(
    () => onRejectEnd(cue.id),
    [cue.id, onRejectEnd],
  );
  const handleClearTimeError = useCallback(
    () => onClearTimeError(cue.id),
    [cue.id, onClearTimeError],
  );

  const cardClassName = [
    "subtitle-row-card",
    isSelected ? "selected" : "",
    hasFindHit ? "has-find-hit" : "",
    isFindCurrent ? "is-find-current" : "",
    isLocated ? "is-located" : "",
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
            title={labels.select}
            aria-label={t("subtitles:cue.selectAria", { n: cueNumber })}
            aria-pressed={isSelected}
            onClick={(event) => {
              event.stopPropagation();
              onCueClick(cue.id, event);
            }}
          >
            #{cueNumber}
          </button>
          <TimeField
            valueMs={cue.startMs}
            label={labels.startTime}
            canEdit={canEdit}
            invalid={Boolean(timeError)}
            describedBy={timeErrorId}
            onCommit={handleStartCommit}
            onReject={handleRejectStart}
            onCancel={handleClearTimeError}
            onFocusCue={handleFocusCue}
          />
          <span className="subtitle-time-arrow" aria-hidden="true">
            →
          </span>
          <TimeField
            valueMs={cue.endMs}
            label={labels.endTime}
            canEdit={canEdit}
            invalid={Boolean(timeError)}
            describedBy={timeErrorId}
            onCommit={handleEndCommit}
            onReject={handleRejectEnd}
            onCancel={handleClearTimeError}
            onFocusCue={handleFocusCue}
          />
        </div>
        <div className="subtitle-row-actions">
          {warningList.length > 0 ? (
            <span
              className={`subtitle-warning-badge${hasError ? " is-error" : " is-warning"}`}
              title={warningList.map((diagnostic) => diagnosticLabel(t, diagnostic)).join("\n")}
              aria-label={t("subtitles:cue.warningCount", { count: warningList.length })}
            >
              <AlertIcon />
            </span>
          ) : null}
          <button
            type="button"
            className="subtitle-icon-btn subtitle-icon-btn-danger"
            title={labels.delete}
            aria-label={labels.delete}
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
        className="subtitle-editor-textarea subtitle-row-textarea"
        value={cue.text}
        onChange={(e) => onUpdateCue(cue.id, { text: e.target.value })}
        onClick={stopCardClick}
        onFocus={handleFocusCue}
        placeholder={labels.textPlaceholder}
        aria-label={t("subtitles:cue.textAria", { n: cueNumber })}
        readOnly={!canEdit}
        rows={1}
      />
      <textarea
        className="subtitle-editor-textarea subtitle-row-textarea subtitle-row-textarea-translation"
        value={cue.translatedText}
        onChange={(e) => onUpdateCue(cue.id, { translatedText: e.target.value })}
        onClick={stopCardClick}
        onFocus={handleFocusCue}
        placeholder={labels.translationPlaceholder}
        aria-label={t("subtitles:cue.translationAria", { n: cueNumber })}
        readOnly={!canEdit}
        rows={1}
      />
    </article>
  );
});

function SubtitleCueList({
  canEdit,
  cues,
  diagnosticsByCueId,
  cueNumbers,
  totalCount,
  locatedCueId,
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
  onCommitStart,
  onCommitEnd,
  onRejectStart,
  onRejectEnd,
  onClearTimeError,
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

  const { t } = useTranslation(["subtitles", "common"]);

  // Resolved once per list rather than once per row (see SubtitleRowLabels), and
  // the selection is turned into a Set once: `selectedCueIds` is an array, so the
  // per-row `includes` was O(rows x selected) on every render.
  const rowLabels = useMemo<SubtitleRowLabels>(
    () => ({
      select: t("subtitles:cue.select"),
      startTime: t("subtitles:cue.startTime"),
      endTime: t("subtitles:cue.endTime"),
      delete: t("subtitles:cue.delete"),
      textPlaceholder: t("subtitles:cue.textPlaceholder"),
      translationPlaceholder: t("subtitles:cue.translationPlaceholder"),
    }),
    [t],
  );
  const selectedIdSet = useMemo(() => new Set(selectedCueIds), [selectedCueIds]);

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
            canEdit={canEdit}
            cuesLength={totalCount ?? cues.length}
            isSelected={selectedIdSet.has(cue.id)}
            hasFindHit={matchedCueIds.has(cue.id)}
            isFindCurrent={currentMatchCueId === cue.id}
            cueNumber={cueNumbers?.[cue.id] ?? idx + 1}
            diagnostics={diagnosticsByCueId[cue.id]}
            isLocated={locatedCueId === cue.id}
            timeError={timeErrorByCue[cue.id] ?? ""}
            labels={rowLabels}
            t={t}
            registerCardRef={registerCardRef}
            onCueClick={onCueClick}
            onEnsureSelected={onEnsureSelected}
            onDeleteCue={onDeleteCue}
            onCommitStart={onCommitStart}
            onCommitEnd={onCommitEnd}
            onRejectStart={onRejectStart}
            onRejectEnd={onRejectEnd}
            onClearTimeError={onClearTimeError}
            onUpdateCue={onUpdateCue}
          />
        ))
      )}
    </div>
  );
}

export default memo(SubtitleCueList);
