import type { RefObject } from "react";
import { ChevronDownIcon, ChevronLeftIcon, ChevronRightIcon, ReplaceIcon } from "../Icons";

type SubtitleEditorToolbarProps = {
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
  return (
    <div className="subtitle-editor-topbar">
      <div className="subtitle-toolbar-shell">
        <div className="subtitle-find-block subtitle-find-replace-inline">
          <div className="subtitle-find-replace subtitle-find-shell">
            <input
              className="apple-input subtitle-find-input"
              value={findText}
              onChange={(e) => onFindTextChange(e.target.value)}
              placeholder="查找文本"
            />
            <input
              className="apple-input subtitle-find-input"
              value={replaceText}
              onChange={(e) => onReplaceTextChange(e.target.value)}
              placeholder="替换为"
            />
            <div className="subtitle-find-nav" role="group" aria-label="查找匹配导航">
              <button
                className="subtitle-find-nav-btn"
                type="button"
                onClick={onPrevMatch}
                disabled={!findKeyword || matchCount === 0}
                aria-label="上一条匹配"
                title="上一条匹配"
              >
                <ChevronLeftIcon />
              </button>
              <span className="subtitle-find-count" aria-live="polite">{findCounterLabel}</span>
              <button
                className="subtitle-find-nav-btn"
                type="button"
                onClick={onNextMatch}
                disabled={!findKeyword || matchCount === 0}
                aria-label="下一条匹配"
                title="下一条匹配"
              >
                <ChevronRightIcon />
              </button>
            </div>
            <div className="subtitle-find-split" ref={replaceMenuRef}>
              <button
                className="apple-button apple-button-secondary subtitle-find-text-btn subtitle-find-primary-btn subtitle-find-split-main"
                onClick={onReplaceOne}
                title="替换当前命中并跳到下一条"
                aria-label="替换当前命中并跳到下一条"
                disabled={!findKeyword}
              >
                替换
              </button>
              <button
                className="apple-button apple-button-secondary subtitle-find-text-btn subtitle-find-split-toggle"
                type="button"
                onClick={onToggleReplaceMenu}
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
                    onClick={onReplaceAll}
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
            onClick={onAddCue}
            disabled={isBatchAnimating}
          >
            新增字幕段
          </button>
          <button
            className="apple-button apple-button-secondary subtitle-mini-btn"
            disabled={selectedCount < 2 || isBatchAnimating}
            onClick={onMergeSelected}
            title={selectedCount >= 2 ? `合并 ${selectedCount} 条` : "请选择至少两条字幕"}
          >
            {selectedCount >= 2 ? `合并(${selectedCount})` : "合并"}
          </button>
          <button
            className="apple-button apple-button-secondary subtitle-mini-btn"
            disabled={selectedCount < 1 || isBatchAnimating}
            onClick={onSplitSelected}
            title={selectedCount >= 1 ? `拆分 ${selectedCount} 条` : "请选择字幕"}
          >
            {selectedCount >= 1 ? `拆分(${selectedCount})` : "拆分"}
          </button>
        </div>
      </div>
    </div>
  );
}
