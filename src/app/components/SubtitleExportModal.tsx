import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import type { ExportSrtItem } from "../api/transcribe";

type SubtitleExportModalProps = {
  canExportTranslated: boolean;
  initialSelectedItems?: ExportSrtItem[];
  onClose: () => void;
  onConfirm: (items: ExportSrtItem[]) => void | Promise<void>;
};

type ExportOption = {
  item: ExportSrtItem;
  labelKey: string;
  needsTranslation: boolean;
};

const EXPORT_OPTIONS: ExportOption[] = [
  {
    item: "source",
    labelKey: "subtitles.export.sourceMono",
    needsTranslation: false,
  },
  {
    item: "target",
    labelKey: "subtitles.export.targetMono",
    needsTranslation: true,
  },
  {
    item: "bilingualSourceFirst",
    labelKey: "subtitles.export.bilingualSourceFirst",
    needsTranslation: true,
  },
  {
    item: "bilingualTargetFirst",
    labelKey: "subtitles.export.bilingualTargetFirst",
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
  const { t } = useTranslation(["subtitles", "common"]);
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
        <button className="modal-close" onClick={onClose} aria-label={t("common:button.close")}>
          ×
        </button>

        <h3 className="apple-heading-small">{t("subtitles.export.title")}</h3>
        <p className="apple-body-small export-modal-desc">
          {t("subtitles.export.description")}
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
                  <span className="export-option-label">{t(option.labelKey)}</span>
                </span>
              </label>
            );
          })}
        </div>

        {!canExportTranslated ? (
          <p className="apple-body-small export-modal-tip">{t("subtitles.export.noTranslationTip")}</p>
        ) : null}

        <div className="export-modal-actions">
          <button type="button" className="apple-button apple-button-secondary" onClick={onClose}>
            {t("common:button.cancel")}
          </button>
          <button
            type="button"
            className="apple-button"
            disabled={selectedCount === 0}
            onClick={() => {
              void onConfirm(selectedItems);
            }}
          >
            {t("subtitles.export.button")} {selectedCount > 0 ? t("subtitles.export.itemCount", { count: selectedCount }) : ""}
          </button>
        </div>
      </div>
    </div>
  );
}
