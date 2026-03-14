import type { Provider, ModelDownloadStateSnapshot } from "../../features/media/types";
import { CheckIcon, CloseIcon, CpuIcon, DownloadIcon, FolderIcon, GpuIcon } from "./Icons";
import { useDialogA11y } from "./useDialogA11y";

type SettingsModalProps = {
  visible: boolean;
  draftProvider: Provider;
  draftChunkInput: string;
  draftSubtitleMaxWordsInput: string;
  modelDir: string;
  modelReady: boolean;
  modelDownload: ModelDownloadStateSnapshot;
  modelBusy: boolean;
  onClose: () => void;
  onSave: () => void | Promise<void>;
  onDraftProviderChange: (value: Provider) => void;
  onDraftChunkInputChange: (value: string) => void;
  onDraftSubtitleMaxWordsInputChange: (value: string) => void;
  onOpenModelDir: () => void | Promise<void>;
  onStartModelDownload: () => void | Promise<void>;
  onCancelModelDownload: () => void | Promise<void>;
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

export default function SettingsModal(props: SettingsModalProps) {
  const {
    visible,
    draftProvider,
    draftChunkInput,
    draftSubtitleMaxWordsInput,
    modelDir,
    modelReady,
    modelDownload,
    modelBusy,
    onClose,
    onSave,
    onDraftProviderChange,
    onDraftChunkInputChange,
    onDraftSubtitleMaxWordsInputChange,
    onOpenModelDir,
    onStartModelDownload,
    onCancelModelDownload,
  } = props;

  const dialogRef = useDialogA11y(visible, onClose);
  if (!visible) return null;
  const hasTotal = modelDownload.totalBytes > 0;
  const percent = hasTotal ? Math.min(100, Math.round((modelDownload.downloadedBytes / modelDownload.totalBytes) * 100)) : 0;
  const sizeText = `${formatBytes(modelDownload.downloadedBytes)} / ${formatBytes(modelDownload.totalBytes)}`;
  const speedText = modelDownload.speedBytesPerSec > 0 ? `${formatBytes(modelDownload.speedBytesPerSec)}/s` : "-";

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
        <div className="settings-body">
          <div className="settings-section">
            <h3 className="apple-heading-small">转录参数</h3>
            <div className="api-config-form">
              <div className="form-row">
                <div className="form-group">
                  <label>执行设备</label>
                  <div className="device-toggle-group" role="group" aria-label="执行设备">
                    <button
                      type="button"
                      className={`device-toggle-btn ${draftProvider === "cpu" ? "active" : ""}`}
                      onClick={() => onDraftProviderChange("cpu")}
                      aria-pressed={draftProvider === "cpu"}
                      title="CPU"
                    >
                      <CpuIcon />
                      <span>CPU</span>
                    </button>
                    <button
                      type="button"
                      className={`device-toggle-btn ${draftProvider === "cuda" ? "active" : ""}`}
                      onClick={() => onDraftProviderChange("cuda")}
                      aria-pressed={draftProvider === "cuda"}
                      title="GPU (CUDA)"
                    >
                      <GpuIcon />
                      <span>CUDA</span>
                    </button>
                  </div>
                </div>
                <div className="form-group">
                  <label>分段时长（秒）</label>
                  <input
                    className="apple-input"
                    inputMode="numeric"
                    value={draftChunkInput}
                    onChange={(e) => onDraftChunkInputChange(e.target.value.replace(/[^0-9]/g, ""))}
                    placeholder="60 - 300"
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
            </div>
          </div>

          <div className="settings-section">
            <h3 className="apple-heading-small">模型管理</h3>
            <div className="api-config-form model-manager-card">
              <div className="form-group">
                <label>模型目录</label>
                <div className="model-path-row">
                  <input className="apple-input" value={modelDir} readOnly />
                  <button
                    className="model-icon-btn"
                    type="button"
                    title="打开目录"
                    aria-label="打开模型目录"
                    onClick={() => { void onOpenModelDir(); }}
                  >
                    <FolderIcon />
                  </button>
                </div>
              </div>
              <div className="model-progress-panel">
                <div className="model-progress-block">
                  <div className="model-progress-head">
                    <span className="model-progress-title">下载进度</span>
                    <span className="model-progress-percent">{percent}%</span>
                  </div>
                  <div className="model-progress-track">
                    <div className="model-progress-fill" style={{ width: `${percent}%` }} />
                  </div>
                  <div className="model-progress-meta">
                    <span>速度: {speedText}</span>
                    <span>{sizeText}</span>
                  </div>
                </div>
                {modelDownload.phase === "downloading" ? (
                  <button
                    className="model-icon-btn"
                    type="button"
                    title="取消下载"
                    aria-label="取消下载"
                    onClick={() => { void onCancelModelDownload(); }}
                  >
                    <CloseIcon />
                  </button>
                ) : modelReady ? (
                  <button
                    className="model-icon-btn model-icon-btn-success"
                    type="button"
                    title="已下载完成"
                    aria-label="已下载完成"
                    disabled
                  >
                    <CheckIcon />
                  </button>
                ) : (
                  <button
                    className="model-icon-btn"
                    type="button"
                    title="下载模型"
                    aria-label="下载模型"
                    onClick={() => { void onStartModelDownload(); }}
                    disabled={modelBusy}
                  >
                    <DownloadIcon />
                  </button>
                )}
              </div>
            </div>
          </div>
        </div>
        <div className="settings-footer">
          <button className="apple-button" onClick={onSave}>保存设置</button>
        </div>
      </div>
    </div>
  );
}
