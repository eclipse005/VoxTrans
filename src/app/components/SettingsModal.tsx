import { useState } from "react";
import type {
  AsrModel,
  DemucsModel,
  ModelStatusResponse,
  Provider,
} from "../../features/media/types";
import { PROVIDER_OPTIONS } from "../../features/media/provider";
import { CheckIcon, CpuIcon, DownloadIcon, FolderIcon, GpuIcon } from "./Icons";
import { useDialogA11y } from "./useDialogA11y";

type SettingsModalProps = {
  visible: boolean;
  draftProvider: Provider;
  draftChunkInput: string;
  draftSubtitleMaxWordsInput: string;
  draftAsrModel: AsrModel;
  draftDemucsModel: DemucsModel;
  draftEnableVocalSeparation: boolean;
  draftTranslateApiKey: string;
  draftTranslateBaseUrl: string;
  draftTranslateModel: string;
  draftLlmConcurrencyInput: string;
  draftEnableTerminology: boolean;
  draftEnablePunctuationOptimization: boolean;
  asrStatus: ModelStatusResponse | null;
  demucsStatus: ModelStatusResponse | null;
  onClose: () => void;
  onSave: () => void | Promise<void>;
  onDraftProviderChange: (value: Provider) => void;
  onDraftChunkInputChange: (value: string) => void;
  onDraftSubtitleMaxWordsInputChange: (value: string) => void;
  onDraftAsrModelChange: (value: AsrModel) => void;
  onDraftDemucsModelChange: (value: DemucsModel) => void;
  onDraftEnableVocalSeparationChange: (value: boolean) => void;
  onDraftTranslateApiKeyChange: (value: string) => void;
  onDraftTranslateBaseUrlChange: (value: string) => void;
  onDraftTranslateModelChange: (value: string) => void;
  onDraftLlmConcurrencyInputChange: (value: string) => void;
  onDraftEnableTerminologyChange: (value: boolean) => void;
  onDraftEnablePunctuationOptimizationChange: (value: boolean) => void;
  onTestTranslateConnection: () => void | Promise<void>;
  onOpenModelDir: (target: "asr" | "demucs") => void | Promise<void>;
  onStartModelDownload: (target: "asr" | "demucs") => void | Promise<void>;
  onCancelModelDownload: (target: "asr" | "demucs") => void | Promise<void>;
};

function formatBytes(value: number): string {
  if (!Number.isFinite(value) || value <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  let size = value;
  let idx = 0;
  while (size >= 1024 && idx < units.length - 1) {
    size /= 1024;
    idx += 1;
  }
  return `${size.toFixed(idx === 0 ? 0 : 2)} ${units[idx]}`;
}

function progressPercent(status: ModelStatusResponse | null): number {
  const total = status?.download.totalBytes ?? 0;
  const downloaded = status?.download.downloadedBytes ?? 0;
  if (total <= 0) return 0;
  return Math.max(0, Math.min(100, Math.round((downloaded / total) * 100)));
}

function isReady(status: ModelStatusResponse | null): boolean {
  if (!status) return false;
  return status.ready || status.download.phase === "completed";
}

function formatModelSizeText(status: ModelStatusResponse | null): string {
  if (!status) return "-";
  const downloaded = status.download.downloadedBytes;
  const total = status.download.totalBytes;
  if (total <= 0) return "-";
  if (status.download.phase === "downloading") {
    return `${formatBytes(downloaded)} / ${formatBytes(total)}`;
  }
  return formatBytes(total);
}

export default function SettingsModal(props: SettingsModalProps) {
  const {
    visible,
    draftProvider,
    draftChunkInput,
    draftSubtitleMaxWordsInput,
    draftAsrModel,
    draftDemucsModel,
    draftEnableVocalSeparation,
    draftTranslateApiKey,
    draftTranslateBaseUrl,
    draftTranslateModel,
    draftLlmConcurrencyInput,
    draftEnableTerminology,
    draftEnablePunctuationOptimization,
    asrStatus,
    demucsStatus,
    onClose,
    onSave,
    onDraftProviderChange,
    onDraftChunkInputChange,
    onDraftSubtitleMaxWordsInputChange,
    onDraftAsrModelChange,
    onDraftDemucsModelChange,
    onDraftEnableVocalSeparationChange,
    onDraftTranslateApiKeyChange,
    onDraftTranslateBaseUrlChange,
    onDraftTranslateModelChange,
    onDraftLlmConcurrencyInputChange,
    onDraftEnableTerminologyChange,
    onDraftEnablePunctuationOptimizationChange,
    onTestTranslateConnection,
    onOpenModelDir,
    onStartModelDownload,
    onCancelModelDownload,
  } = props;

  const [activeTab, setActiveTab] = useState<"transcribe" | "translate" | "models">("transcribe");
  const dialogRef = useDialogA11y(visible, onClose);
  if (!visible) return null;

  const asrDownloading = asrStatus?.download.phase === "downloading";
  const demucsDownloading = demucsStatus?.download.phase === "downloading";
  const asrReady = isReady(asrStatus);
  const demucsReady = isReady(demucsStatus);
  const asrPercent = progressPercent(asrStatus);
  const demucsPercent = progressPercent(demucsStatus);
  const tabIndex = activeTab === "transcribe" ? 0 : activeTab === "translate" ? 1 : 2;

  const asrSizeText = formatModelSizeText(asrStatus);
  const demucsSizeText = formatModelSizeText(demucsStatus);

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
        <div className="settings-tabs-nav" style={{ ["--tab-index" as string]: tabIndex, ["--tab-count" as string]: 3 }}>
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
                    <div className="form-group">
                      <label>字幕长度（词）</label>
                      <input
                        className="apple-input"
                        inputMode="numeric"
                        value={draftSubtitleMaxWordsInput}
                        onChange={(e) => onDraftSubtitleMaxWordsInputChange(e.target.value.replace(/[^0-9]/g, ""))}
                        placeholder="8 - 40"
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
                <h3 className="apple-heading-small">翻译配置</h3>
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
                  <label className="setting-toggle" htmlFor="enable-punctuation-optimization">
                    <input
                      id="enable-punctuation-optimization"
                      type="checkbox"
                      checked={draftEnablePunctuationOptimization}
                      onChange={(e) => onDraftEnablePunctuationOptimizationChange(e.target.checked)}
                    />
                    <div className="toggle-label">
                      <span className="toggle-title">标点符号优化</span>
                      <span className="toggle-desc">使用 LLM 优化大小写标点符号，有益于断句。</span>
                    </div>
                    <span className="toggle-switch" />
                  </label>
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
          ) : (
            <div className="settings-tab-content model-center-content">
              <div className="model-task-card">
                <div className="model-task-card-head">
                  <h4 className="apple-heading-small">转录模型</h4>
                  <span className={`model-ready-pill ${asrReady ? "ready" : "not-ready"}`}>
                    {asrReady ? "已就绪" : "未就绪"}
                  </span>
                </div>
                <p className="apple-body-small">parakeet-tdt-0.6b-v2 支持高质量英文转录，可自动补全标点与大小写并提供准确时间戳。</p>
                <div className="model-inline-row">
                  <div className="device-toggle-group model-inline-model" role="group" aria-label="ASR 版本">
                    <button
                      type="button"
                      className={`device-toggle-btn ${draftAsrModel === "parakeet-tdt-0.6b-v2" ? "active" : ""}`}
                      onClick={() => onDraftAsrModelChange("parakeet-tdt-0.6b-v2")}
                    >
                      <span className="model-inline-label">
                        <span>parakeet-tdt-0.6b-v2</span>
                        <span className="model-inline-size">{asrSizeText}</span>
                      </span>
                    </button>
                  </div>
                  <div className="model-task-actions model-task-actions-inline">
                    <button
                      className="file-list-icon-btn"
                      type="button"
                      title="打开目录"
                      aria-label="打开 ASR 模型目录"
                      onClick={() => { void onOpenModelDir("asr"); }}
                    >
                      <FolderIcon />
                    </button>
                    <button
                      className={`file-list-icon-btn model-download-state-btn ${asrReady ? "is-ready" : asrDownloading ? "is-downloading" : "is-idle"}`}
                      type="button"
                      title={asrDownloading ? "取消下载" : asrReady ? "已就绪" : "下载模型"}
                      aria-label={asrDownloading ? "取消 ASR 下载" : asrReady ? "ASR 已就绪" : "下载 ASR 模型"}
                      onClick={() => {
                        if (asrDownloading) {
                          void onCancelModelDownload("asr");
                          return;
                        }
                        if (!asrReady) {
                          void onStartModelDownload("asr");
                        }
                      }}
                      disabled={asrReady}
                      style={{ ["--ring-progress" as string]: `${asrPercent}%` }}
                    >
                      {asrDownloading ? (
                        <span className="model-progress-ring-inner">{asrPercent}%</span>
                      ) : asrReady ? (
                        <CheckIcon />
                      ) : (
                        <DownloadIcon />
                      )}
                    </button>
                  </div>
                </div>
              </div>

              <div className="model-task-card">
                <div className="model-task-card-head">
                  <h4 className="apple-heading-small">人声分离模型</h4>
                  <span className={`model-ready-pill ${demucsReady ? "ready" : "not-ready"}`}>
                    {demucsReady ? "已就绪" : "未就绪"}
                  </span>
                </div>
                <p className="apple-body-small">htdemucs_ft 是高保真人声分离模型，能更稳定地提取清晰 vocals、减少伴奏残留。</p>
                <div className="model-inline-row">
                  <div className="device-toggle-group model-inline-model" role="group" aria-label="Demucs 版本">
                    <button
                      type="button"
                      className={`device-toggle-btn ${draftDemucsModel === "htdemucs_ft" ? "active" : ""}`}
                      onClick={() => onDraftDemucsModelChange("htdemucs_ft")}
                    >
                      <span className="model-inline-label">
                        <span>htdemucs_ft</span>
                        <span className="model-inline-size">{demucsSizeText}</span>
                      </span>
                    </button>
                  </div>
                  <div className="model-task-actions model-task-actions-inline">
                    <button
                      className="file-list-icon-btn"
                      type="button"
                      title="打开目录"
                      aria-label="打开 Demucs 模型目录"
                      onClick={() => { void onOpenModelDir("demucs"); }}
                    >
                      <FolderIcon />
                    </button>
                    <button
                      className={`file-list-icon-btn model-download-state-btn ${demucsReady ? "is-ready" : demucsDownloading ? "is-downloading" : "is-idle"}`}
                      type="button"
                      title={demucsDownloading ? "取消下载" : demucsReady ? "已就绪" : "下载模型"}
                      aria-label={demucsDownloading ? "取消 Demucs 下载" : demucsReady ? "Demucs 已就绪" : "下载 Demucs 模型"}
                      onClick={() => {
                        if (demucsDownloading) {
                          void onCancelModelDownload("demucs");
                          return;
                        }
                        if (!demucsReady) {
                          void onStartModelDownload("demucs");
                        }
                      }}
                      disabled={demucsReady}
                      style={{ ["--ring-progress" as string]: `${demucsPercent}%` }}
                    >
                      {demucsDownloading ? (
                        <span className="model-progress-ring-inner">{demucsPercent}%</span>
                      ) : demucsReady ? (
                        <CheckIcon />
                      ) : (
                        <DownloadIcon />
                      )}
                    </button>
                  </div>
                </div>
              </div>
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
