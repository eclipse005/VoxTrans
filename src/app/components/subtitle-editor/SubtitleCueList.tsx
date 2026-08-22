import {
  memo,
  useCallback,
  type MouseEvent,
  type RefObject,
} from "react";
import { useTranslation } from "react-i18next";
import type { SubtitleCue } from "../../../features/media/types";
import { AlertIcon, TrashIcon } from "../Icons";
import { TimeField } from "./TimeField";

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
  onCommitStart: (cue: SubtitleCue, ms: number) => void;
  onCommitEnd: (cue: SubtitleCue, ms: number) => void;
  onRejectStart: (cueId: string) => void;
  onRejectEnd: (cueId: string) => void;
  onClearTimeError: (cueId: string) => void;
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
  onCommitStart,
  onCommitEnd,
  onRejectStart,
  onRejectEnd,
  onClearTimeError,
  onUpdateCue,
}: SubtitleCueRowProps) {
  const { t } = useTranslation(["subtitles", "common"]);
  const warningList = warnings ?? [];
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
            label={t("subtitles:cue.endTime")}
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
        className="subtitle-editor-textarea subtitle-row-textarea"
        value={cue.text}
        onChange={(e) => onUpdateCue(cue.id, { text: e.target.value })}
        onClick={stopCardClick}
        onFocus={handleFocusCue}
        placeholder={t("subtitles:cue.textPlaceholder")}
        aria-label={t("subtitles:cue.textAria", { n: index + 1 })}
        readOnly={!canEdit}
        rows={1}
      />
      <textarea
        className="subtitle-editor-textarea subtitle-row-textarea subtitle-row-textarea-translation"
        value={cue.translatedText}
        onChange={(e) => onUpdateCue(cue.id, { translatedText: e.target.value })}
        onClick={stopCardClick}
        onFocus={handleFocusCue}
        placeholder={t("subtitles:cue.translationPlaceholder")}
        aria-label={t("subtitles:cue.translationAria", { n: index + 1 })}
        readOnly={!canEdit}
        rows={1}
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
