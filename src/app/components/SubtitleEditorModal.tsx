import { memo, useCallback, useEffect, useMemo, useRef, useState, type MouseEvent } from "react";
import { useTranslation } from "react-i18next";
import type { SubtitleCue } from "../../features/media/types";
import {
  diagnoseCues,
  groupDiagnosticsByCueId,
  summarizeDiagnostics,
  type DiagnosticKind,
} from "../../features/media/subtitleDiagnostics";
import { useSubtitleBatchAnimations } from "../hooks/useSubtitleBatchAnimations";
import { useSubtitleFindReplace } from "../hooks/useSubtitleFindReplace";
import { useSubtitleSelection } from "../hooks/useSubtitleSelection";
import { useSubtitleTimeValidation } from "../hooks/useSubtitleTimeValidation";
import SubtitleCueList from "./subtitle-editor/SubtitleCueList";
import SubtitleEditorHeader from "./subtitle-editor/SubtitleEditorHeader";
import SubtitleEditorToolbar from "./subtitle-editor/SubtitleEditorToolbar";
import SubtitleQualityBar from "./subtitle-editor/SubtitleQualityBar";

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
  /** Task target language, used to gate CJK-only text checks. */
  targetLang?: string;
  /** Runs the shared Rust beautify over the live draft; returns changed cues. */
  onApplyBeautify?: () => Promise<number>;
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

function SubtitleEditorModal({
  visible,
  embedded = false,
  canEdit,
  readOnlyReason = "",
  emptyText,
  taskName,
  cues,
  targetLang,
  onApplyBeautify,
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
    matchedCueIds,
    matchCount,
    onFindTextChange,
    onReplaceTextChange,
    onToggleReplaceMenu,
    onReplaceOne,
    onReplaceAll,
    onPrevMatch,
    onNextMatch,
    moveCursorToCue,
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
    commitStart,
    commitEnd,
    rejectStart,
    rejectEnd,
    clearTimeError,
  } = useSubtitleTimeValidation({
    onUpdateCue,
  });
  const {
    validSelectedCueIds,
    primarySelectedCueId,
    orderedSelectedCueIds,
    clearSelection,
    ensureSelected,
    handleCueClick,
  } = useSubtitleSelection({
    cueIds,
    onSelectedCueChanged: moveCursorToCue,
  });

  // Diagnostics are a pure function of the cue list, so the strip, the per-row
  // badges and the chip filters all read the same result.
  const diagnostics = useMemo(() => diagnoseCues(cues, targetLang), [cues, targetLang]);
  const summary = useMemo(() => summarizeDiagnostics(diagnostics), [diagnostics]);
  const diagnosticsByCueId = useMemo(
    () => groupDiagnosticsByCueId(diagnostics),
    [diagnostics],
  );
  const cueNumbers = useMemo(() => {
    const numbers: Record<string, number> = {};
    cues.forEach((cue, index) => {
      numbers[cue.id] = index + 1;
    });
    return numbers;
  }, [cues]);

  const [activeKind, setActiveKind] = useState<DiagnosticKind | null>(null);
  const [applying, setApplying] = useState(false);
  const [locatedCueId, setLocatedCueId] = useState<string | null>(null);

  // Derived, not synced: when a fix clears the last cue of the filtered kind
  // the chip disappears on its own, so the stored filter silently stops
  // applying instead of needing an effect to reset it.
  const effectiveKind =
    activeKind && summary.byKind.some((entry) => entry.kind === activeKind)
      ? activeKind
      : null;

  const visibleCues = useMemo(
    () =>
      effectiveKind
        ? cues.filter((cue) =>
            diagnostics.some(
              (diagnostic) => diagnostic.cueId === cue.id && diagnostic.kind === effectiveKind,
            ),
          )
        : cues,
    [effectiveKind, cues, diagnostics],
  );

  const locateCue = useCallback(
    (cueId: string) => {
      ensureSelected(cueId);
      const node = cardRefs.current[cueId];
      node?.scrollIntoView({ block: "center", behavior: "smooth" });
      setLocatedCueId(cueId);
    },
    [cardRefs, ensureSelected],
  );

  useEffect(() => {
    if (!locatedCueId) return;
    const timer = window.setTimeout(() => setLocatedCueId(null), 1200);
    return () => window.clearTimeout(timer);
  }, [locatedCueId]);

  const applyBeautify = useCallback(() => {
    if (!onApplyBeautify) return;
    setApplying(true);
    void onApplyBeautify()
      .catch(() => undefined)
      .finally(() => setApplying(false));
  }, [onApplyBeautify]);

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
        onBeautify={onApplyBeautify ? applyBeautify : undefined}
        beautifying={applying}
        onOpenLogs={onOpenLogs}
      />

      <SubtitleEditorToolbar        canEdit={canEdit}
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

      {summary.total > 0 ? (
        <SubtitleQualityBar
          diagnostics={diagnostics}
          summary={summary}
          canEdit={canEdit}
          applying={applying}
          activeKind={effectiveKind}
          onActiveKindChange={setActiveKind}
          onLocate={locateCue}
          onApply={applyBeautify}
        />
      ) : null}

      <SubtitleCueList
        canEdit={canEdit}
        cues={visibleCues}
        totalCount={cues.length}
        diagnosticsByCueId={diagnosticsByCueId}
        cueNumbers={cueNumbers}
        locatedCueId={locatedCueId}
        emptyText={emptyText ?? (canEdit ? t("subtitles:editor.emptyEditable") : t("subtitles:editor.emptyReadOnly"))}
        selectedCueIds={validSelectedCueIds}
        matchedCueIds={matchedCueIds}
        currentMatchCueId={currentMatch?.cueId ?? null}
        timeErrorByCue={timeErrorByCue}
        listContainerRef={listContainerRef}
        cardRefs={cardRefs}
        onClearSelection={clearSelection}
        onCueClick={handleCueClick}
        onEnsureSelected={ensureSelected}
        onDeleteCue={onDeleteCue}
        onCommitStart={commitStart}
        onCommitEnd={commitEnd}
        onRejectStart={rejectStart}
        onRejectEnd={rejectEnd}
        onClearTimeError={clearTimeError}
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

export default memo(SubtitleEditorModal);

