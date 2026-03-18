import { useEffect, useMemo, useState } from "react";
import type { TerminologyGroup } from "../../features/media/types";
import { CheckIcon, CloseIcon, EditIcon, PlusIcon, TrashIcon } from "./Icons";
import { useDialogA11y } from "./useDialogA11y";
import {
  createTerminologyGroup,
  normalizeTerminologyGroups,
  parseInlineTerminologyInput,
} from "../utils/terminology";

type TerminologyModalProps = {
  visible: boolean;
  groups: TerminologyGroup[];
  onClose: () => void;
  onChange: (groups: TerminologyGroup[]) => void;
  onSave?: (groups: TerminologyGroup[]) => void | Promise<void>;
};

export default function TerminologyModal({ visible, groups, onClose, onChange, onSave }: TerminologyModalProps) {
  const dialogRef = useDialogA11y(visible, onClose);
  const [selectedGroupId, setSelectedGroupId] = useState<string>("");
  const [editingGroupId, setEditingGroupId] = useState<string>("");
  const [editingGroupName, setEditingGroupName] = useState<string>("");
  const [singleInput, setSingleInput] = useState("");

  const selectedGroup = useMemo(
    () => groups.find((g) => g.id === selectedGroupId) ?? groups[0] ?? null,
    [groups, selectedGroupId],
  );

  useEffect(() => {
    if (!visible) return;
    if (groups.length > 0) return;
    const next = normalizeTerminologyGroups(groups);
    onChange(next);
  }, [visible, groups, onChange]);

  if (!visible) return null;

  function ensureSelection(next: TerminologyGroup[]) {
    if (next.length === 0) {
      setSelectedGroupId("");
      return;
    }
    if (!next.some((g) => g.id === selectedGroupId)) {
      setSelectedGroupId(next[0].id);
    }
  }

  function addGroup() {
    const next = [...groups, createTerminologyGroup()];
    onChange(next);
    setSelectedGroupId(next[next.length - 1].id);
  }

  function removeGroup(groupId: string) {
    if (groups.length <= 1) return;
    const next = groups.filter((g) => g.id !== groupId);
    onChange(next);
    ensureSelection(next);
  }

  function startEditGroup(groupId: string, currentName: string) {
    setEditingGroupId(groupId);
    setEditingGroupName(currentName);
  }

  function applyEditGroup() {
    if (!editingGroupId) return;
    const nextName = editingGroupName.trim();
    if (!nextName) {
      setEditingGroupId("");
      setEditingGroupName("");
      return;
    }
    const next = groups.map((g) => (g.id === editingGroupId ? { ...g, name: nextName } : g));
    onChange(next);
    setEditingGroupId("");
    setEditingGroupName("");
  }

  function updateTerms(nextTerms: TerminologyGroup["terms"]) {
    if (!selectedGroup) return;
    onChange(groups.map((g) => (g.id === selectedGroup.id ? { ...g, terms: nextTerms } : g)));
  }

  function addSingleTerm() {
    if (!selectedGroup) return;
    const parsed = parseInlineTerminologyInput(singleInput);
    if (parsed.terms.length === 0) return;
    updateTerms([...selectedGroup.terms, ...parsed.terms]);
    setSingleInput("");
  }

  function removeTerm(termId: string) {
    if (!selectedGroup) return;
    updateTerms(selectedGroup.terms.filter((term) => term.id !== termId));
  }

  return (
    <div className="modal-overlay">
      <div
        ref={dialogRef}
        className="modal-content modal-content-terms"
        role="dialog"
        aria-modal="true"
        aria-labelledby="terms-modal-title"
        tabIndex={-1}
      >
        <button className="modal-close" onClick={onClose} aria-label="关闭术语管理">×</button>
        <div className="terms-header">
          <h3 id="terms-modal-title" className="apple-heading-medium">术语管理</h3>
          <span className="terms-count">{groups.reduce((acc, g) => acc + g.terms.length, 0)}</span>
        </div>
        <div className="terms-body">
          <div className="settings-section">
            <div className="terms-groups-row">
              <div className="hotword-groups-tabs">
                {groups.map((group) => (
                  <div
                    key={group.id}
                    className={`hotword-group-tab ${selectedGroup?.id === group.id ? "active" : ""}`}
                    role="button"
                    tabIndex={0}
                    onClick={() => setSelectedGroupId(group.id)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter" || e.key === " ") {
                        e.preventDefault();
                        setSelectedGroupId(group.id);
                      }
                    }}
                  >
                    {editingGroupId === group.id ? (
                      <input
                        className="apple-input terms-group-name-input"
                        value={editingGroupName}
                        onChange={(e) => setEditingGroupName(e.target.value)}
                        onClick={(e) => e.stopPropagation()}
                        onKeyDown={(e) => {
                          if (e.key === "Enter") {
                            e.preventDefault();
                            applyEditGroup();
                          }
                          if (e.key === "Escape") {
                            setEditingGroupId("");
                            setEditingGroupName("");
                          }
                        }}
                        autoFocus
                      />
                    ) : (
                      <span>
                        {group.name}
                        {group.terms.length > 0 ? `(${group.terms.length})` : ""}
                      </span>
                    )}
                    <span className="group-actions">
                      {editingGroupId === group.id ? (
                        <>
                          <button
                            type="button"
                            className="group-action-btn"
                            onClick={(e) => {
                              e.stopPropagation();
                              applyEditGroup();
                            }}
                            aria-label="保存组名"
                          >
                            <CheckIcon />
                          </button>
                          <button
                            type="button"
                            className="group-action-btn"
                            onClick={(e) => {
                              e.stopPropagation();
                              setEditingGroupId("");
                              setEditingGroupName("");
                            }}
                            aria-label="取消编辑"
                          >
                            <CloseIcon />
                          </button>
                        </>
                      ) : (
                        <>
                          <button
                            type="button"
                            className="group-action-btn"
                            onClick={(e) => {
                              e.stopPropagation();
                              startEditGroup(group.id, group.name);
                            }}
                            aria-label="编辑组名"
                          >
                            <EditIcon />
                          </button>
                          <button
                            type="button"
                            className="group-action-btn"
                            onClick={(e) => {
                              e.stopPropagation();
                              removeGroup(group.id);
                            }}
                            disabled={groups.length <= 1}
                            aria-label="删除分组"
                          >
                            <TrashIcon />
                          </button>
                        </>
                      )}
                    </span>
                  </div>
                ))}
              </div>
              <button
                type="button"
                className="nav-button hotword-group-add-btn"
                onClick={addGroup}
                title="新建分组"
                aria-label="新建分组"
              >
                <PlusIcon />
                <span>添加</span>
              </button>
            </div>
          </div>

          <div className="settings-section terms-editor-section">
            <div className="terms-list-header">
              <h4 className="apple-heading-small">术语编辑</h4>
            </div>
            {!selectedGroup ? (
              <div className="terms-empty">请先新建一个分组。</div>
            ) : (
              <>
                <div className="terms-add-form">
                  <input
                    className="terms-input"
                    value={singleInput}
                    onChange={(e) => setSingleInput(e.target.value)}
                    placeholder="原文:译文:说明，多条用逗号分隔，如 LLM:大模型, hypertension:高血压:医疗术语"
                  />
                  <button type="button" className="nav-button" onClick={addSingleTerm}>添加</button>
                </div>

                <div className="terms-table-wrap terms-chip-wrap">
                  {selectedGroup.terms.length === 0 ? (
                    <div className="terms-empty-row">当前分组暂无术语</div>
                  ) : (
                    <div className="terms-chip-list">
                      {selectedGroup.terms.map((term) => (
                        <div key={term.id} className="terms-chip">
                          <span className="terms-chip-text">
                            {term.origin}
                            {" \u2192 "}
                            {term.target}
                            {term.note.trim() ? `(${term.note.trim()})` : ""}
                          </span>
                          <button
                            type="button"
                            className="terms-chip-delete"
                            onClick={() => removeTerm(term.id)}
                            title="删除术语"
                            aria-label="删除术语"
                          >
                            ×
                          </button>
                        </div>
                      ))}
                    </div>
                  )}
                </div>
              </>
            )}
          </div>
        </div>
        <div className="settings-footer">
          <button
            className="nav-button"
            onClick={() => {
              if (onSave) {
                void onSave(groups);
                return;
              }
              onClose();
            }}
            title="保存术语"
            aria-label="保存术语"
          >
            <CheckIcon />
            <span>保存</span>
          </button>
        </div>
      </div>
    </div>
  );
}
