import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import {
  LLM_PROVIDER_PRESETS,
  getProviderById,
  type LlmProviderId,
} from "../../../features/media/llmProviders";
import { CheckIcon } from "../Icons";

type ProviderPresetPickerProps = {
  selectedId: LlmProviderId | string;
  activeModel?: string;
  configuredIds?: Set<string>;
  /** When true, URL/model already match catalog — disable reset. */
  atPresetDefaults?: boolean;
  onSelect: (id: LlmProviderId) => void;
  /** Restore catalog baseUrl/model for the current slot only (keeps key). */
  onResetPreset?: () => void;
};

async function openExternalUrl(url: string) {
  try {
    await invoke("open_external_url", { url });
  } catch (err) {
    console.error("failed to open external url:", url, err);
  }
}

export function ProviderPresetPicker({
  selectedId,
  activeModel,
  configuredIds,
  atPresetDefaults = false,
  onSelect,
  onResetPreset,
}: ProviderPresetPickerProps) {
  const { t } = useTranslation(["settings"]);
  const selected = getProviderById(selectedId);
  const modelLabel = (activeModel || selected.model || "").trim();

  return (
    <div className="llm-provider-picker">
      <div className="llm-provider-picker-head">
        <label className="llm-provider-label">{t("settings:translate.provider")}</label>
        <div className="llm-provider-head-actions">
          {selected.keyUrl ? (
            <a
              href={selected.keyUrl}
              className="llm-provider-key-link"
              onClick={(e) => {
                e.preventDefault();
                void openExternalUrl(selected.keyUrl!);
              }}
            >
              {t("settings:translate.getKey", { name: selected.shortName })}
            </a>
          ) : null}
          {onResetPreset ? (
            <button
              type="button"
              className="llm-provider-reset-btn"
              disabled={atPresetDefaults}
              title={t("settings:translate.resetPresetTitle")}
              onClick={() => onResetPreset()}
            >
              {t("settings:translate.resetPreset")}
            </button>
          ) : null}
        </div>
      </div>

      <div className="llm-provider-grid" role="listbox" aria-label={t("settings:translate.provider")}>
        {LLM_PROVIDER_PRESETS.map((preset) => {
          const isSelected = selectedId === preset.id;
          const hasKey = configuredIds?.has(preset.id);
          return (
            <button
              key={preset.id}
              type="button"
              role="option"
              aria-selected={isSelected}
              onClick={() => onSelect(preset.id)}
              title={`${preset.name}${preset.hint ? ` · ${preset.hint}` : ""}${hasKey ? ` · ${t("settings:translate.configured")}` : ""}`}
              className={`llm-provider-card ${isSelected ? "selected" : ""}`}
            >
              {preset.badge ? (
                <span
                  className={`llm-provider-badge ${
                    preset.badgeTone === "recommend"
                      ? "recommend"
                      : preset.badgeTone === "free"
                        ? "free"
                        : ""
                  }`}
                >
                  {preset.badge}
                </span>
              ) : null}
              {preset.iconSrc ? (
                <span className={`llm-provider-icon-wrap${preset.iconMono ? " is-mono" : ""}`}>
                  <img
                    src={preset.iconSrc}
                    alt=""
                    className="llm-provider-icon"
                    draggable={false}
                  />
                </span>
              ) : (
                <span className="llm-provider-icon-wrap llm-provider-icon-fallback" aria-hidden>
                  ⚙
                </span>
              )}
              <span className={`llm-provider-short ${isSelected ? "active" : ""}`}>
                {preset.shortName}
              </span>
              {isSelected ? (
                <span className="llm-provider-check" aria-hidden>
                  <CheckIcon />
                </span>
              ) : hasKey ? (
                <span className="llm-provider-dot" aria-hidden />
              ) : null}
            </button>
          );
        })}
      </div>

      {(selected.hint || modelLabel) && (
        <p className="llm-provider-hint">
          {selected.name}
          {selected.hint ? ` · ${selected.hint}` : ""}
          {modelLabel ? ` · ${modelLabel}` : ""}
        </p>
      )}
    </div>
  );
}
