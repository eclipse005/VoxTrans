import { useEffect, useState } from "react";
import type {
  AsrModel,
  DemucsModel,
  ModelStatusResponse,
  Provider,
  SubtitleBurnMode,
  SubtitleRenderStyle,
} from "../../features/media/types";
import { PROVIDER_OPTIONS } from "../../features/media/provider";
import { listSystemFonts } from "../api/system";
import { CheckIcon, CpuIcon, GpuIcon } from "./Icons";
import { ModelDownloadCard } from "./settings/ModelDownloadCard";
import { SubtitleStylePreview } from "./settings/SubtitleStylePreview";
import { useDialogA11y } from "./useDialogA11y";

type SettingsModalProps = {
  visible: boolean;
  draftProvider: Provider;
  draftChunkInput: string;
  draftSubtitleMaxWordsInput: string;
  draftSubtitleLengthReferenceInput: string;
  draftAsrModel: AsrModel;
  draftDemucsModel: DemucsModel;
  draftEnableVocalSeparation: boolean;
  draftTranslateApiKey: string;
  draftTranslateBaseUrl: string;
  draftTranslateModel: string;
  draftLlmConcurrencyInput: string;
  draftEnableTerminology: boolean;
  draftEnableSubtitleBeautify: boolean;
  draftAutoBurnHardSubtitle: boolean;
  draftSubtitleBurnMode: SubtitleBurnMode;
  draftSubtitleRenderStyle: SubtitleRenderStyle;
  asrStatus: ModelStatusResponse | null;
  demucsStatus: ModelStatusResponse | null;
  onClose: () => void;
  onSave: () => void | Promise<void>;
  onDraftProviderChange: (value: Provider) => void;
  onDraftChunkInputChange: (value: string) => void;
  onDraftSubtitleMaxWordsInputChange: (value: string) => void;
  onDraftSubtitleLengthReferenceInputChange: (value: string) => void;
  onDraftAsrModelChange: (value: AsrModel) => void;
  onDraftDemucsModelChange: (value: DemucsModel) => void;
  onDraftEnableVocalSeparationChange: (value: boolean) => void;
  onDraftTranslateApiKeyChange: (value: string) => void;
  onDraftTranslateBaseUrlChange: (value: string) => void;
  onDraftTranslateModelChange: (value: string) => void;
  onDraftLlmConcurrencyInputChange: (value: string) => void;
  onDraftEnableTerminologyChange: (value: boolean) => void;
  onDraftEnableSubtitleBeautifyChange: (value: boolean) => void;
  onDraftAutoBurnHardSubtitleChange: (value: boolean) => void;
  onDraftSubtitleBurnModeChange: (value: SubtitleBurnMode) => void;
  onDraftSubtitleRenderStyleChange: (value: SubtitleRenderStyle) => void;
  onTestTranslateConnection: () => void | Promise<void>;
  onOpenModelDir: (target: "asr" | "demucs") => void | Promise<void>;
  onStartModelDownload: (target: "asr" | "demucs") => void | Promise<void>;
  onCancelModelDownload: (target: "asr" | "demucs") => void | Promise<void>;
};

export default function SettingsModal(props: SettingsModalProps) {
  const {
    visible,
    draftProvider,
    draftChunkInput,
    draftSubtitleMaxWordsInput,
    draftSubtitleLengthReferenceInput,
    draftAsrModel,
    draftDemucsModel,
    draftEnableVocalSeparation,
    draftTranslateApiKey,
    draftTranslateBaseUrl,
    draftTranslateModel,
    draftLlmConcurrencyInput,
    draftEnableTerminology,
    draftEnableSubtitleBeautify,
    draftAutoBurnHardSubtitle,
    draftSubtitleBurnMode,
    draftSubtitleRenderStyle,
    asrStatus,
    demucsStatus,
    onClose,
    onSave,
    onDraftProviderChange,
    onDraftChunkInputChange,
    onDraftSubtitleMaxWordsInputChange,
    onDraftSubtitleLengthReferenceInputChange,
    onDraftAsrModelChange,
    onDraftDemucsModelChange,
    onDraftEnableVocalSeparationChange,
    onDraftTranslateApiKeyChange,
    onDraftTranslateBaseUrlChange,
    onDraftTranslateModelChange,
    onDraftLlmConcurrencyInputChange,
    onDraftEnableTerminologyChange,
    onDraftEnableSubtitleBeautifyChange,
    onDraftAutoBurnHardSubtitleChange,
    onDraftSubtitleBurnModeChange,
    onDraftSubtitleRenderStyleChange,
    onTestTranslateConnection,
    onOpenModelDir,
    onStartModelDownload,
    onCancelModelDownload,
  } = props;

  const [activeTab, setActiveTab] = useState<"transcribe" | "translate" | "subtitle" | "models">("transcribe");
  const [systemFonts, setSystemFonts] = useState<string[]>([]);
  const dialogRef = useDialogA11y(visible, onClose);

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
        <button className="modal-close" onClick={onClose} aria-label="关闭设置">×</button>
        <div className="settings-header">
          <h3 id="settings-modal-title" className="apple-heading-medium">设置</h3>
        </div>
        <div className="settings-tabs-nav" style={{ ["--tab-index" as string]: tabIndex, ["--tab-count" as string]: 4 }}>
          <div className="settings-tab-indicator" />
          <button
            type="button"
            className={`settings-tab-btn ${activeTab === "transcribe" ? "active" : ""}`}
            onClick={() => setActiveTab("transcribe")}
          >
            转录
          </button>
          <button
            type="button"
            className={`settings-tab-btn ${activeTab === "translate" ? "active" : ""}`}
            onClick={() => setActiveTab("translate")}
          >
            翻译
          </button>
          <button
            type="button"
            className={`settings-tab-btn ${activeTab === "subtitle" ? "active" : ""}`}
            onClick={() => setActiveTab("subtitle")}
          >
            字幕
          </button>
          <button
            type="button"
            className={`settings-tab-btn ${activeTab === "models" ? "active" : ""}`}
            onClick={() => setActiveTab("models")}
          >
            模型
          </button>
        </div>
        <div className="settings-body">
          {activeTab === "transcribe" ? (
            <div className="settings-tab-content">
              <div className="settings-section">
                <h3 className="apple-heading-small">转录参数</h3>
                <div className="api-config-form">
                  <div className="form-row">
                    <div className="form-group">
                      <label>执行设备</label>
                      <div className="device-toggle-group" role="group" aria-label="执行设备">
                        {PROVIDER_OPTIONS.map((option) => (
                          <button
                            key={option.id}
                            type="button"
                            className={`device-toggle-btn ${draftProvider === option.id ? "active" : ""}`}
                            onClick={() => onDraftProviderChange(option.id)}
                            aria-pressed={draftProvider === option.id}
                            title={option.title}
                          >
                            {option.kind === "cpu" ? <CpuIcon /> : <GpuIcon />}
                            <span>{option.label}</span>
                          </button>
                        ))}
                      </div>
                    </div>
                    <div className="form-group">
                      <label>分段时长（秒）</label>
                      <input
                        className="apple-input"
                        inputMode="numeric"
                        value={draftChunkInput}
                        onChange={(e) => onDraftChunkInputChange(e.target.value.replace(/[^0-9]/g, ""))}
                        placeholder="建议：4G=60秒，8G=180秒"
                      />
                    </div>
                  </div>
                  <label className="setting-toggle" htmlFor="enable-vocal-separation">
                    <input
                      id="enable-vocal-separation"
                      type="checkbox"
                      checked={draftEnableVocalSeparation}
                      onChange={(e) => onDraftEnableVocalSeparationChange(e.target.checked)}
                    />
                    <div className="toggle-label">
                      <span className="toggle-title">人声分离</span>
                      <span className="toggle-desc">背景吵杂时请使用，提高转录准确率</span>
                    </div>
                    <span className="toggle-switch" />
                  </label>
                </div>
              </div>
            </div>
          ) : activeTab === "translate" ? (
            <div className="settings-tab-content">
              <div className="settings-section">
                <div className="api-config-form">
                  <div className="form-row">
                    <div className="form-group">
                      <label>接口密钥</label>
                      <input
                        className="apple-input"
                        type="password"
                        value={draftTranslateApiKey}
                        onChange={(e) => onDraftTranslateApiKeyChange(e.target.value)}
                        placeholder="sk-..."
                      />
                    </div>
                    <div className="form-group">
                      <label>接口地址</label>
                      <input
                        className="apple-input"
                        value={draftTranslateBaseUrl}
                        onChange={(e) => onDraftTranslateBaseUrlChange(e.target.value)}
                        placeholder="https://api.openai.com/v1"
                      />
                    </div>
                    <div className="form-group llm-model-field">
                      <label>模型名称</label>
                      <div className="llm-model-test-row">
                        <input
                          className="apple-input llm-model-input"
                          value={draftTranslateModel}
                          onChange={(e) => onDraftTranslateModelChange(e.target.value)}
                          placeholder="gpt-4.1-mini"
                        />
                        <button
                          type="button"
                          className="nav-button llm-test-btn"
                          onClick={() => { void onTestTranslateConnection(); }}
                        >
                          测试
                        </button>
                      </div>
                    </div>
                    <div className="form-group">
                      <label>并发数</label>
                      <input
                        className="apple-input"
                        inputMode="numeric"
                        value={draftLlmConcurrencyInput}
                        onChange={(e) => onDraftLlmConcurrencyInputChange(e.target.value.replace(/[^0-9]/g, ""))}
                        placeholder="1 - 16"
                      />
                    </div>
                  </div>
                  <label className="setting-toggle" htmlFor="enable-terminology">
                    <input
                      id="enable-terminology"
                      type="checkbox"
                      checked={draftEnableTerminology}
                      onChange={(e) => onDraftEnableTerminologyChange(e.target.checked)}
                    />
                    <div className="toggle-label">
                      <span className="toggle-title">启用术语库</span>
                      <span className="toggle-desc">关闭后翻译不注入术语，按通用语义翻译。</span>
                    </div>
                    <span className="toggle-switch" />
                  </label>
                </div>
              </div>
            </div>
          ) : activeTab === "subtitle" ? (
            <div className="settings-tab-content">
              <div className="settings-section">
                <div className="api-config-form">
                  <div className="form-row">
                    <div className="form-group">
                      <label>原文长度（词）</label>
                      <input
                        className="apple-input"
                        inputMode="numeric"
                        value={draftSubtitleMaxWordsInput}
                        onChange={(e) => onDraftSubtitleMaxWordsInputChange(e.target.value.replace(/[^0-9]/g, ""))}
                        placeholder="8 - 40"
                      />
                    </div>
                    <div className="form-group">
                      <label>译文长度（字）</label>
                      <input
                        className="apple-input"
                        inputMode="numeric"
                        value={draftSubtitleLengthReferenceInput}
                        onChange={(e) => onDraftSubtitleLengthReferenceInputChange(e.target.value.replace(/[^0-9]/g, ""))}
                        placeholder="8 - 80（软约束）"
                      />
                    </div>
                  </div>
                  <div className="subtitle-toggle-row">
                    <label className="setting-toggle" htmlFor="auto-burn-hard-subtitle">
                      <input
                        id="auto-burn-hard-subtitle"
                        type="checkbox"
                        checked={draftAutoBurnHardSubtitle}
                        onChange={(e) => onDraftAutoBurnHardSubtitleChange(e.target.checked)}
                      />
                      <div className="toggle-label">
                        <span className="toggle-title">自动压制</span>
                        <span className="toggle-desc">仅视频任务生效</span>
                      </div>
                      <span className="toggle-switch" />
                    </label>
                    <label className="setting-toggle" htmlFor="enable-subtitle-beautify">
                      <input
                        id="enable-subtitle-beautify"
                        type="checkbox"
                        checked={draftEnableSubtitleBeautify}
                        onChange={(e) => onDraftEnableSubtitleBeautifyChange(e.target.checked)}
                      />
                      <div className="toggle-label">
                        <span className="toggle-title">美化字幕</span>
                        <span className="toggle-desc">去首尾标点与观感优化</span>
                      </div>
                      <span className="toggle-switch" />
                    </label>
                  </div>
                  <div className="form-row">
                    <div className="form-group">
                      <label>字幕类型</label>
                      <select
                        className="apple-input"
                        value={draftSubtitleBurnMode}
                        onChange={(e) => onDraftSubtitleBurnModeChange(e.target.value as SubtitleBurnMode)}
                      >
                        <option value="source">原文</option>
                        <option value="target">译文</option>
                        <option value="bilingualSourceFirst">双语（原文上译文下）</option>
                        <option value="bilingualTargetFirst">双语（译文上原文下）</option>
                      </select>
                    </div>
                  </div>
                </div>
              </div>
              <div className="settings-section">
                <h3 className="apple-heading-small">字幕样式</h3>
                <div className="api-config-form">
                  <div className="form-row subtitle-style-grid">
                    <div className="form-group">
                      <label>原文字体</label>
                      <select
                        className="apple-input"
                        value={draftSubtitleRenderStyle.source.fontFamily}
                        onChange={(e) => onDraftSubtitleRenderStyleChange({
                          ...draftSubtitleRenderStyle,
                          source: {
                            ...draftSubtitleRenderStyle.source,
                            fontFamily: e.target.value,
                          },
                        })}
                      >
                        <option value={draftSubtitleRenderStyle.source.fontFamily}>
                          {draftSubtitleRenderStyle.source.fontFamily}
                        </option>
                        {systemFonts
                          .filter((font) => font !== draftSubtitleRenderStyle.source.fontFamily)
                          .map((font) => (
                            <option key={`source-${font}`} value={font}>{font}</option>
                          ))}
                      </select>
                    </div>
                    <div className="form-group">
                      <label>原文字号</label>
                      <input
                        className="apple-input"
                        type="number"
                        min={16}
                        max={96}
                        value={draftSubtitleRenderStyle.source.fontSize}
                        onChange={(e) => onDraftSubtitleRenderStyleChange({
                          ...draftSubtitleRenderStyle,
                          source: {
                            ...draftSubtitleRenderStyle.source,
                            fontSize: Number.parseInt(e.target.value || "0", 10) || 44,
                          },
                        })}
                      />
                    </div>
                    <div className="form-group">
                      <label>原文颜色</label>
                      <input
                        className="apple-input subtitle-color-input"
                        type="color"
                        value={draftSubtitleRenderStyle.source.primaryColor}
                        onChange={(e) => onDraftSubtitleRenderStyleChange({
                          ...draftSubtitleRenderStyle,
                          source: {
                            ...draftSubtitleRenderStyle.source,
                            primaryColor: e.target.value.toUpperCase(),
                          },
                        })}
                      />
                    </div>
                    <div className="form-group">
                      <label>原文阴影强度</label>
                      <input
                        className="apple-input"
                        type="number"
                        min={0}
                        max={8}
                        step={0.5}
                        value={draftSubtitleRenderStyle.source.shadow}
                        onChange={(e) => onDraftSubtitleRenderStyleChange({
                          ...draftSubtitleRenderStyle,
                          source: {
                            ...draftSubtitleRenderStyle.source,
                            shadow: Number.parseFloat(e.target.value || "0") || 0,
                          },
                        })}
                      />
                    </div>
                    <div className="form-group">
                      <label>原文阴影色</label>
                      <input
                        className="apple-input subtitle-color-input"
                        type="color"
                        value={draftSubtitleRenderStyle.source.backColor}
                        onChange={(e) => onDraftSubtitleRenderStyleChange({
                          ...draftSubtitleRenderStyle,
                          source: {
                            ...draftSubtitleRenderStyle.source,
                            backColor: e.target.value.toUpperCase(),
                          },
                        })}
                      />
                    </div>
                    <div className="form-group">
                      <label>原文边框样式</label>
                      <select
                        className="apple-input"
                        value={draftSubtitleRenderStyle.source.borderStyle}
                        onChange={(e) => onDraftSubtitleRenderStyleChange({
                          ...draftSubtitleRenderStyle,
                          source: {
                            ...draftSubtitleRenderStyle.source,
                            borderStyle: e.target.value === "box" ? "box" : "outline",
                          },
                        })}
                      >
                        <option value="outline">描边</option>
                        <option value="box">方框</option>
                      </select>
                    </div>
                    <div className="form-group">
                      <label>原文描边粗细</label>
                      <input
                        className="apple-input"
                        type="number"
                        min={0}
                        max={8}
                        step={0.5}
                        value={draftSubtitleRenderStyle.source.outline}
                        onChange={(e) => onDraftSubtitleRenderStyleChange({
                          ...draftSubtitleRenderStyle,
                          source: {
                            ...draftSubtitleRenderStyle.source,
                            outline: Number.parseFloat(e.target.value || "0") || 0,
                          },
                        })}
                      />
                    </div>
                    <div className="form-group">
                      <label>原文边框色</label>
                      <input
                        className="apple-input subtitle-color-input"
                        type="color"
                        value={draftSubtitleRenderStyle.source.outlineColor}
                        onChange={(e) => onDraftSubtitleRenderStyleChange({
                          ...draftSubtitleRenderStyle,
                          source: {
                            ...draftSubtitleRenderStyle.source,
                            outlineColor: e.target.value.toUpperCase(),
                          },
                        })}
                      />
                    </div>
                    <div className="form-group">
                      <label>原文边框透明度</label>
                      <input
                        className="apple-input"
                        type="number"
                        min={0}
                        max={100}
                        value={draftSubtitleRenderStyle.source.borderOpacity}
                        onChange={(e) => onDraftSubtitleRenderStyleChange({
                          ...draftSubtitleRenderStyle,
                          source: {
                            ...draftSubtitleRenderStyle.source,
                            borderOpacity: Number.parseInt(e.target.value || "0", 10) || 0,
                          },
                        })}
                      />
                    </div>
                    <div className="subtitle-style-divider" aria-hidden="true" />
                    <div className="subtitle-style-grid-break" aria-hidden="true" />
                    <div className="form-group">
                      <label>译文字体</label>
                      <select
                        className="apple-input"
                        value={draftSubtitleRenderStyle.target.fontFamily}
                        onChange={(e) => onDraftSubtitleRenderStyleChange({
                          ...draftSubtitleRenderStyle,
                          target: {
                            ...draftSubtitleRenderStyle.target,
                            fontFamily: e.target.value,
                          },
                        })}
                      >
                        <option value={draftSubtitleRenderStyle.target.fontFamily}>
                          {draftSubtitleRenderStyle.target.fontFamily}
                        </option>
                        {systemFonts
                          .filter((font) => font !== draftSubtitleRenderStyle.target.fontFamily)
                          .map((font) => (
                            <option key={`target-${font}`} value={font}>{font}</option>
                          ))}
                      </select>
                    </div>
                    <div className="form-group">
                      <label>译文字号</label>
                      <input
                        className="apple-input"
                        type="number"
                        min={16}
                        max={96}
                        value={draftSubtitleRenderStyle.target.fontSize}
                        onChange={(e) => onDraftSubtitleRenderStyleChange({
                          ...draftSubtitleRenderStyle,
                          target: {
                            ...draftSubtitleRenderStyle.target,
                            fontSize: Number.parseInt(e.target.value || "0", 10) || 40,
                          },
                        })}
                      />
                    </div>
                    <div className="form-group">
                      <label>译文颜色</label>
                      <input
                        className="apple-input subtitle-color-input"
                        type="color"
                        value={draftSubtitleRenderStyle.target.primaryColor}
                        onChange={(e) => onDraftSubtitleRenderStyleChange({
                          ...draftSubtitleRenderStyle,
                          target: {
                            ...draftSubtitleRenderStyle.target,
                            primaryColor: e.target.value.toUpperCase(),
                          },
                        })}
                      />
                    </div>
                    <div className="form-group">
                      <label>译文阴影强度</label>
                      <input
                        className="apple-input"
                        type="number"
                        min={0}
                        max={8}
                        step={0.5}
                        value={draftSubtitleRenderStyle.target.shadow}
                        onChange={(e) => onDraftSubtitleRenderStyleChange({
                          ...draftSubtitleRenderStyle,
                          target: {
                            ...draftSubtitleRenderStyle.target,
                            shadow: Number.parseFloat(e.target.value || "0") || 0,
                          },
                        })}
                      />
                    </div>
                    <div className="form-group">
                      <label>译文阴影色</label>
                      <input
                        className="apple-input subtitle-color-input"
                        type="color"
                        value={draftSubtitleRenderStyle.target.backColor}
                        onChange={(e) => onDraftSubtitleRenderStyleChange({
                          ...draftSubtitleRenderStyle,
                          target: {
                            ...draftSubtitleRenderStyle.target,
                            backColor: e.target.value.toUpperCase(),
                          },
                        })}
                      />
                    </div>
                    <div className="form-group">
                      <label>译文边框样式</label>
                      <select
                        className="apple-input"
                        value={draftSubtitleRenderStyle.target.borderStyle}
                        onChange={(e) => onDraftSubtitleRenderStyleChange({
                          ...draftSubtitleRenderStyle,
                          target: {
                            ...draftSubtitleRenderStyle.target,
                            borderStyle: e.target.value === "box" ? "box" : "outline",
                          },
                        })}
                      >
                        <option value="outline">描边</option>
                        <option value="box">方框</option>
                      </select>
                    </div>
                    <div className="form-group">
                      <label>译文描边粗细</label>
                      <input
                        className="apple-input"
                        type="number"
                        min={0}
                        max={8}
                        step={0.5}
                        value={draftSubtitleRenderStyle.target.outline}
                        onChange={(e) => onDraftSubtitleRenderStyleChange({
                          ...draftSubtitleRenderStyle,
                          target: {
                            ...draftSubtitleRenderStyle.target,
                            outline: Number.parseFloat(e.target.value || "0") || 0,
                          },
                        })}
                      />
                    </div>
                    <div className="form-group">
                      <label>译文边框色</label>
                      <input
                        className="apple-input subtitle-color-input"
                        type="color"
                        value={draftSubtitleRenderStyle.target.outlineColor}
                        onChange={(e) => onDraftSubtitleRenderStyleChange({
                          ...draftSubtitleRenderStyle,
                          target: {
                            ...draftSubtitleRenderStyle.target,
                            outlineColor: e.target.value.toUpperCase(),
                          },
                        })}
                      />
                    </div>
                    <div className="form-group">
                      <label>译文边框透明度</label>
                      <input
                        className="apple-input"
                        type="number"
                        min={0}
                        max={100}
                        value={draftSubtitleRenderStyle.target.borderOpacity}
                        onChange={(e) => onDraftSubtitleRenderStyleChange({
                          ...draftSubtitleRenderStyle,
                          target: {
                            ...draftSubtitleRenderStyle.target,
                            borderOpacity: Number.parseInt(e.target.value || "0", 10) || 0,
                          },
                        })}
                      />
                    </div>
                    <div className="subtitle-style-grid-break" aria-hidden="true" />
                    <div className="form-group">
                      <label>底部边距</label>
                      <input
                        className="apple-input"
                        type="number"
                        min={0}
                        max={200}
                        value={draftSubtitleRenderStyle.layout.marginV}
                        onChange={(e) => onDraftSubtitleRenderStyleChange({
                          ...draftSubtitleRenderStyle,
                          layout: {
                            ...draftSubtitleRenderStyle.layout,
                            marginV: Number.parseInt(e.target.value || "0", 10) || 0,
                          },
                        })}
                      />
                    </div>
                    <div className="form-group">
                      <label>双语行距</label>
                      <input
                        className="apple-input"
                        type="number"
                        min={0}
                        max={140}
                        value={draftSubtitleRenderStyle.layout.bilingualLineGap}
                        onChange={(e) => onDraftSubtitleRenderStyleChange({
                          ...draftSubtitleRenderStyle,
                          layout: {
                            ...draftSubtitleRenderStyle.layout,
                            bilingualLineGap: e.target.value === ""
                              ? 10
                              : Number.parseInt(e.target.value, 10),
                          },
                        })}
                      />
                    </div>
                    <div className="form-group">
                      <label>对齐</label>
                      <select
                        className="apple-input"
                        value={draftSubtitleRenderStyle.layout.alignment}
                        onChange={(e) => onDraftSubtitleRenderStyleChange({
                          ...draftSubtitleRenderStyle,
                          layout: {
                            ...draftSubtitleRenderStyle.layout,
                            alignment: Number.parseInt(e.target.value, 10) as 1 | 2 | 3,
                          },
                        })}
                      >
                        <option value={1}>底部左对齐</option>
                        <option value={2}>底部居中</option>
                        <option value={3}>底部右对齐</option>
                      </select>
                    </div>
                  </div>
                  <SubtitleStylePreview mode={draftSubtitleBurnMode} style={draftSubtitleRenderStyle} />
                </div>
              </div>
            </div>
          ) : (
            <div className="settings-tab-content model-center-content">
              <ModelDownloadCard
                target="asr"
                title="转录模型"
                description="parakeet-tdt-0.6b-v2 支持高质量英文转录，可自动补全标点与大小写并提供准确时间戳。"
                modelName="parakeet-tdt-0.6b-v2"
                selected={draftAsrModel === "parakeet-tdt-0.6b-v2"}
                status={asrStatus}
                onSelect={() => onDraftAsrModelChange("parakeet-tdt-0.6b-v2")}
                onOpenModelDir={onOpenModelDir}
                onStartModelDownload={onStartModelDownload}
                onCancelModelDownload={onCancelModelDownload}
              />

              <ModelDownloadCard
                target="demucs"
                title="人声分离模型"
                description="htdemucs_ft 是高保真人声分离模型，能更稳定地提取清晰 vocals、减少伴奏残留。"
                modelName="htdemucs_ft"
                selected={draftDemucsModel === "htdemucs_ft"}
                status={demucsStatus}
                onSelect={() => onDraftDemucsModelChange("htdemucs_ft")}
                onOpenModelDir={onOpenModelDir}
                onStartModelDownload={onStartModelDownload}
                onCancelModelDownload={onCancelModelDownload}
              />
            </div>
          )}
        </div>
        <div className="settings-footer">
          <button className="nav-button" onClick={onSave} title="保存设置" aria-label="保存设置">
            <CheckIcon />
            <span>保存</span>
          </button>
        </div>
      </div>
    </div>
  );
}
