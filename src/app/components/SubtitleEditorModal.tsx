import { type MouseEvent, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { SubtitleCue } from "../../features/media/types";
import { useSubtitleBatchAnimations } from "../hooks/useSubtitleBatchAnimations";
import { useSubtitleFindReplace } from "../hooks/useSubtitleFindReplace";
import { useSubtitleSelection } from "../hooks/useSubtitleSelection";
import { useSubtitleTimeValidation } from "../hooks/useSubtitleTimeValidation";
import SubtitleCueList from "./subtitle-editor/SubtitleCueList";
import SubtitleEditorHeader from "./subtitle-editor/SubtitleEditorHeader";
import SubtitleEditorToolbar from "./subtitle-editor/SubtitleEditorToolbar";

type SubtitleEditorModalProps = {
  visible: boolean;
  embedded?: boolean;
  canEdit: boolean;
  readOnlyReason?: string;
  /** Empty-state message for the cue list, resolved by the caller from task
   *  state so this component stays presentational. */
  emptyText?: string;
  taskName: string;
  cues: SubtitleCue[];
  cueWarningsById: Record<string, string[]>;
  onUpdateCue: (cueId: string, patch: Partial<SubtitleCue>) => void;
  onAddCueAfter: (selectedCueId: string | null) => void;
  onMergeSelected: (selectedCueIds: string[]) => void;
  onSplitSelected: (selectedCueIds: string[]) => Array<{ sourceCueId: string; bornCueId: string }>;
  onReplaceText: (findText: string, replaceText: string, scopeCueIds: string[] | null, maxReplacements?: number) => number;
  onDeleteCue: (cueId: string) => void;
  onOpenSrtDir: () => void | Promise<void>;
  onExportSrt: () => void | Promise<void>;
  onOpenLogs: () => void | Promise<void>;
  onClose: () => void | Promise<void>;
  reviewSource?: boolean;
  reviewTarget?: boolean;
  reviewBanner?: string;
  onToggleReviewSource?: (value: boolean) => void;
  onToggleReviewTarget?: (value: boolean) => void;
};

export default function SubtitleEditorModal({
  visible,
  embedded = false,
  canEdit,
  readOnlyReason = "",
  emptyText,
  taskName,
  cues,
  cueWarningsById,
  onUpdateCue,
  onAddCueAfter,
  onMergeSelected,
  onSplitSelected,
  onReplaceText,
  onDeleteCue,
  onOpenSrtDir,
  onExportSrt,
  onOpenLogs,
  onClose,
  reviewSource = false,
  reviewTarget = false,
  reviewBanner = "",
  onToggleReviewSource,
  onToggleReviewTarget,
}: SubtitleEditorModalProps) {
  const { t } = useTranslation(["subtitles", "common"]);
  const [editingCueId, setEditingCueId] = useState<string>("");
  const listContainerRef = useRef<HTMLDivElement | null>(null);
  const cardRefs = useRef<Record<string, HTMLElement | null>>({});

  const cueIds = useMemo(() => cues.map((cue) => cue.id), [cues]);
  const {
    findText,
    replaceText,
    findKeyword,
    findCounterLabel,
    findStatusLabel,
    isReplaceMenuOpen,
    replaceMenuRef,
    currentMatch,
    matchCount,
    onFindTextChange,
    onReplaceTextChange,
    onToggleReplaceMenu,
    onReplaceOne,
    onReplaceAll,
    onPrevMatch,
    onNextMatch,
    moveCursorToCue,
    renderHighlightedText,
  } = useSubtitleFindReplace({
    cues,
    onReplaceText,
  });
  const {
    isBatchAnimating,
    runSplitAnimation,
  } = useSubtitleBatchAnimations({
    cues,
    listContainerRef,
    cardRefs,
    currentMatchCueId: currentMatch?.cueId ?? null,
  });
  const {
    timeErrorByCue,
    applyStart,
    applyEnd,
  } = useSubtitleTimeValidation({
    onUpdateCue,
  });
  const {
    validSelectedCueIds,
    primarySelectedCueId,
    orderedSelectedCueIds,
    clearSelection,
    selectForEdit,
    handleCueClick,
  } = useSubtitleSelection({
    cueIds,
    onSelectedCueChanged: moveCursorToCue,
  });
  if (!visible) return null;

  const content = (
    <div className={embedded ? "subtitle-inline-content" : "modal-content modal-content-subtitle"} onClick={handleContainerClick}>
      {!embedded ? (
        <button className="modal-close" onClick={() => { void onClose(); }} aria-label={t("common:button.close")}>
          ×
        </button>
      ) : null}

      <SubtitleEditorHeader
        canEdit={canEdit}
        readOnlyReason={readOnlyReason}
        cueCount={cues.length}
        taskName={taskName}
        reviewSource={reviewSource}
        reviewTarget={reviewTarget}
        reviewBanner={reviewBanner}
        onToggleReviewSource={onToggleReviewSource}
        onToggleReviewTarget={onToggleReviewTarget}
        onOpenSrtDir={onOpenSrtDir}
        onExportSrt={onExportSrt}
        onOpenLogs={onOpenLogs}
      />

      <SubtitleEditorToolbar
        canEdit={canEdit}
        findText={findText}
        replaceText={replaceText}
        findCounterLabel={findCounterLabel}
        findStatusLabel={findStatusLabel}
        findKeyword={findKeyword}
        matchCount={matchCount}
        isReplaceMenuOpen={isReplaceMenuOpen}
        isBatchAnimating={isBatchAnimating}
        selectedCount={validSelectedCueIds.length}
        replaceMenuRef={replaceMenuRef}
        onFindTextChange={onFindTextChange}
        onReplaceTextChange={onReplaceTextChange}
        onPrevMatch={onPrevMatch}
        onNextMatch={onNextMatch}
        onToggleReplaceMenu={onToggleReplaceMenu}
        onReplaceOne={onReplaceOne}
        onReplaceAll={onReplaceAll}
        onAddCue={() => onAddCueAfter(primarySelectedCueId)}
        onMergeSelected={() => onMergeSelected(validSelectedCueIds)}
        onSplitSelected={() => {
          runSplitAnimation(orderedSelectedCueIds, onSplitSelected);
        }}
      />

      <SubtitleCueList
        canEdit={canEdit}
        cues={cues}
        cueWarningsById={cueWarningsById}
        emptyText={emptyText ?? (canEdit ? t("subtitles:editor.emptyEditable") : t("subtitles:editor.emptyReadOnly"))}
        editingCueId={editingCueId}
        selectedCueIds={validSelectedCueIds}
        timeErrorByCue={timeErrorByCue}
        listContainerRef={listContainerRef}
        cardRefs={cardRefs}
        renderHighlightedText={renderHighlightedText}
        onClearSelection={clearSelection}
        onCueClick={handleCueClick}
        onToggleEdit={(cueId) => {
          selectForEdit(cueId);
          setEditingCueId((old) => (old === cueId ? "" : cueId));
        }}
        onDeleteCue={onDeleteCue}
        onApplyStart={applyStart}
        onApplyEnd={applyEnd}
        onUpdateCue={onUpdateCue}
      />
    </div>
  );

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

  if (embedded) {
    return (
      <section className="subtitle-inline-root" role="region" aria-label={t("subtitles:editor.regionLabel")}>
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

