import { useMemo, useState } from "react";
import type { ExportSrtItem } from "../api/transcribe";

type SubtitleExportModalProps = {
  canExportTranslated: boolean;
  initialSelectedItems?: ExportSrtItem[];
  onClose: () => void;
  onConfirm: (items: ExportSrtItem[]) => void | Promise<void>;
};

type ExportOption = {
  item: ExportSrtItem;
  label: string;
  needsTranslation: boolean;
};

const EXPORT_OPTIONS: ExportOption[] = [
  {
    item: "source",
    label: "原文单语",
    needsTranslation: false,
  },
  {
    item: "target",
    label: "译文单语",
    needsTranslation: true,
  },
  {
    item: "bilingualSourceFirst",
    label: "双语（原文在上）",
    needsTranslation: true,
  },
  {
    item: "bilingualTargetFirst",
    label: "双语（译文在上）",
    needsTranslation: true,
  },
];

function emptySelection(): Record<ExportSrtItem, boolean> {
  return {
    source: false,
    target: false,
    bilingualSourceFirst: false,
    bilingualTargetFirst: false,
  };
}

function buildInitialSelection(
  canExportTranslated: boolean,
  initialSelectedItems?: ExportSrtItem[],
): Record<ExportSrtItem, boolean> {
  const preset = emptySelection();
  for (const item of initialSelectedItems ?? []) {
    preset[item] = true;
  }
  if (!canExportTranslated) {
    preset.target = false;
    preset.bilingualSourceFirst = false;
    preset.bilingualTargetFirst = false;
  }
  const enabledItems = EXPORT_OPTIONS
    .filter((option) => canExportTranslated || !option.needsTranslation)
    .map((option) => option.item);
  const hasEnabledSelected = enabledItems.some((item) => preset[item]);
  if (hasEnabledSelected) {
    return preset;
  }
  return {
    source: !canExportTranslated,
    target: canExportTranslated,
    bilingualSourceFirst: canExportTranslated,
    bilingualTargetFirst: false,
  };
}

export default function SubtitleExportModal({
  canExportTranslated,
  initialSelectedItems,
  onClose,
  onConfirm,
}: SubtitleExportModalProps) {
  const [selection, setSelection] = useState<Record<ExportSrtItem, boolean>>(
    buildInitialSelection(canExportTranslated, initialSelectedItems),
  );
  const selectedItems = useMemo(
    () =>
      EXPORT_OPTIONS
        .filter((option) => canExportTranslated || !option.needsTranslation)
        .map((option) => option.item)
        .filter((item) => selection[item]),
    [canExportTranslated, selection],
  );
  const selectedCount = selectedItems.length;

  return (
    <div className="modal-overlay" role="dialog" aria-modal="true">
      <div className="modal-content modal-content-export">
        <button className="modal-close" onClick={onClose} aria-label="关闭">
          ×
        </button>

        <h3 className="apple-heading-small">导出字幕</h3>
        <p className="apple-body-small export-modal-desc">
          请选择要导出的字幕类型。实际文件后缀会按任务源语言/目标语言生成。
        </p>

        <div className="export-option-list">
          {EXPORT_OPTIONS.map((option) => {
            const disabled = option.needsTranslation && !canExportTranslated;
            return (
              <label
                key={option.item}
                className={`export-option-card ${disabled ? "is-disabled" : ""}`}
              >
                <input
                  type="checkbox"
                  checked={selection[option.item]}
                  disabled={disabled}
                  onChange={(event) => {
                    const checked = event.target.checked;
                    setSelection((prev) => ({
                      ...prev,
                      [option.item]: checked,
                    }));
                  }}
                />
                <span className="export-option-main">
                  <span className="export-option-label">{option.label}</span>
                </span>
              </label>
            );
          })}
        </div>

        {!canExportTranslated ? (
          <p className="apple-body-small export-modal-tip">当前任务暂无译文，译文相关选项已禁用。</p>
        ) : null}

        <div className="export-modal-actions">
          <button type="button" className="apple-button apple-button-secondary" onClick={onClose}>
            取消
          </button>
          <button
            type="button"
            className="apple-button"
            disabled={selectedCount === 0}
            onClick={() => {
              void onConfirm(selectedItems);
            }}
          >
            导出 {selectedCount > 0 ? `${selectedCount} 项` : ""}
          </button>
        </div>
      </div>
    </div>
  );
}
