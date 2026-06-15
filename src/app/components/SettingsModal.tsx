import { useEffect, useState } from "react";
import type { SubtitleBurnMode } from "../../features/media/types";
import { PROVIDER_OPTIONS } from "../../features/media/provider";
import { listSystemFonts } from "../api/system";
import { CheckIcon, CpuIcon, GpuIcon } from "./Icons";
import { ModelDownloadCard } from "./settings/ModelDownloadCard";
import { SubtitleStylePreview } from "./settings/SubtitleStylePreview";
import { useDialogA11y } from "./useDialogA11y";
import { useSettingsFormContext } from "../contexts/SettingsFormContext";

const SUBTITLE_LENGTH_PRESETS = [
  { id: "short", label: "短" },
  { id: "standard", label: "标准" },
  { id: "loose", label: "宽松" },
] as const;

const FLAT_SRT_OPTIONS: { item: SubtitleBurnMode; label: string }[] = [
  { item: "source", label: "原文" },
  { item: "target", label: "译文" },
  { item: "bilingualSourceFirst", label: "双语（原文上）" },
  { item: "bilingualTargetFirst", label: "双语（译文上）" },
];

type SettingsModalProps = {
  visible: boolean;
  onClose: () => void;
};

export default function SettingsModal({ visible, onClose }: SettingsModalProps) {
  const ctx = useSettingsFormContext();

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
    const nextValue = Math.min(60, Number.parseInt(digits, 10));
    ctx.setForm((prev) => ({ ...prev, chunkInput: String(nextValue) }));
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
                      <label>分段时长（秒）</label>
                      <div className="bounded-number-field">
                        <input
                          className="apple-input"
                          type="number"
                          inputMode="numeric"
                          min={30}
                          max={60}
                          value={ctx.form.chunkInput}
                          onChange={(e) => handleChunkInputChange(e.target.value)}
                          placeholder="30 - 60"
                        />
                        <span className="bounded-number-hint">30-60</span>
                      </div>
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
                      <span className="toggle-title">人声分离</span>
                      <span className="toggle-desc">背景吵杂时请使用，提高转录准确率</span>
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
                      <span className="toggle-title">点击音效</span>
                      <span className="toggle-desc">点击按钮和开关时播放轻提示音</span>
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
                      <span className="toggle-title">字幕平铺输出</span>
                      <span className="toggle-desc">额外将字幕直接输出到 output 目录，方便批量查找</span>
                    </div>
                    <span className="toggle-switch" />
                  </label>
                  {ctx.form.flatSrtOutput ? (
                    <div className="flat-srt-items-group">
                      <label className="flat-srt-items-label">输出字幕类型</label>
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
                            <span>{option.label}</span>
                          </label>
                        ))}
                      </div>
                    </div>
                  ) : null}
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
                        value={ctx.form.translateApiKey}
                        onChange={(e) => ctx.setForm((prev) => ({ ...prev, translateApiKey: e.target.value }))}
                        placeholder="sk-..."
                      />
                    </div>
                    <div className="form-group">
                      <label>接口地址</label>
                      <input
                        className="apple-input"
                        value={ctx.form.translateBaseUrl}
                        onChange={(e) => ctx.setForm((prev) => ({ ...prev, translateBaseUrl: e.target.value }))}
                        placeholder="https://api.openai.com/v1"
                      />
                    </div>
                    <div className="form-group llm-model-field">
                      <label>模型名称</label>
                      <div className="llm-model-test-row">
                        <input
                          className="apple-input llm-model-input"
                          value={ctx.form.translateModel}
                          onChange={(e) => ctx.setForm((prev) => ({ ...prev, translateModel: e.target.value }))}
                          placeholder="gpt-4.1-mini"
                        />
                        <button
                          type="button"
                          className="nav-button llm-test-btn"
                          onClick={() => { void ctx.testTranslateConnection(); }}
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
                        value={ctx.form.llmConcurrencyInput}
                        onChange={(e) => ctx.setForm((prev) => ({ ...prev, llmConcurrencyInput: e.target.value.replace(/[^0-9]/g, "") }))}
                        placeholder="1 - 16"
                      />
                    </div>
                  </div>
                </div>
              </div>
            </div>
          ) : activeTab === "subtitle" ? (
            <div className="settings-tab-content">
              <div className="settings-section">
                <div className="api-config-form">
                  <div className="form-row">
                    <div className="form-group subtitle-length-group">
                      <div className="subtitle-length-card">
                        <span className="toggle-title">字幕长度</span>
                        <div
                          className="subtitle-length-slider"
                          role="radiogroup"
                          aria-label="字幕长度"
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
                              {preset.label}
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
                        <span className="toggle-title">自动压制</span>
                        <span className="toggle-desc">仅视频任务生效</span>
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
                        value={ctx.form.subtitleBurnMode}
                        onChange={(e) => ctx.setForm((prev) => ({ ...prev, subtitleBurnMode: e.target.value as SubtitleBurnMode }))}
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
                      <label>原文字号</label>
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
                      <label>原文颜色</label>
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
                      <label>原文阴影强度</label>
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
                      <label>原文阴影色</label>
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
                      <label>原文边框样式</label>
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
                      <label>原文边框色</label>
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
                      <label>原文边框透明度</label>
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
                      <label>译文字体</label>
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
                      <label>译文字号</label>
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
                      <label>译文颜色</label>
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
                      <label>译文阴影强度</label>
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
                      <label>译文阴影色</label>
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
                      <label>译文边框样式</label>
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
                      <label>译文边框色</label>
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
                      <label>译文边框透明度</label>
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
                      <label>底部边距</label>
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
                      <label>双语行距</label>
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
                      <label>对齐</label>
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
                        <option value={1}>底部左对齐</option>
                        <option value={2}>底部居中</option>
                        <option value={3}>底部右对齐</option>
                      </select>
                    </div>
                  </div>
                  <SubtitleStylePreview mode={ctx.form.subtitleBurnMode} style={ctx.form.subtitleRenderStyle} />
                </div>
              </div>
            </div>
          ) : (
            <div className="settings-tab-content model-center-content">
              <ModelDownloadCard
                target="asr"
                title="ASR 模型"
                description="Qwen3-ASR-0.6B 负责从音频生成纯文本，时间戳由独立对齐模型处理。"
                modelName="Qwen3-ASR-0.6B"
                selected={ctx.form.asrModel === "Qwen3-ASR-0.6B"}
                status={ctx.asrStatusByModel["Qwen3-ASR-0.6B"] ?? (ctx.form.asrModel === "Qwen3-ASR-0.6B" ? ctx.asrStatus : null)}
                onSelect={() => ctx.setForm((prev) => ({ ...prev, asrModel: "Qwen3-ASR-0.6B" }))}
                onOpenModelDir={ctx.openModelDir}
                onStartModelDownload={ctx.startModelDownload}
                onCancelModelDownload={ctx.cancelModelDownload}
              />

              <ModelDownloadCard
                target="asr"
                title="ASR 模型"
                description="Qwen3-ASR-1.7B 负责从音频生成纯文本，时间戳由独立对齐模型处理。"
                modelName="Qwen3-ASR-1.7B"
                selected={ctx.form.asrModel === "Qwen3-ASR-1.7B"}
                status={ctx.asrStatusByModel["Qwen3-ASR-1.7B"] ?? (ctx.form.asrModel === "Qwen3-ASR-1.7B" ? ctx.asrStatus : null)}
                onSelect={() => ctx.setForm((prev) => ({ ...prev, asrModel: "Qwen3-ASR-1.7B" }))}
                onOpenModelDir={ctx.openModelDir}
                onStartModelDownload={ctx.startModelDownload}
                onCancelModelDownload={ctx.cancelModelDownload}
              />

              <ModelDownloadCard
                target="align"
                title="对齐模型"
                description="Qwen3 Forced Aligner 负责把转录文本对齐回音频，生成词级时间戳。"
                modelName="Qwen3-ForcedAligner-0.6B"
                selected={ctx.form.alignModel === "Qwen3-ForcedAligner-0.6B"}
                status={ctx.alignStatus}
                onSelect={() => ctx.setForm((prev) => ({ ...prev, alignModel: "Qwen3-ForcedAligner-0.6B" }))}
                onOpenModelDir={ctx.openModelDir}
                onStartModelDownload={ctx.startModelDownload}
                onCancelModelDownload={ctx.cancelModelDownload}
              />

              <ModelDownloadCard
                target="demucs"
                title="人声分离模型"
                description="htdemucs_ft 是高保真人声分离模型，能更稳定地提取清晰 vocals、减少伴奏残留。"
                modelName="htdemucs_ft"
                selected={ctx.form.demucsModel === "htdemucs_ft"}
                status={ctx.demucsStatus}
                onSelect={() => ctx.setForm((prev) => ({ ...prev, demucsModel: "htdemucs_ft" }))}
                onOpenModelDir={ctx.openModelDir}
                onStartModelDownload={ctx.startModelDownload}
                onCancelModelDownload={ctx.cancelModelDownload}
              />
            </div>
          )}
        </div>
        <div className="settings-footer">
          <button className="nav-button" onClick={ctx.saveSettings} title="保存设置" aria-label="保存设置">
            <CheckIcon />
            <span>保存</span>
          </button>
        </div>
      </div>
    </div>
  );
}
