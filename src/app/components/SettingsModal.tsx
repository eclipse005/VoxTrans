import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { open } from "@tauri-apps/plugin-dialog";
import type { SubtitleBurnMode } from "../../features/media/types";
import { PROVIDER_OPTIONS } from "../../features/media/provider";
import { listSystemFonts } from "../api/system";
import { CheckIcon, CpuIcon, GpuIcon, UpdateIcon } from "./Icons";
import {
  MOSS_FIXED_CHUNK_SECONDS,
  asrUsesFixedChunk,
  isAsrModel,
} from "../../features/media/modelCatalog";
import { ModelCenter } from "./settings/ModelCenter";
import { ProviderPresetPicker } from "./settings/ProviderPresetPicker";
import { SubtitleStylePreview } from "./settings/SubtitleStylePreview";
import { SUBTITLE_STYLE_PRESETS } from "./settings/subtitleStylePresets";
import { useDialogA11y } from "./useDialogA11y";
import { useSettingsFormContext } from "../contexts/SettingsFormContext";
import {
  getActiveProfile,
  isProfileAtPresetDefaults,
  isProfileConfigured,
} from "../../features/media/llmProfiles";
import type { LlmModelInfo } from "../api/settings";

const SUBTITLE_LENGTH_PRESETS = [
  { id: "short", labelKey: "settings:subtitle.lengthShort" },
  { id: "standard", labelKey: "settings:subtitle.lengthStandard" },
  { id: "loose", labelKey: "settings:subtitle.lengthLoose" },
] as const;

const FLAT_SRT_OPTIONS: { item: SubtitleBurnMode; labelKey: string }[] = [
  { item: "source", labelKey: "settings:subtitle.flatSrtSource" },
  { item: "target", labelKey: "settings:subtitle.flatSrtTarget" },
  { item: "bilingualSourceFirst", labelKey: "settings:subtitle.flatSrtBilingualSourceFirst" },
  { item: "bilingualTargetFirst", labelKey: "settings:subtitle.flatSrtBilingualTargetFirst" },
];

type SettingsModalProps = {
  visible: boolean;
  onClose: () => void;
};

export default function SettingsModal({ visible, onClose }: SettingsModalProps) {
  const ctx = useSettingsFormContext();
  const { t } = useTranslation(["settings", "common"]);

  const [activeTab, setActiveTab] = useState<"transcribe" | "translate" | "subtitle" | "models">("transcribe");
  const [systemFonts, setSystemFonts] = useState<string[]>([]);
  const [chatModels, setChatModels] = useState<LlmModelInfo[]>([]);
  const [modelsLoading, setModelsLoading] = useState(false);
  const [modelPickerOpen, setModelPickerOpen] = useState(false);
  const [modelFilter, setModelFilter] = useState("");
  const modelPickerRef = useRef<HTMLDivElement | null>(null);
  /** Bumped on provider switch / new fetch so stale in-flight work cannot clear UI. */
  const fetchGenRef = useRef(0);
  const dialogRef = useDialogA11y(visible, onClose);

  const activeLlm = useMemo(
    () => getActiveProfile(ctx.form.llmProfiles, ctx.form.activeLlmProfileId),
    [ctx.form.llmProfiles, ctx.form.activeLlmProfileId],
  );
  const configuredProviderIds = useMemo(() => {
    const s = new Set<string>();
    for (const p of ctx.form.llmProfiles) {
      // Keyful: green when key set. Keyless (Ollama): ready without key.
      if (isProfileConfigured(p) && (p.requiresKey === false || p.apiKey?.trim())) {
        s.add(p.id);
      }
    }
    return s;
  }, [ctx.form.llmProfiles]);

  useEffect(() => {
    // Invalidate any in-flight fetch for the previous provider.
    fetchGenRef.current += 1;
    setModelsLoading(false);
    setChatModels([]);
    setModelPickerOpen(false);
    setModelFilter("");
  }, [ctx.form.activeLlmProfileId]);

  useEffect(() => {
    if (!modelPickerOpen) return;
    const onDoc = (e: MouseEvent) => {
      if (modelPickerRef.current?.contains(e.target as Node)) return;
      setModelPickerOpen(false);
      setModelFilter("");
    };
    document.addEventListener("mousedown", onDoc);
    return () => document.removeEventListener("mousedown", onDoc);
  }, [modelPickerOpen]);

  const filteredModels = chatModels.filter((m) =>
    !modelFilter.trim() ? true : m.id.toLowerCase().includes(modelFilter.trim().toLowerCase()),
  );

  const canFetchModels = Boolean(
    activeLlm.baseUrl?.trim() &&
      (activeLlm.requiresKey === false || activeLlm.apiKey?.trim()),
  );

  const handleFetchModels = async () => {
    if (!canFetchModels) return;
    // New generation: previous in-flight work must not touch list/loading.
    const gen = ++fetchGenRef.current;
    const profileId = ctx.form.activeLlmProfileId;
    setModelsLoading(true);
    try {
      const result = await ctx.fetchLlmModels();
      if (gen !== fetchGenRef.current) return;
      if (!result.ok) {
        // discarded / validation / empty / error — never apply [] onto the UI
        return;
      }
      // Only apply if this fetch still matches the slot it was started for.
      if (result.profileId !== profileId) return;
      setChatModels(result.models);
      if (result.models.length > 0) setModelPickerOpen(true);
    } finally {
      if (gen === fetchGenRef.current) setModelsLoading(false);
    }
  };

  useEffect(() => {
    if (!visible) return;
    let cancelled = false;
    void listSystemFonts()
      .then((items) => {
        if (cancelled) return;
        setSystemFonts(items);
      })
      .catch(() => {
        if (cancelled) return;
        setSystemFonts([]);
      });
    return () => {
      cancelled = true;
    };
  }, [visible]);

  if (!visible) return null;

  const tabIndex = activeTab === "transcribe"
    ? 0
    : activeTab === "translate"
      ? 1
      : activeTab === "subtitle"
        ? 2
        : 3;

  const subtitleLengthPresetIndex = SUBTITLE_LENGTH_PRESETS.findIndex(
    (preset) => preset.id === ctx.form.subtitleLengthPreset
  );

  const handleSubtitleLengthPresetChange = (preset: (typeof SUBTITLE_LENGTH_PRESETS)[number]) => {
    ctx.setForm((prev) => ({ ...prev, subtitleLengthPreset: preset.id }));
  };

  const handleChunkInputChange = (value: string) => {
    const digits = value.replace(/[^0-9]/g, "");
    if (!digits) {
      ctx.setForm((prev) => ({ ...prev, chunkInput: "" }));
      return;
    }
    const nextValue = Math.max(30, Math.min(180, Number.parseInt(digits, 10)));
    ctx.setForm((prev) => ({ ...prev, chunkInput: String(nextValue) }));
  };

  const handlePickModelsDir = async () => {
    const selected = await open({ directory: true, multiple: false, title: t("settings:models.storagePickerTitle") });
    if (selected && typeof selected === "string") {
      ctx.setForm((prev) => ({ ...prev, modelsDir: selected }));
    }
  };

  return (
    <div className="modal-overlay">
      <div
        ref={dialogRef}
        className="modal-content modal-content-settings"
        role="dialog"
        aria-modal="true"
        aria-labelledby="settings-modal-title"
        tabIndex={-1}
      >
        <button className="modal-close" onClick={onClose} aria-label={t("settings:modal.close")}>×</button>
        <div className="settings-header">
          <h3 id="settings-modal-title" className="apple-heading-medium">{t("settings:modal.title")}</h3>
        </div>
        <div className="settings-tabs-nav" style={{ ["--tab-index" as string]: tabIndex, ["--tab-count" as string]: 4 }}>
          <div className="settings-tab-indicator" />
          <button
            type="button"
            className={`settings-tab-btn ${activeTab === "transcribe" ? "active" : ""}`}
            onClick={() => setActiveTab("transcribe")}
          >
            {t("settings:tab.transcribe")}
          </button>
          <button
            type="button"
            className={`settings-tab-btn ${activeTab === "translate" ? "active" : ""}`}
            onClick={() => setActiveTab("translate")}
          >
            {t("settings:tab.translate")}
          </button>
          <button
            type="button"
            className={`settings-tab-btn ${activeTab === "subtitle" ? "active" : ""}`}
            onClick={() => setActiveTab("subtitle")}
          >
            {t("settings:tab.subtitle")}
          </button>
          <button
            type="button"
            className={`settings-tab-btn ${activeTab === "models" ? "active" : ""}`}
            onClick={() => setActiveTab("models")}
          >
            {t("settings:tab.models")}
          </button>
        </div>
        <div className="settings-body">
          <div className="settings-tab-content" hidden={activeTab !== "transcribe"}>
              <div className="settings-section">
                <h3 className="apple-heading-small">{t("settings:transcribe.params")}</h3>
                <div className="api-config-form">
                  <div className="form-row">
                    <div className="form-group">
                      <label>{t("settings:transcribe.device")}</label>
                      <div className="device-toggle-group" role="group" aria-label={t("settings:transcribe.device")}>
                        {PROVIDER_OPTIONS.map((option) => (
                          <button
                            key={option.id}
                            type="button"
                            className={`device-toggle-btn ${ctx.form.provider === option.id ? "active" : ""}`}
                            onClick={() => ctx.setForm((prev) => ({ ...prev, provider: option.id }))}
                            aria-pressed={ctx.form.provider === option.id}
                            title={option.title}
                          >
                            {option.kind === "cpu" ? <CpuIcon /> : <GpuIcon />}
                            <span>{option.label}</span>
                          </button>
                        ))}
                      </div>
                    </div>
                    <div className="form-group">
                      <label>{t("settings:transcribe.chunkDuration")}</label>
                      {isAsrModel(ctx.form.asrModel) && asrUsesFixedChunk(ctx.form.asrModel) ? (
                        <>
                          <div className="bounded-number-field is-disabled">
                            <input
                              className="apple-input"
                              type="text"
                              value={String(MOSS_FIXED_CHUNK_SECONDS)}
                              disabled
                              readOnly
                              aria-describedby="chunk-duration-moss-hint"
                            />
                            <span className="bounded-number-hint">
                              {t("settings:transcribe.chunkDurationMossFixed", {
                                seconds: MOSS_FIXED_CHUNK_SECONDS,
                              })}
                            </span>
                          </div>
                          <p id="chunk-duration-moss-hint" className="field-inline-hint">
                            {t("settings:transcribe.chunkDurationMossHint")}
                          </p>
                        </>
                      ) : (
                        <div className="bounded-number-field">
                          <input
                            className="apple-input"
                            type="number"
                            inputMode="numeric"
                            min={30}
                            max={180}
                            value={ctx.form.chunkInput}
                            onChange={(e) => handleChunkInputChange(e.target.value)}
                            placeholder="30 - 180"
                          />
                          <span className="bounded-number-hint">30-180</span>
                        </div>
                      )}
                    </div>
                  </div>
                  <label className="setting-toggle" htmlFor="enable-vocal-separation">
                    <input
                      id="enable-vocal-separation"
                      type="checkbox"
                      checked={ctx.form.enableVocalSeparation}
                      onChange={(e) => ctx.setForm((prev) => ({ ...prev, enableVocalSeparation: e.target.checked }))}
                    />
                    <div className="toggle-label">
                      <span className="toggle-title">{t("settings:transcribe.vocalSeparation")}</span>
                      <span className="toggle-desc">{t("settings:transcribe.vocalSeparationDesc")}</span>
                    </div>
                    <span className="toggle-switch" />
                  </label>
                  <label className="setting-toggle" htmlFor="enable-click-sound">
                    <input
                      id="enable-click-sound"
                      type="checkbox"
                      checked={ctx.form.enableClickSound}
                      onChange={(e) => ctx.setForm((prev) => ({ ...prev, enableClickSound: e.target.checked }))}
                    />
                    <div className="toggle-label">
                      <span className="toggle-title">{t("settings:transcribe.clickSound")}</span>
                      <span className="toggle-desc">{t("settings:transcribe.clickSoundDesc")}</span>
                    </div>
                    <span className="toggle-switch" />
                  </label>
                  <label className="setting-toggle" htmlFor="flat-srt-output">
                    <input
                      id="flat-srt-output"
                      type="checkbox"
                      checked={ctx.form.flatSrtOutput}
                      onChange={(e) => ctx.setForm((prev) => ({ ...prev, flatSrtOutput: e.target.checked }))}
                    />
                    <div className="toggle-label">
                      <span className="toggle-title">{t("settings:transcribe.flatSrtOutput")}</span>
                      <span className="toggle-desc">{t("settings:transcribe.flatSrtOutputDesc")}</span>
                    </div>
                    <span className="toggle-switch" />
                  </label>
                  {ctx.form.flatSrtOutput ? (
                    <div className="flat-srt-items-group">
                      <label className="flat-srt-items-label">{t("settings:transcribe.flatSrtItemsLabel")}</label>
                      <div className="flat-srt-items-options">
                        {FLAT_SRT_OPTIONS.map((option) => (
                          <label key={option.item} className="flat-srt-item-checkbox">
                            <input
                              type="checkbox"
                              checked={ctx.form.flatSrtItems.includes(option.item)}
                              onChange={(e) => {
                                const checked = e.target.checked;
                                ctx.setForm((prev) => ({
                                  ...prev,
                                  flatSrtItems: checked
                                    ? [...prev.flatSrtItems, option.item]
                                    : prev.flatSrtItems.filter((v) => v !== option.item),
                                }));
                              }}
                            />
                            <span>{t(option.labelKey)}</span>
                          </label>
                        ))}
                      </div>
                    </div>
                  ) : null}
                </div>
              </div>
            </div>
          <div className="settings-tab-content" hidden={activeTab !== "translate"}>
              <div className="settings-section">
                <div className="api-config-form">
                  <p className="llm-provider-section-hint">{t("settings:translate.providerHint")}</p>
                  <ProviderPresetPicker
                    selectedId={ctx.form.activeLlmProfileId}
                    activeModel={activeLlm.model}
                    configuredIds={configuredProviderIds}
                    atPresetDefaults={isProfileAtPresetDefaults(activeLlm)}
                    onSelect={ctx.selectLlmProvider}
                    onResetPreset={ctx.resetActiveLlmProfile}
                  />
                  <div className="form-row">
                    <div className="form-group">
                      <label>{t("settings:translate.apiKey")}</label>
                      <input
                        className="apple-input"
                        type="password"
                        value={activeLlm.apiKey}
                        onChange={(e) => ctx.updateActiveLlmProfile({ apiKey: e.target.value })}
                        placeholder={activeLlm.requiresKey === false ? "ollama" : "sk-..."}
                        autoComplete="off"
                      />
                    </div>
                    <div className="form-group">
                      <label>{t("settings:translate.baseUrl")}</label>
                      <input
                        className="apple-input"
                        value={activeLlm.baseUrl}
                        onChange={(e) => ctx.updateActiveLlmProfile({ baseUrl: e.target.value })}
                        placeholder="https://api.openai.com/v1"
                      />
                    </div>
                    <div className="form-group llm-model-field">
                      <div className="llm-model-label-row">
                        <label>{t("settings:translate.modelName")}</label>
                        <button
                          type="button"
                          className="llm-fetch-models-btn"
                          disabled={!canFetchModels || modelsLoading}
                          onClick={() => { void handleFetchModels(); }}
                        >
                          <UpdateIcon />
                          {modelsLoading
                            ? t("settings:translate.fetchingModels")
                            : t("settings:translate.fetchModels")}
                        </button>
                      </div>
                      <div className="llm-model-test-row" ref={modelPickerRef}>
                        <div className="llm-model-input-wrap">
                          <input
                            className="apple-input llm-model-input"
                            value={modelPickerOpen ? modelFilter : activeLlm.model}
                            onChange={(e) => {
                              if (modelPickerOpen) setModelFilter(e.target.value);
                              else ctx.updateActiveLlmProfile({ model: e.target.value });
                            }}
                            onFocus={() => {
                              if (chatModels.length > 0) {
                                setModelPickerOpen(true);
                                setModelFilter("");
                              }
                            }}
                            placeholder="deepseek-v4-flash"
                            autoComplete="off"
                          />
                          {modelPickerOpen && chatModels.length > 0 ? (
                            <div className="llm-model-dropdown" role="listbox">
                              {filteredModels.length === 0 ? (
                                <p className="llm-model-dropdown-empty">—</p>
                              ) : (
                                filteredModels.map((m) => {
                                  const active = m.id === activeLlm.model;
                                  return (
                                    <button
                                      key={m.id}
                                      type="button"
                                      role="option"
                                      aria-selected={active}
                                      className={`llm-model-option ${active ? "active" : ""}`}
                                      onMouseDown={(e) => e.preventDefault()}
                                      onClick={() => {
                                        ctx.updateActiveLlmProfile({ model: m.id });
                                        setModelPickerOpen(false);
                                        setModelFilter("");
                                      }}
                                    >
                                      <span className="llm-model-option-id">{m.id}</span>
                                      <span className="llm-model-option-kind">{m.kind}</span>
                                    </button>
                                  );
                                })
                              )}
                            </div>
                          ) : null}
                        </div>
                        <button
                          type="button"
                          className="nav-button llm-test-btn"
                          onClick={() => { void ctx.testTranslateConnection(); }}
                        >
                          {t("settings:translate.test")}
                        </button>
                      </div>
                    </div>
                    <div className="form-group">
                      <label>{t("settings:translate.concurrency")}</label>
                      <input
                        className="apple-input"
                        inputMode="numeric"
                        value={ctx.form.llmConcurrencyInput}
                        onChange={(e) => ctx.setForm((prev) => ({ ...prev, llmConcurrencyInput: e.target.value.replace(/[^0-9]/g, "") }))}
                        placeholder="1 - 16"
                      />
                    </div>
                  </div>
                  <div className="subtitle-toggle-row">
                    <label className="setting-toggle" htmlFor="enable-vision-assist">
                      <input
                        id="enable-vision-assist"
                        type="checkbox"
                        checked={ctx.form.enableVisionAssist}
                        onChange={(e) => ctx.setForm((prev) => ({ ...prev, enableVisionAssist: e.target.checked }))}
                      />
                      <div className="toggle-label">
                        <span className="toggle-title">{t("settings:translate.visionAssist")}</span>
                        <span className="toggle-desc">{t("settings:translate.visionAssistDesc")}</span>
                      </div>
                      <span className="toggle-switch" />
                    </label>
                  </div>
                </div>
              </div>
            </div>
          <div className="settings-tab-content" hidden={activeTab !== "subtitle"}>
              <div className="settings-section">
                <div className="api-config-form">
                  <div className="form-row">
                    <div className="form-group subtitle-length-group">
                      <div className="subtitle-length-card">
                        <span className="toggle-title">{t("settings:subtitle.length")}</span>
                        <div
                          className="subtitle-length-slider"
                          role="radiogroup"
                          aria-label={t("settings:subtitle.length")}
                          style={{ ["--subtitle-length-index" as string]: subtitleLengthPresetIndex }}
                        >
                          <span className="subtitle-length-slider-thumb" aria-hidden="true" />
                          {SUBTITLE_LENGTH_PRESETS.map((preset) => (
                            <button
                              key={preset.id}
                              type="button"
                              className={`subtitle-length-option ${ctx.form.subtitleLengthPreset === preset.id ? "active" : ""}`}
                              onClick={() => handleSubtitleLengthPresetChange(preset)}
                              role="radio"
                              aria-checked={ctx.form.subtitleLengthPreset === preset.id}
                            >
                              {t(preset.labelKey)}
                            </button>
                          ))}
                        </div>
                      </div>
                    </div>
                  </div>
                  <div className="subtitle-toggle-row">
                    <label className="setting-toggle" htmlFor="auto-burn-hard-subtitle">
                      <input
                        id="auto-burn-hard-subtitle"
                        type="checkbox"
                        checked={ctx.form.autoBurnHardSubtitle}
                        onChange={(e) => ctx.setForm((prev) => ({ ...prev, autoBurnHardSubtitle: e.target.checked }))}
                      />
                      <div className="toggle-label">
                        <span className="toggle-title">{t("settings:subtitle.autoBurn")}</span>
                        <span className="toggle-desc">{t("settings:subtitle.autoBurnDesc")}</span>
                      </div>
                      <span className="toggle-switch" />
                    </label>
                    <label className="setting-toggle" htmlFor="enable-subtitle-beautify">
                      <input
                        id="enable-subtitle-beautify"
                        type="checkbox"
                        checked={ctx.form.enableSubtitleBeautify}
                        onChange={(e) => ctx.setForm((prev) => ({ ...prev, enableSubtitleBeautify: e.target.checked }))}
                      />
                      <div className="toggle-label">
                        <span className="toggle-title">{t("settings:subtitle.beautify")}</span>
                        <span className="toggle-desc">{t("settings:subtitle.beautifyDesc")}</span>
                      </div>
                      <span className="toggle-switch" />
                    </label>
                  </div>
                  <div className="form-row">
                    <div className="form-group">
                      <label>{t("settings:subtitle.burnMode")}</label>
                      <select
                        className="apple-input"
                        value={ctx.form.subtitleBurnMode}
                        onChange={(e) => ctx.setForm((prev) => ({ ...prev, subtitleBurnMode: e.target.value as SubtitleBurnMode }))}
                      >
                        <option value="source">{t("settings:subtitle.burnModeSource")}</option>
                        <option value="target">{t("settings:subtitle.burnModeTarget")}</option>
                        <option value="bilingualSourceFirst">{t("settings:subtitle.burnModeBilingualSourceFirst")}</option>
                        <option value="bilingualTargetFirst">{t("settings:subtitle.burnModeBilingualTargetFirst")}</option>
                      </select>
                    </div>
                    <div className="form-group">
                      <label>{t("settings:subtitle.stylePreset")}</label>
                      <select
                        className="apple-input"
                        value=""
                        onChange={(e) => {
                          const preset = SUBTITLE_STYLE_PRESETS.find((p) => p.id === e.target.value);
                          if (preset) {
                            ctx.setForm((prev) => ({
                              ...prev,
                              subtitleRenderStyle: structuredClone(preset.style),
                            }));
                          }
                        }}
                      >
                        <option value="" disabled>{t("settings:subtitle.stylePresetPlaceholder")}</option>
                        {SUBTITLE_STYLE_PRESETS.map((preset) => (
                          <option key={preset.id} value={preset.id}>{t(preset.labelKey)}</option>
                        ))}
                      </select>
                    </div>
                  </div>
                </div>
              </div>
              <div className="settings-section">
                <h3 className="apple-heading-small">{t("settings:subtitle.style")}</h3>
                <div className="api-config-form">
                  <div className="form-row subtitle-style-grid">
                    <div className="form-group">
                      <label>{t("settings:subtitle.source.fontFamily")}</label>
                      <select
                        className="apple-input"
                        value={ctx.form.subtitleRenderStyle.source.fontFamily}
                        onChange={(e) => ctx.setForm((prev) => ({
                          ...prev,
                          subtitleRenderStyle: {
                            ...prev.subtitleRenderStyle,
                            source: { ...prev.subtitleRenderStyle.source, fontFamily: e.target.value },
                          },
                        }))}
                      >
                        <option value={ctx.form.subtitleRenderStyle.source.fontFamily}>
                          {ctx.form.subtitleRenderStyle.source.fontFamily}
                        </option>
                        {systemFonts
                          .filter((font) => font !== ctx.form.subtitleRenderStyle.source.fontFamily)
                          .map((font) => (
                            <option key={`source-${font}`} value={font}>{font}</option>
                          ))}
                      </select>
                    </div>
                    <div className="form-group">
                      <label>{t("settings:subtitle.source.fontSize")}</label>
                      <input
                        className="apple-input"
                        type="number"
                        min={16}
                        max={96}
                        value={ctx.form.subtitleRenderStyle.source.fontSize}
                        onChange={(e) => ctx.setForm((prev) => ({
                          ...prev,
                          subtitleRenderStyle: {
                            ...prev.subtitleRenderStyle,
                            source: { ...prev.subtitleRenderStyle.source, fontSize: Number.parseInt(e.target.value || "0", 10) || 44 },
                          },
                        }))}
                      />
                    </div>
                    <div className="form-group">
                      <label>{t("settings:subtitle.source.primaryColor")}</label>
                      <input
                        className="apple-input subtitle-color-input"
                        type="color"
                        value={ctx.form.subtitleRenderStyle.source.primaryColor}
                        onChange={(e) => ctx.setForm((prev) => ({
                          ...prev,
                          subtitleRenderStyle: {
                            ...prev.subtitleRenderStyle,
                            source: { ...prev.subtitleRenderStyle.source, primaryColor: e.target.value.toUpperCase() },
                          },
                        }))}
                      />
                    </div>
                    <div className="form-group">
                      <label>{t("settings:subtitle.source.shadow")}</label>
                      <input
                        className="apple-input"
                        type="number"
                        min={0}
                        max={8}
                        step={0.5}
                        value={ctx.form.subtitleRenderStyle.source.shadow}
                        onChange={(e) => ctx.setForm((prev) => ({
                          ...prev,
                          subtitleRenderStyle: {
                            ...prev.subtitleRenderStyle,
                            source: { ...prev.subtitleRenderStyle.source, shadow: Number.parseFloat(e.target.value || "0") || 0 },
                          },
                        }))}
                      />
                    </div>
                    <div className="form-group">
                      <label>{t("settings:subtitle.source.shadowColor")}</label>
                      <input
                        className="apple-input subtitle-color-input"
                        type="color"
                        value={ctx.form.subtitleRenderStyle.source.backColor}
                        onChange={(e) => ctx.setForm((prev) => ({
                          ...prev,
                          subtitleRenderStyle: {
                            ...prev.subtitleRenderStyle,
                            source: { ...prev.subtitleRenderStyle.source, backColor: e.target.value.toUpperCase() },
                          },
                        }))}
                      />
                    </div>
                    <div className="form-group">
                      <label>{t("settings:subtitle.source.borderStyle")}</label>
                      <select
                        className="apple-input"
                        value={ctx.form.subtitleRenderStyle.source.borderStyle}
                        onChange={(e) => ctx.setForm((prev) => ({
                          ...prev,
                          subtitleRenderStyle: {
                            ...prev.subtitleRenderStyle,
                            source: { ...prev.subtitleRenderStyle.source, borderStyle: e.target.value === "box" ? "box" : "outline" },
                          },
                        }))}
                      >
                        <option value="outline">{t("settings:subtitle.borderStyleOutline")}</option>
                        <option value="box">{t("settings:subtitle.borderStyleBox")}</option>
                      </select>
                    </div>
                    <div className="form-group">
                      <label>{t("settings:subtitle.source.outline")}</label>
                      <input
                        className="apple-input"
                        type="number"
                        min={0}
                        max={8}
                        step={0.5}
                        value={ctx.form.subtitleRenderStyle.source.outline}
                        onChange={(e) => ctx.setForm((prev) => ({
                          ...prev,
                          subtitleRenderStyle: {
                            ...prev.subtitleRenderStyle,
                            source: { ...prev.subtitleRenderStyle.source, outline: Number.parseFloat(e.target.value || "0") || 0 },
                          },
                        }))}
                      />
                    </div>
                    <div className="form-group">
                      <label>{t("settings:subtitle.source.outlineColor")}</label>
                      <input
                        className="apple-input subtitle-color-input"
                        type="color"
                        value={ctx.form.subtitleRenderStyle.source.outlineColor}
                        onChange={(e) => ctx.setForm((prev) => ({
                          ...prev,
                          subtitleRenderStyle: {
                            ...prev.subtitleRenderStyle,
                            source: { ...prev.subtitleRenderStyle.source, outlineColor: e.target.value.toUpperCase() },
                          },
                        }))}
                      />
                    </div>
                    <div className="form-group">
                      <label>{t("settings:subtitle.source.borderOpacity")}</label>
                      <input
                        className="apple-input"
                        type="number"
                        min={0}
                        max={100}
                        value={ctx.form.subtitleRenderStyle.source.borderOpacity}
                        onChange={(e) => ctx.setForm((prev) => ({
                          ...prev,
                          subtitleRenderStyle: {
                            ...prev.subtitleRenderStyle,
                            source: { ...prev.subtitleRenderStyle.source, borderOpacity: Number.parseInt(e.target.value || "0", 10) || 0 },
                          },
                        }))}
                      />
                    </div>
                    <div className="subtitle-style-divider" aria-hidden="true" />
                    <div className="subtitle-style-grid-break" aria-hidden="true" />
                    <div className="form-group">
                      <label>{t("settings:subtitle.target.fontFamily")}</label>
                      <select
                        className="apple-input"
                        value={ctx.form.subtitleRenderStyle.target.fontFamily}
                        onChange={(e) => ctx.setForm((prev) => ({
                          ...prev,
                          subtitleRenderStyle: {
                            ...prev.subtitleRenderStyle,
                            target: { ...prev.subtitleRenderStyle.target, fontFamily: e.target.value },
                          },
                        }))}
                      >
                        <option value={ctx.form.subtitleRenderStyle.target.fontFamily}>
                          {ctx.form.subtitleRenderStyle.target.fontFamily}
                        </option>
                        {systemFonts
                          .filter((font) => font !== ctx.form.subtitleRenderStyle.target.fontFamily)
                          .map((font) => (
                            <option key={`target-${font}`} value={font}>{font}</option>
                          ))}
                      </select>
                    </div>
                    <div className="form-group">
                      <label>{t("settings:subtitle.target.fontSize")}</label>
                      <input
                        className="apple-input"
                        type="number"
                        min={16}
                        max={96}
                        value={ctx.form.subtitleRenderStyle.target.fontSize}
                        onChange={(e) => ctx.setForm((prev) => ({
                          ...prev,
                          subtitleRenderStyle: {
                            ...prev.subtitleRenderStyle,
                            target: { ...prev.subtitleRenderStyle.target, fontSize: Number.parseInt(e.target.value || "0", 10) || 40 },
                          },
                        }))}
                      />
                    </div>
                    <div className="form-group">
                      <label>{t("settings:subtitle.target.primaryColor")}</label>
                      <input
                        className="apple-input subtitle-color-input"
                        type="color"
                        value={ctx.form.subtitleRenderStyle.target.primaryColor}
                        onChange={(e) => ctx.setForm((prev) => ({
                          ...prev,
                          subtitleRenderStyle: {
                            ...prev.subtitleRenderStyle,
                            target: { ...prev.subtitleRenderStyle.target, primaryColor: e.target.value.toUpperCase() },
                          },
                        }))}
                      />
                    </div>
                    <div className="form-group">
                      <label>{t("settings:subtitle.target.shadow")}</label>
                      <input
                        className="apple-input"
                        type="number"
                        min={0}
                        max={8}
                        step={0.5}
                        value={ctx.form.subtitleRenderStyle.target.shadow}
                        onChange={(e) => ctx.setForm((prev) => ({
                          ...prev,
                          subtitleRenderStyle: {
                            ...prev.subtitleRenderStyle,
                            target: { ...prev.subtitleRenderStyle.target, shadow: Number.parseFloat(e.target.value || "0") || 0 },
                          },
                        }))}
                      />
                    </div>
                    <div className="form-group">
                      <label>{t("settings:subtitle.target.shadowColor")}</label>
                      <input
                        className="apple-input subtitle-color-input"
                        type="color"
                        value={ctx.form.subtitleRenderStyle.target.backColor}
                        onChange={(e) => ctx.setForm((prev) => ({
                          ...prev,
                          subtitleRenderStyle: {
                            ...prev.subtitleRenderStyle,
                            target: { ...prev.subtitleRenderStyle.target, backColor: e.target.value.toUpperCase() },
                          },
                        }))}
                      />
                    </div>
                    <div className="form-group">
                      <label>{t("settings:subtitle.target.borderStyle")}</label>
                      <select
                        className="apple-input"
                        value={ctx.form.subtitleRenderStyle.target.borderStyle}
                        onChange={(e) => ctx.setForm((prev) => ({
                          ...prev,
                          subtitleRenderStyle: {
                            ...prev.subtitleRenderStyle,
                            target: { ...prev.subtitleRenderStyle.target, borderStyle: e.target.value === "box" ? "box" : "outline" },
                          },
                        }))}
                      >
                        <option value="outline">{t("settings:subtitle.borderStyleOutline")}</option>
                        <option value="box">{t("settings:subtitle.borderStyleBox")}</option>
                      </select>
                    </div>
                    <div className="form-group">
                      <label>{t("settings:subtitle.target.outline")}</label>
                      <input
                        className="apple-input"
                        type="number"
                        min={0}
                        max={8}
                        step={0.5}
                        value={ctx.form.subtitleRenderStyle.target.outline}
                        onChange={(e) => ctx.setForm((prev) => ({
                          ...prev,
                          subtitleRenderStyle: {
                            ...prev.subtitleRenderStyle,
                            target: { ...prev.subtitleRenderStyle.target, outline: Number.parseFloat(e.target.value || "0") || 0 },
                          },
                        }))}
                      />
                    </div>
                    <div className="form-group">
                      <label>{t("settings:subtitle.target.outlineColor")}</label>
                      <input
                        className="apple-input subtitle-color-input"
                        type="color"
                        value={ctx.form.subtitleRenderStyle.target.outlineColor}
                        onChange={(e) => ctx.setForm((prev) => ({
                          ...prev,
                          subtitleRenderStyle: {
                            ...prev.subtitleRenderStyle,
                            target: { ...prev.subtitleRenderStyle.target, outlineColor: e.target.value.toUpperCase() },
                          },
                        }))}
                      />
                    </div>
                    <div className="form-group">
                      <label>{t("settings:subtitle.target.borderOpacity")}</label>
                      <input
                        className="apple-input"
                        type="number"
                        min={0}
                        max={100}
                        value={ctx.form.subtitleRenderStyle.target.borderOpacity}
                        onChange={(e) => ctx.setForm((prev) => ({
                          ...prev,
                          subtitleRenderStyle: {
                            ...prev.subtitleRenderStyle,
                            target: { ...prev.subtitleRenderStyle.target, borderOpacity: Number.parseInt(e.target.value || "0", 10) || 0 },
                          },
                        }))}
                      />
                    </div>
                    <div className="subtitle-style-grid-break" aria-hidden="true" />
                    <div className="form-group">
                      <label>{t("settings:subtitle.layout.marginV")}</label>
                      <input
                        className="apple-input"
                        type="number"
                        min={0}
                        max={200}
                        value={ctx.form.subtitleRenderStyle.layout.marginV}
                        onChange={(e) => ctx.setForm((prev) => ({
                          ...prev,
                          subtitleRenderStyle: {
                            ...prev.subtitleRenderStyle,
                            layout: { ...prev.subtitleRenderStyle.layout, marginV: Number.parseInt(e.target.value || "0", 10) || 0 },
                          },
                        }))}
                      />
                    </div>
                    <div className="form-group">
                      <label>{t("settings:subtitle.layout.bilingualLineGap")}</label>
                      <input
                        className="apple-input"
                        type="number"
                        min={0}
                        max={140}
                        value={ctx.form.subtitleRenderStyle.layout.bilingualLineGap}
                        onChange={(e) => ctx.setForm((prev) => ({
                          ...prev,
                          subtitleRenderStyle: {
                            ...prev.subtitleRenderStyle,
                            layout: {
                              ...prev.subtitleRenderStyle.layout,
                              bilingualLineGap: e.target.value === "" ? 10 : Number.parseInt(e.target.value, 10),
                            },
                          },
                        }))}
                      />
                    </div>
                    <div className="form-group">
                      <label>{t("settings:subtitle.layout.alignment")}</label>
                      <select
                        className="apple-input"
                        value={ctx.form.subtitleRenderStyle.layout.alignment}
                        onChange={(e) => ctx.setForm((prev) => ({
                          ...prev,
                          subtitleRenderStyle: {
                            ...prev.subtitleRenderStyle,
                            layout: { ...prev.subtitleRenderStyle.layout, alignment: Number.parseInt(e.target.value, 10) as 1 | 2 | 3 },
                          },
                        }))}
                      >
                        <option value={1}>{t("settings:subtitle.alignmentLeft")}</option>
                        <option value={2}>{t("settings:subtitle.alignmentCenter")}</option>
                        <option value={3}>{t("settings:subtitle.alignmentRight")}</option>
                      </select>
                    </div>
                  </div>
                  <SubtitleStylePreview mode={ctx.form.subtitleBurnMode} style={ctx.form.subtitleRenderStyle} />
                </div>
              </div>
            </div>
          <div className="settings-tab-content" hidden={activeTab !== "models"}>
            <ModelCenter
              modelsDir={ctx.form.modelsDir}
              storageDefaultLabel={t("settings:models.storageDefault")}
              onPickModelsDir={handlePickModelsDir}
              onResetModelsDir={() => ctx.setForm((prev) => ({ ...prev, modelsDir: "" }))}
            />
          </div>
        </div>
        <div className="settings-footer">
          <button className="nav-button" onClick={ctx.saveSettings} title={t("settings:modal.save")} aria-label={t("settings:modal.save")}>
            <CheckIcon />
            <span>{t("common:button.save")}</span>
          </button>
        </div>
      </div>
    </div>
  );
}
