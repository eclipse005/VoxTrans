import { useEffect, useMemo, useState } from "react";
import type { HotwordGroup, HotwordLang } from "../../features/media/types";
import { CheckIcon, CloseIcon, EditIcon, PlusIcon, TrashIcon } from "./Icons";
import { useDialogA11y } from "./useDialogA11y";
import {
  createHotwordGroup,
  normalizeHotwordGroups,
  parseInlineHotwordInput,
} from "../utils/hotwords";

type HotwordsModalProps = {
  visible: boolean;
  groups: HotwordGroup[];
  onClose: () => void;
  onChange: (groups: HotwordGroup[]) => void;
  onSave?: (groups: HotwordGroup[]) => void | Promise<void>;
};

const LANG_LABELS: Record<HotwordLang, string> = {
  auto: "自动",
  zh: "中文",
  non_zh: "非中文",
};

export default function HotwordsModal({ visible, groups, onClose, onChange, onSave }: HotwordsModalProps) {
  const dialogRef = useDialogA11y(visible, onClose);
  const [selectedGroupId, setSelectedGroupId] = useState<string>("");
  const [editingGroupId, setEditingGroupId] = useState<string>("");
  const [editingGroupName, setEditingGroupName] = useState<string>("");
  const [singleInput, setSingleInput] = useState("");
  const [selectedLang, setSelectedLang] = useState<HotwordLang>("auto");

  const selectedGroup = useMemo(
    () => groups.find((g) => g.id === selectedGroupId) ?? groups[0] ?? null,
    [groups, selectedGroupId],
  );

  useEffect(() => {
    if (!visible) return;
    if (groups.length > 0) return;
    const next = normalizeHotwordGroups(groups);
    onChange(next);
  }, [visible, groups, onChange]);

  if (!visible) return null;

  function ensureSelection(next: HotwordGroup[]) {
    if (next.length === 0) {
      setSelectedGroupId("");
      return;
    }
    if (!next.some((g) => g.id === selectedGroupId)) {
      setSelectedGroupId(next[0].id);
    }
  }

  function addGroup() {
    const next = [...groups, createHotwordGroup()];
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

  function updateTerms(nextTerms: HotwordGroup["terms"]) {
    if (!selectedGroup) return;
    onChange(groups.map((g) => (g.id === selectedGroup.id ? { ...g, terms: nextTerms } : g)));
  }

  function addSingleTerm() {
    if (!selectedGroup) return;
    const parsed = parseInlineHotwordInput(singleInput);
    if (parsed.terms.length === 0) return;
    updateTerms([
      ...selectedGroup.terms,
      ...parsed.terms.map((term) => ({ ...term, lang: selectedLang })),
    ]);
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
        aria-labelledby="hotwords-modal-title"
        tabIndex={-1}
      >
        <button className="modal-close" onClick={onClose} aria-label="关闭热词管理">×</button>
        <div className="terms-header">
          <h3 id="hotwords-modal-title" className="apple-heading-medium">热词管理</h3>
          <span className="terms-count">{groups.reduce((acc, g) => acc + g.terms.length, 0)}</span>
        </div>
        <div className="terms-body">
          <div className="settings-section">
            <div className="terms-groups-row">
              <div className="terminology-groups-tabs">
                {groups.map((group) => (
                  <div
                    key={group.id}
                    className={`terminology-group-tab ${selectedGroup?.id === group.id ? "active" : ""}`}
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
                className="nav-button terminology-group-add-btn"
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
              <h4 className="apple-heading-small">热词编辑</h4>
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
                    placeholder="热词=错词1,错词2；如 Claude Code=cloud code,clod code"
                  />
                  <select
                    className="apple-input terms-lang-select"
                    value={selectedLang}
                    onChange={(e) => setSelectedLang(e.target.value as HotwordLang)}
                    aria-label="热词语言"
                  >
                    <option value="auto">自动</option>
                    <option value="zh">中文</option>
                    <option value="non_zh">非中文</option>
                  </select>
                  <button type="button" className="nav-button" onClick={addSingleTerm}>添加</button>
                </div>

                <div className="terms-table-wrap terms-chip-wrap">
                  {selectedGroup.terms.length === 0 ? (
                    <div className="terms-empty-row">当前分组暂无热词</div>
                  ) : (
                    <div className="terms-chip-list">
                      {selectedGroup.terms.map((term) => (
                        <div key={term.id} className="terms-chip">
                          <span className="terms-chip-text">
                            {term.word}
                            {term.aliases.length > 0 ? ` = ${term.aliases.join(", ")}` : ""}
                            {term.lang !== "auto" ? ` · ${LANG_LABELS[term.lang]}` : ""}
                          </span>
                          <button
                            type="button"
                            className="terms-chip-delete"
                            onClick={() => removeTerm(term.id)}
                            title="删除热词"
                            aria-label="删除热词"
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
            title="保存热词"
            aria-label="保存热词"
          >
            <CheckIcon />
            <span>保存</span>
          </button>
        </div>
      </div>
    </div>
  );
}
