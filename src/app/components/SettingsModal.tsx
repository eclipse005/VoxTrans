import type { Provider } from "../../features/media/types";
import type { SettingsTab } from "../types";
import { CpuIcon, GpuIcon } from "./Icons";

type SettingsModalProps = {
  visible: boolean;
  settingsTab: SettingsTab;
  tabIndicatorStyle: Record<string, number>;
  draftProvider: Provider;
  draftChunkInput: string;
  draftAutoPunc: boolean;
  draftHotwordCorrection: boolean;
  draftApiBase: string;
  onClose: () => void;
  onSave: () => void;
  onSettingsTabChange: (tab: SettingsTab) => void;
  onDraftProviderChange: (value: Provider) => void;
  onDraftChunkInputChange: (value: string) => void;
  onDraftAutoPuncChange: (value: boolean) => void;
  onDraftHotwordCorrectionChange: (value: boolean) => void;
  onDraftApiBaseChange: (value: string) => void;
};

export default function SettingsModal(props: SettingsModalProps) {
  const {
    visible,
    settingsTab,
    tabIndicatorStyle,
    draftProvider,
    draftChunkInput,
    draftAutoPunc,
    draftHotwordCorrection,
    draftApiBase,
    onClose,
    onSave,
    onSettingsTabChange,
    onDraftProviderChange,
    onDraftChunkInputChange,
    onDraftAutoPuncChange,
    onDraftHotwordCorrectionChange,
    onDraftApiBaseChange,
  } = props;

  if (!visible) return null;

  return (
    <div className="modal-overlay">
      <div className="modal-content modal-content-settings">
        <button className="modal-close" onClick={onClose}>×</button>
        <div className="settings-header">
          <h3 className="apple-heading-medium">设置</h3>
        </div>
        <div className="settings-tabs-nav">
          <div className="settings-tab-indicator" style={tabIndicatorStyle} />
          <button className={`settings-tab-btn ${settingsTab === "basic" ? "active" : ""}`} onClick={() => onSettingsTabChange("basic")}>
            基础
          </button>
          <button className={`settings-tab-btn ${settingsTab === "transcribe" ? "active" : ""}`} onClick={() => onSettingsTabChange("transcribe")}>
            转录
          </button>
          <button className={`settings-tab-btn ${settingsTab === "advanced" ? "active" : ""}`} onClick={() => onSettingsTabChange("advanced")}>
            高级
          </button>
        </div>
        <div className="settings-body">
          {settingsTab === "basic" ? (
            <div className="settings-tab-content active">
              <div className="settings-section">
                <h3 className="apple-heading-small">核心参数</h3>
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
                          <span>GPU</span>
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
                        placeholder="60 - 1800"
                      />
                    </div>
                  </div>
                </div>
              </div>
            </div>
          ) : null}

          {settingsTab === "transcribe" ? (
            <div className="settings-tab-content active">
              <div className="settings-section">
                <h3 className="apple-heading-small">转录流程</h3>
                <div className="api-config-form">
                  <div className="settings-toggles">
                    <label className="setting-toggle">
                      <input type="checkbox" checked={draftAutoPunc} onChange={(e) => onDraftAutoPuncChange(e.target.checked)} />
                      <span className="toggle-label">
                        <span className="toggle-title">自动标点增强</span>
                        <span className="toggle-desc">让转录结果更接近可阅读文本。</span>
                      </span>
                      <span className="toggle-switch" />
                    </label>
                    <label className="setting-toggle">
                      <input type="checkbox" checked={draftHotwordCorrection} onChange={(e) => onDraftHotwordCorrectionChange(e.target.checked)} />
                      <span className="toggle-label">
                        <span className="toggle-title">术语热词矫正</span>
                        <span className="toggle-desc">转录后自动应用术语表词汇矫正。</span>
                      </span>
                      <span className="toggle-switch" />
                    </label>
                  </div>
                </div>
              </div>
            </div>
          ) : null}

          {settingsTab === "advanced" ? (
            <div className="settings-tab-content active">
              <div className="settings-section">
                <h3 className="apple-heading-small">接口预留（后续翻译）</h3>
                <div className="api-config-form">
                  <div className="form-group">
                    <label>API Base URL</label>
                    <input
                      className="apple-input"
                      value={draftApiBase}
                      onChange={(e) => onDraftApiBaseChange(e.target.value)}
                      placeholder="https://api.example.com"
                    />
                  </div>
                </div>
              </div>
            </div>
          ) : null}
        </div>
        <div className="settings-footer">
          <button className="apple-button" onClick={onSave}>保存设置</button>
          <button className="apple-button apple-button-secondary" onClick={onClose}>关闭</button>
        </div>
      </div>
    </div>
  );
}
