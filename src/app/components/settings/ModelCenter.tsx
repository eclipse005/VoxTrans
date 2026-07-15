import { useTranslation } from "react-i18next";
import {
  ASR_CATALOG,
  SUPPORT_CATALOG,
  tagLabelKey,
} from "../../../features/media/modelCatalog";
import type { AsrModel, ModelStatusResponse } from "../../../features/media/types";
import { useSettingsFormContext } from "../../contexts/SettingsFormContext";
import { ModelActions } from "./ModelActions";
import { isModelReady } from "./modelStatusUtils";

type ModelCenterProps = {
  modelsDir: string;
  storageDefaultLabel: string;
  onPickModelsDir: () => void | Promise<void>;
  onResetModelsDir: () => void;
};

export function ModelCenter({
  modelsDir,
  storageDefaultLabel,
  onPickModelsDir,
  onResetModelsDir,
}: ModelCenterProps) {
  const { t } = useTranslation(["settings", "models", "common"]);
  const ctx = useSettingsFormContext();
  const selectedAsr = ctx.form.asrModel;

  const selectedStatus = ctx.asrStatusByModel[selectedAsr] ?? ctx.asrStatus;
  const selectedReady = isModelReady(selectedStatus);

  const handleSelectAsr = (id: AsrModel) => {
    ctx.setForm((prev) => ({ ...prev, asrModel: id }));
  };

  return (
    <div className="model-center-content">
      <section className="model-section model-dir-card" aria-label={t("models:section.storage")}>
        <span className="model-dir-icon" aria-hidden="true">
          📁
        </span>
        <div className="model-dir-main">
          <span className="model-section-label">{t("models:section.storage")}</span>
          <span className="model-dir-path">{modelsDir || storageDefaultLabel}</span>
        </div>
        <div className="model-dir-actions">
          <button
            type="button"
            className="nav-button model-dir-btn"
            onClick={() => {
              void onPickModelsDir();
            }}
          >
            {t("settings:models.storageChange")}
          </button>
          {modelsDir ? (
            <button type="button" className="nav-button model-dir-btn" onClick={onResetModelsDir}>
              {t("settings:models.storageReset")}
            </button>
          ) : null}
        </div>
      </section>

      <section className="model-section model-panel" aria-labelledby="model-asr-heading">
        <header className="model-panel-head">
          <div className="model-panel-titles">
            <h3 id="model-asr-heading" className="apple-heading-small">
              {t("models:section.asr")}
            </h3>
          </div>
          <div className="model-current-pill" title={selectedAsr}>
            <span className="model-current-label">{t("models:section.currentAsr")}</span>
            <span className="model-current-value">{selectedAsr}</span>
            <span
              className={`model-ready-pill ${selectedReady ? "ready" : "not-ready"}`}
            >
              {selectedReady ? t("models:card.ready") : t("models:card.notReady")}
            </span>
          </div>
        </header>

        <div className="model-asr-list" role="radiogroup" aria-label={t("models:section.asr")}>
          {ASR_CATALOG.map((entry) => {
            const selected = selectedAsr === entry.id;
            const status = ctx.asrStatusByModel[entry.id] ?? null;
            const ready = isModelReady(status);
            return (
              <div
                key={entry.id}
                className={`model-asr-row ${selected ? "is-selected" : ""} ${ready ? "is-ready" : ""}`}
              >
                <button
                  type="button"
                  className="model-asr-select"
                  role="radio"
                  aria-checked={selected}
                  onClick={() => handleSelectAsr(entry.id)}
                >
                  <span className={`model-radio-dot ${selected ? "active" : ""}`} aria-hidden="true" />
                  <span className="model-asr-body">
                    <span className="model-asr-name-row">
                      <span className="model-asr-name">{entry.id}</span>
                      <span className={`model-ready-pill compact ${ready ? "ready" : "not-ready"}`}>
                        {ready ? t("models:card.ready") : t("models:card.notReady")}
                      </span>
                    </span>
                    <span className="model-tag-row">
                      {entry.tagIds.map((tag) => (
                        <span key={tag} className={`model-tag model-tag-${tag}`}>
                          {t(tagLabelKey(tag))}
                        </span>
                      ))}
                    </span>
                    <span className="model-asr-desc apple-body-small">{t(entry.descKey)}</span>
                  </span>
                </button>
                <ModelActions
                  target="asr"
                  modelName={entry.id}
                  status={status}
                  onOpenModelDir={ctx.openModelDir}
                  onStartModelDownload={ctx.startModelDownload}
                  onCancelModelDownload={ctx.cancelModelDownload}
                />
              </div>
            );
          })}
        </div>
      </section>

      <section className="model-section model-panel" aria-labelledby="model-support-heading">
        <header className="model-panel-head">
          <div className="model-panel-titles">
            <h3 id="model-support-heading" className="apple-heading-small">
              {t("models:section.support")}
            </h3>
          </div>
        </header>

        <div className="model-support-list">
          {SUPPORT_CATALOG.map((entry) => {
            const status: ModelStatusResponse | null =
              entry.target === "align" ? ctx.alignStatus : ctx.demucsStatus;
            const ready = isModelReady(status);
            return (
              <div key={entry.id} className={`model-support-row ${ready ? "is-ready" : ""}`}>
                <div className="model-support-body">
                  <div className="model-asr-name-row">
                    <span className="model-asr-name">{entry.id}</span>
                    <span className="model-role-chip">{t(entry.roleKey)}</span>
                    <span className={`model-ready-pill compact ${ready ? "ready" : "not-ready"}`}>
                      {ready ? t("models:card.ready") : t("models:card.notReady")}
                    </span>
                  </div>
                  <p className="model-asr-desc apple-body-small">{t(entry.descKey)}</p>
                </div>
                <ModelActions
                  target={entry.target}
                  modelName={entry.id}
                  status={status}
                  onOpenModelDir={ctx.openModelDir}
                  onStartModelDownload={ctx.startModelDownload}
                  onCancelModelDownload={ctx.cancelModelDownload}
                />
              </div>
            );
          })}
        </div>
      </section>
    </div>
  );
}
