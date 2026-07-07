import type { MouseEvent, ReactNode, RefObject } from "react";
import { useTranslation } from "react-i18next";
import type { SubtitleCue } from "../../../features/media/types";
import { formatSrtTime } from "../../../features/media/srt";
import { AlertIcon, EditIcon, TrashIcon } from "../Icons";

type SubtitleCueListProps = {
  canEdit: boolean;
  cues: SubtitleCue[];
  cueWarningsById: Record<string, string[]>;
  /** Message shown when there are no cues. Caller decides the wording from
   *  task state; the list only renders it (presentational component). */
  emptyText: string;
  editingCueId: string;
  selectedCueIds: string[];
  timeErrorByCue: Record<string, string>;
  listContainerRef: RefObject<HTMLDivElement | null>;
  cardRefs: RefObject<Record<string, HTMLElement | null>>;
  renderHighlightedText: (text: string, fallback: string, cueId: string) => ReactNode;
  onClearSelection: () => void;
  onCueClick: (cueId: string, event: MouseEvent<HTMLElement>) => void;
  onToggleEdit: (cueId: string) => void;
  onDeleteCue: (cueId: string) => void;
  onApplyStart: (cue: SubtitleCue, value: string) => void;
  onApplyEnd: (cue: SubtitleCue, value: string) => void;
  onUpdateCue: (cueId: string, patch: Partial<SubtitleCue>) => void;
};

export default function SubtitleCueList({
  canEdit,
  cues,
  cueWarningsById,
  emptyText,
  editingCueId,
  selectedCueIds,
  timeErrorByCue,
  listContainerRef,
  cardRefs,
  renderHighlightedText,
  onClearSelection,
  onCueClick,
  onToggleEdit,
  onDeleteCue,
  onApplyStart,
  onApplyEnd,
  onUpdateCue,
}: SubtitleCueListProps) {
  const { t } = useTranslation(["subtitles", "common"]);
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
          <article
            key={cue.id}
            ref={(node) => {
              cardRefs.current[cue.id] = node;
            }}
            className={`subtitle-row-card ${selectedCueIds.includes(cue.id) ? "selected" : ""}`}
            onClick={(event) => onCueClick(cue.id, event)}
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
                    aria-label={t("subtitles:cue.warningCount", { count: cueWarningsById[cue.id].length })}
                  >
                    <AlertIcon />
                  </span>
                ) : null}
                <button
                  className="subtitle-icon-btn"
                  title={editingCueId === cue.id ? t("subtitles:cue.collapseEdit") : t("subtitles:cue.edit")}
                  onClick={(e) => {
                    e.stopPropagation();
                    onToggleEdit(cue.id);
                  }}
                  disabled={!canEdit}
                >
                  <EditIcon />
                </button>
                <button
                  className="subtitle-icon-btn subtitle-icon-btn-danger"
                  title={t("subtitles:cue.delete")}
                  onClick={(e) => {
                    e.stopPropagation();
                    onDeleteCue(cue.id);
                  }}
                  disabled={!canEdit || cues.length <= 1}
                >
                  <TrashIcon />
                </button>
              </div>
            </div>

            <div className="subtitle-row-summary">
              <span className="subtitle-row-text-preview" title={cue.text || t("subtitles:cue.emptyText")}>
                <span className="subtitle-row-text-value">{renderHighlightedText(cue.text, t("subtitles:cue.emptyText"), cue.id)}</span>
              </span>
              <span className="subtitle-row-text-preview subtitle-row-text-preview-translation" title={cue.translatedText || t("subtitles:cue.noTranslation")}>
                <span className="subtitle-row-text-value">{renderHighlightedText(cue.translatedText, t("subtitles:cue.noTranslation"), cue.id)}</span>
              </span>
            </div>

            {editingCueId === cue.id ? (
              <>
                <div className="subtitle-time-grid">
                  <label className="subtitle-time-field">
                    <span>{t("subtitles:cue.startTime")}</span>
                    <input
                      key={`start-${cue.id}-${cue.startMs}`}
                      className="apple-input"
                      defaultValue={formatSrtTime(cue.startMs)}
                      onBlur={(e) => onApplyStart(cue, e.currentTarget.value)}
                      disabled={!canEdit}
                    />
                  </label>
                  <label className="subtitle-time-field">
                    <span>{t("subtitles:cue.endTime")}</span>
                    <input
                      key={`end-${cue.id}-${cue.endMs}`}
                      className="apple-input"
                      defaultValue={formatSrtTime(cue.endMs)}
                      onBlur={(e) => onApplyEnd(cue, e.currentTarget.value)}
                      disabled={!canEdit}
                    />
                  </label>
                </div>

                {timeErrorByCue[cue.id] ? <div className="subtitle-time-error">{timeErrorByCue[cue.id]}</div> : null}

                <textarea
                  className="subtitle-editor-textarea subtitle-row-textarea"
                  value={cue.text}
                  onChange={(e) => onUpdateCue(cue.id, { text: e.target.value })}
                  placeholder={t("subtitles:cue.textPlaceholder")}
                  disabled={!canEdit}
                />
                <textarea
                  className="subtitle-editor-textarea subtitle-row-textarea subtitle-row-textarea-translation"
                  value={cue.translatedText}
                  onChange={(e) => onUpdateCue(cue.id, { translatedText: e.target.value })}
                  placeholder={t("subtitles:cue.translationPlaceholder")}
                  disabled={!canEdit}
                />
              </>
            ) : null}
          </article>
        ))
      )}
    </div>
  );
}
