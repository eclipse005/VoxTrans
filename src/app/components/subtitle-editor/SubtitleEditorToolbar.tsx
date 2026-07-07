import type { RefObject } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDownIcon, ChevronLeftIcon, ChevronRightIcon, MergeIcon, PlusIcon, ReplaceIcon, SplitIcon } from "../Icons";

type SubtitleEditorToolbarProps = {
  canEdit: boolean;
  findText: string;
  replaceText: string;
  findCounterLabel: string;
  findStatusLabel: string;
  findKeyword: string;
  matchCount: number;
  isReplaceMenuOpen: boolean;
  isBatchAnimating: boolean;
  selectedCount: number;
  replaceMenuRef: RefObject<HTMLDivElement | null>;
  onFindTextChange: (value: string) => void;
  onReplaceTextChange: (value: string) => void;
  onPrevMatch: () => void;
  onNextMatch: () => void;
  onToggleReplaceMenu: () => void;
  onReplaceOne: () => void;
  onReplaceAll: () => void;
  onAddCue: () => void;
  onMergeSelected: () => void;
  onSplitSelected: () => void;
};

export default function SubtitleEditorToolbar({
  canEdit,
  findText,
  replaceText,
  findCounterLabel,
  findStatusLabel,
  findKeyword,
  matchCount,
  isReplaceMenuOpen,
  isBatchAnimating,
  selectedCount,
  replaceMenuRef,
  onFindTextChange,
  onReplaceTextChange,
  onPrevMatch,
  onNextMatch,
  onToggleReplaceMenu,
  onReplaceOne,
  onReplaceAll,
  onAddCue,
  onMergeSelected,
  onSplitSelected,
}: SubtitleEditorToolbarProps) {
  const { t } = useTranslation(["subtitles", "common"]);
  return (
    <div className="subtitle-editor-topbar">
      <div className="subtitle-toolbar-shell">
        <div className="subtitle-find-block subtitle-find-replace-inline">
          <div className="subtitle-find-replace subtitle-find-shell">
            <input
              className="apple-input subtitle-find-input"
              value={findText}
              onChange={(e) => onFindTextChange(e.target.value)}
              placeholder={t("subtitles:toolbar.findPlaceholder")}
            />
            <input
              className="apple-input subtitle-find-input"
              value={replaceText}
              onChange={(e) => onReplaceTextChange(e.target.value)}
              placeholder={t("subtitles:toolbar.replacePlaceholder")}
              disabled={!canEdit}
            />
            <div className="subtitle-find-nav" role="group" aria-label={t("subtitles:toolbar.findNavAriaLabel")}>
              <button
                className="subtitle-find-nav-btn"
                type="button"
                onClick={onPrevMatch}
                disabled={!findKeyword || matchCount === 0}
                aria-label={t("subtitles:toolbar.prevMatch")}
                title={t("subtitles:toolbar.prevMatch")}
              >
                <ChevronLeftIcon />
              </button>
              <span className="subtitle-find-count" aria-live="polite">{findCounterLabel}</span>
              <button
                className="subtitle-find-nav-btn"
                type="button"
                onClick={onNextMatch}
                disabled={!findKeyword || matchCount === 0}
                aria-label={t("subtitles:toolbar.nextMatch")}
                title={t("subtitles:toolbar.nextMatch")}
              >
                <ChevronRightIcon />
              </button>
            </div>
            <div className="subtitle-find-split" ref={replaceMenuRef}>
              <button
                className="apple-button apple-button-secondary subtitle-find-text-btn subtitle-find-primary-btn subtitle-find-split-main"
                onClick={onReplaceOne}
                title={t("subtitles:toolbar.replaceAndAdvance")}
                aria-label={t("subtitles:toolbar.replaceAndAdvance")}
                disabled={!canEdit || !findKeyword}
              >
                {t("subtitles:toolbar.replace")}
              </button>
              <button
                className="apple-button apple-button-secondary subtitle-find-text-btn subtitle-find-split-toggle"
                type="button"
                onClick={onToggleReplaceMenu}
                aria-label={t("subtitles:toolbar.openReplaceMenu")}
                aria-expanded={isReplaceMenuOpen}
                disabled={!canEdit || !findKeyword}
              >
                <ChevronDownIcon />
              </button>
              {isReplaceMenuOpen ? (
                <div className="subtitle-find-split-menu" role="menu" aria-label={t("subtitles:toolbar.replaceMenuAriaLabel")}>
                  <button
                    type="button"
                    className="subtitle-find-split-menu-item"
                    role="menuitem"
                    onClick={onReplaceAll}
                    disabled={!canEdit}
                  >
                    <ReplaceIcon />
                    {t("subtitles:toolbar.replaceAll")}
                  </button>
                </div>
              ) : null}
            </div>
            {findStatusLabel ? <span className="subtitle-find-status">{findStatusLabel}</span> : null}
          </div>
        </div>

        <div className="subtitle-row-actions subtitle-batch-actions">
          <button
            className="nav-button subtitle-batch-btn"
            onClick={onAddCue}
            disabled={!canEdit || isBatchAnimating}
          >
            <PlusIcon />
            {t("subtitles:toolbar.addCue")}
          </button>
          <button
            className="nav-button subtitle-batch-btn"
            disabled={!canEdit || selectedCount < 2 || isBatchAnimating}
            onClick={onMergeSelected}
            title={selectedCount >= 2 ? t("subtitles:toolbar.mergeActive", { count: selectedCount }) : t("subtitles:toolbar.mergeDisabled")}
          >
            <MergeIcon />
            {selectedCount >= 2 ? t("subtitles:toolbar.mergeCount", { count: selectedCount }) : t("subtitles:toolbar.merge")}
          </button>
          <button
            className="nav-button subtitle-batch-btn"
            disabled={!canEdit || selectedCount < 1 || isBatchAnimating}
            onClick={onSplitSelected}
            title={selectedCount >= 1 ? t("subtitles:toolbar.splitActive", { count: selectedCount }) : t("subtitles:toolbar.splitDisabled")}
          >
            <SplitIcon />
            {selectedCount >= 1 ? t("subtitles:toolbar.splitCount", { count: selectedCount }) : t("subtitles:toolbar.split")}
          </button>
        </div>
      </div>
    </div>
  );
}
