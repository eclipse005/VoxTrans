import { useMemo, useState } from "react";
import type { Provider } from "../../features/media/types";
import type { HotwordCorrection, SettingsTab } from "../types";
import { CpuIcon, GpuIcon } from "./Icons";
import { useDialogA11y } from "./useDialogA11y";

type SettingsModalProps = {
  visible: boolean;
  settingsTab: SettingsTab;
  tabIndicatorStyle: Record<string, number>;
  draftProvider: Provider;
  draftChunkInput: string;
  draftApiKey: string;
  draftAutoPunc: boolean;
  draftApiBase: string;
  draftApiModel: string;
  testingLlm: boolean;
  hotwordCorrection: HotwordCorrection;
  onClose: () => void;
  onSave: () => void | Promise<void>;
  onTestLlmConnection: () => void | Promise<void>;
  onSettingsTabChange: (tab: SettingsTab) => void;
  onDraftProviderChange: (value: Provider) => void;
  onDraftChunkInputChange: (value: string) => void;
  onDraftApiKeyChange: (value: string) => void;
  onDraftAutoPuncChange: (value: boolean) => void;
  onDraftApiBaseChange: (value: string) => void;
  onDraftApiModelChange: (value: string) => void;
  onHotwordCorrectionChange: (value: HotwordCorrection) => void;
};

function nextGroupIndex(groups: HotwordCorrection["groups"]) {
  return groups.reduce((max, group) => {
    const matched = /^group-(\d+)$/.exec(group.id);
    if (!matched) return max;
    return Math.max(max, Number.parseInt(matched[1], 10));
  }, 0) + 1;
}

export default function SettingsModal(props: SettingsModalProps) {
  const {
    visible,
    settingsTab,
    tabIndicatorStyle,
    draftProvider,
    draftChunkInput,
    draftApiKey,
    draftAutoPunc,
    draftApiBase,
    draftApiModel,
    testingLlm,
    hotwordCorrection,
    onClose,
    onSave,
    onTestLlmConnection,
    onSettingsTabChange,
    onDraftProviderChange,
    onDraftChunkInputChange,
    onDraftApiKeyChange,
    onDraftAutoPuncChange,
    onDraftApiBaseChange,
    onDraftApiModelChange,
    onHotwordCorrectionChange,
  } = props;

  const [newHotword, setNewHotword] = useState("");
  const [selectedHotwordIndex, setSelectedHotwordIndex] = useState<number | null>(null);
  const [newGroupName, setNewGroupName] = useState("");
  const [editingGroupId, setEditingGroupId] = useState<string | null>(null);
  const [editingGroupName, setEditingGroupName] = useState("");

  const activeGroup = useMemo(
    () => hotwordCorrection.groups.find((group) => group.id === hotwordCorrection.activeGroupId) ?? hotwordCorrection.groups[0],
    [hotwordCorrection.activeGroupId, hotwordCorrection.groups],
  );

  const applyHotword = (updater: (old: HotwordCorrection) => HotwordCorrection) => {
    onHotwordCorrectionChange(updater(hotwordCorrection));
  };

  const addHotword = () => {
    const input = newHotword.trim();
    if (!input || !activeGroup) return;

    const terms = input.includes(",")
      ? input.split(",").map((item) => item.trim()).filter(Boolean)
      : [input];
    if (terms.length === 0) return;

    applyHotword((old) => ({
      ...old,
      groups: old.groups.map((group) => (
        group.id === old.activeGroupId
          ? { ...group, keyterms: [...group.keyterms, ...terms] }
          : group
      )),
    }));
    setNewHotword("");
    setSelectedHotwordIndex(null);
  };

  const removeHotword = (index: number) => {
    applyHotword((old) => ({
      ...old,
      groups: old.groups.map((group) => (
        group.id === old.activeGroupId
          ? { ...group, keyterms: group.keyterms.filter((_, i) => i !== index) }
          : group
      )),
    }));
    setSelectedHotwordIndex(null);
  };

  const activateGroup = (groupId: string) => {
    applyHotword((old) => ({ ...old, activeGroupId: groupId }));
    setSelectedHotwordIndex(null);
  };

  const saveEditingGroup = () => {
    if (!editingGroupId) return;
    const name = editingGroupName.trim();
    if (!name) {
      setEditingGroupId(null);
      setEditingGroupName("");
      return;
    }
    applyHotword((old) => ({
      ...old,
      groups: old.groups.map((group) => (
        group.id === editingGroupId ? { ...group, name } : group
      )),
    }));
    setEditingGroupId(null);
    setEditingGroupName("");
  };

  const addGroup = () => {
    const name = newGroupName.trim();
    if (!name) return;
    applyHotword((old) => {
      const newGroupId = `group-${nextGroupIndex(old.groups)}`;
      return {
        ...old,
        activeGroupId: newGroupId,
        groups: [...old.groups, { id: newGroupId, name, keyterms: [] }],
      };
    });
    setNewGroupName("");
    setSelectedHotwordIndex(null);
  };

  const deleteGroup = (groupId: string) => {
    if (hotwordCorrection.groups.length <= 1) return;
    applyHotword((old) => {
      const groups = old.groups.filter((group) => group.id !== groupId);
      const activeGroupId = old.activeGroupId === groupId ? groups[0]?.id ?? "group-0" : old.activeGroupId;
      return { ...old, groups, activeGroupId };
    });
    setSelectedHotwordIndex(null);
  };

  const dialogRef = useDialogA11y(visible, onClose);
  if (!visible) return null;

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
        <div className="settings-tabs-nav">
          <div className="settings-tab-indicator" style={tabIndicatorStyle} />
          <button className={`settings-tab-btn ${settingsTab === "transcribe" ? "active" : ""}`} onClick={() => onSettingsTabChange("transcribe")}>
            转录
          </button>
          <button className={`settings-tab-btn ${settingsTab === "llm" ? "active" : ""}`} onClick={() => onSettingsTabChange("llm")}>
            LLM
          </button>
          <button className={`settings-tab-btn ${settingsTab === "hotword" ? "active" : ""}`} onClick={() => onSettingsTabChange("hotword")}>
            热词矫正
          </button>
          <button className={`settings-tab-btn ${settingsTab === "advanced" ? "active" : ""}`} onClick={() => onSettingsTabChange("advanced")}>
            高级
          </button>
        </div>
        <div className="settings-body">
          {settingsTab === "transcribe" ? (
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
              <div className="settings-section">
                <h3 className="apple-heading-small">转录流程</h3>
                <div className="api-config-form">
                  <div className="settings-toggles">
                    <label className="setting-toggle">
                      <input type="checkbox" checked={draftAutoPunc} onChange={(e) => onDraftAutoPuncChange(e.target.checked)} />
                      <span className="toggle-label">
                        <span className="toggle-title">AI 标点优化</span>
                        <span className="toggle-desc">优化标点与断句，提升字幕可读性。</span>
                      </span>
                      <span className="toggle-switch" />
                    </label>
                  </div>
                </div>
              </div>
            </div>
          ) : null}

          {settingsTab === "llm" ? (
            <div className="settings-tab-content active">
              <div className="settings-section">
                <h3 className="apple-heading-small">LLM 接口</h3>
                <div className="api-config-form">
                  <div className="form-group">
                    <label>密钥</label>
                    <input
                      className="apple-input"
                      type="password"
                      value={draftApiKey}
                      onChange={(e) => onDraftApiKeyChange(e.target.value)}
                      placeholder="sk-..."
                      autoComplete="off"
                    />
                  </div>
                  <div className="form-group">
                    <label>接口地址</label>
                    <input
                      className="apple-input"
                      value={draftApiBase}
                      onChange={(e) => onDraftApiBaseChange(e.target.value)}
                      placeholder="https://api.example.com"
                    />
                  </div>
                  <div className="llm-model-test-row">
                    <div className="form-group llm-model-field">
                      <label>模型</label>
                      <input
                        className="apple-input llm-model-input"
                        value={draftApiModel}
                        onChange={(e) => onDraftApiModelChange(e.target.value)}
                        placeholder="gpt-4.1-mini"
                        autoComplete="off"
                      />
                    </div>
                    <button className="apple-button apple-button-secondary llm-test-btn" onClick={() => { void onTestLlmConnection(); }} disabled={testingLlm}>
                      {testingLlm ? "测试中..." : "测试连通性"}
                    </button>
                  </div>
                </div>
              </div>
            </div>
          ) : null}

          {settingsTab === "hotword" ? (
            <div className="settings-tab-content active" onClick={() => setSelectedHotwordIndex(null)}>
              <div className="settings-section">
                <div className="section-header">
                  <h3 className="apple-heading-small">热词矫正</h3>
                  <label className="switch">
                    <input
                      type="checkbox"
                      checked={hotwordCorrection.enabled}
                      onChange={(e) => {
                        applyHotword((old) => ({ ...old, enabled: e.target.checked }));
                      }}
                    />
                    <span className="slider" />
                  </label>
                </div>

                <div className="hotword-groups-tabs">
                  {hotwordCorrection.groups.map((group) => {
                    const isActive = group.id === hotwordCorrection.activeGroupId;
                    const isEditing = editingGroupId === group.id;
                    return (
                      <div
                        key={group.id}
                        className={`hotword-group-tab ${isActive ? "active" : ""}`}
                        onClick={() => activateGroup(group.id)}
                      >
                        <span className="group-name-icon">▦</span>
                        {isEditing ? (
                          <input
                            className="group-name-input"
                            value={editingGroupName}
                            onChange={(e) => setEditingGroupName(e.target.value)}
                            onClick={(e) => e.stopPropagation()}
                            onBlur={saveEditingGroup}
                            onKeyDown={(e) => {
                              if (e.key === "Enter") {
                                e.preventDefault();
                                saveEditingGroup();
                              }
                            }}
                            autoFocus
                          />
                        ) : (
                          <span className="group-name">{group.name}</span>
                        )}
                        <span className="group-count">({group.keyterms.length})</span>
                        <div className="group-actions" onClick={(e) => e.stopPropagation()}>
                          {isEditing ? (
                            <button className="group-action-btn" type="button" onClick={saveEditingGroup} aria-label="保存分组名称">✓</button>
                          ) : (
                            <>
                              <button
                                className="group-action-btn"
                                type="button"
                                aria-label="编辑分组名称"
                                onClick={() => {
                                  setEditingGroupId(group.id);
                                  setEditingGroupName(group.name);
                                }}
                              >
                                ✎
                              </button>
                              {hotwordCorrection.groups.length > 1 ? (
                                <button className="group-action-btn" type="button" aria-label="删除分组" onClick={() => deleteGroup(group.id)}>×</button>
                              ) : null}
                            </>
                          )}
                        </div>
                      </div>
                    );
                  })}
                  {newGroupName ? (
                    <div className="hotword-group-tab" onClick={(e) => e.stopPropagation()}>
                      <input
                        className="group-name-input"
                        value={newGroupName}
                        onChange={(e) => setNewGroupName(e.target.value)}
                        onBlur={() => {
                          if (newGroupName.trim()) addGroup();
                          else setNewGroupName("");
                        }}
                        onKeyDown={(e) => {
                          if (e.key === "Enter") {
                            e.preventDefault();
                            addGroup();
                          }
                        }}
                        autoFocus
                      />
                      <button className="group-action-btn" type="button" onClick={addGroup}>✓</button>
                    </div>
                  ) : (
                    <button className="hotword-group-tab hotword-group-add-btn" type="button" aria-label="新建分组" onClick={() => setNewGroupName("新建分组")} title="新建分组">
                      +
                    </button>
                  )}
                </div>

                <div className="hotwords-config">
                  <div className="hotwords-list">
                    {(activeGroup?.keyterms ?? []).map((term, index) => {
                      const selected = selectedHotwordIndex === index;
                      const [original, translation] = term.includes(":") ? term.split(":", 2) : [term, ""];
                      return (
                        <div
                          key={`${term}-${index}`}
                          className={`hotword-item ${selected ? "selected" : ""}`}
                          onClick={(e) => {
                            e.stopPropagation();
                            setSelectedHotwordIndex(selected ? null : index);
                          }}
                        >
                          <div className="hotword-content">
                            {translation ? (
                              <>
                                <span className="hotword-original">{original.trim()}</span>
                                <span className="hotword-arrow">→</span>
                                <span className="hotword-translation">{translation.trim()}</span>
                              </>
                            ) : (
                              <span className="hotword-text">{term}</span>
                            )}
                          </div>
                          <button
                            className="hotword-actions"
                            type="button"
                            onClick={(e) => {
                              e.stopPropagation();
                              removeHotword(index);
                            }}
                            aria-label="删除热词"
                          >
                            ×
                          </button>
                        </div>
                      );
                    })}
                  </div>

                  <div className="hotwords-add">
                    <input
                      className="apple-input"
                      value={newHotword}
                      onChange={(e) => setNewHotword(e.target.value)}
                      onKeyDown={(e) => {
                        if (e.key === "Enter") {
                          e.preventDefault();
                          addHotword();
                        }
                      }}
                  placeholder="单个：AI:Artificial Intelligence；批量：API,SDK,CLI"
                    />
                    <button className="apple-button apple-button-secondary" type="button" onClick={addHotword}>添加</button>
                  </div>
                </div>
              </div>
            </div>
          ) : null}

          {settingsTab === "advanced" ? (
            <div className="settings-tab-content active">
              <div className="settings-section">
                <h3 className="apple-heading-small">高级参数</h3>
                <div className="api-config-form">
                  <p className="apple-body">当前版本暂无高级参数。</p>
                </div>
              </div>
            </div>
          ) : null}
        </div>
        <div className="settings-footer">
          <button className="apple-button" onClick={onSave}>保存设置</button>
        </div>
      </div>
    </div>
  );
}
