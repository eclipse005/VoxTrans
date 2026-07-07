import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
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
  activeGroupId: string;
  onClose: () => void;
  onChange: (groups: TerminologyGroup[]) => void;
  onChangeActiveGroupId: (groupId: string) => void;
  onSave?: (groups: TerminologyGroup[]) => void | Promise<void>;
};

export default function TerminologyModal({
  visible,
  groups,
  activeGroupId,
  onClose,
  onChange,
  onChangeActiveGroupId,
  onSave,
}: TerminologyModalProps) {
  const { t } = useTranslation(["tasks", "common"]);
  const dialogRef = useDialogA11y(visible, onClose);
  const [editingGroupId, setEditingGroupId] = useState<string>("");
  const [editingGroupName, setEditingGroupName] = useState<string>("");
  const [singleInput, setSingleInput] = useState("");

  const selectedGroup = useMemo(
    () => groups.find((g) => g.id === activeGroupId) ?? null,
    [groups, activeGroupId],
  );

  // Click a group tab to toggle it as the active (task-default) group; click
  // again to deselect (no default). The active group is also the edit target.
  function toggleActive(groupId: string) {
    onChangeActiveGroupId(activeGroupId === groupId ? "" : groupId);
  }

  useEffect(() => {
    if (!visible) return;
    if (groups.length > 0) return;
    const next = normalizeTerminologyGroups(groups);
    onChange(next);
  }, [visible, groups, onChange]);

  if (!visible) return null;

  function ensureSelection(next: TerminologyGroup[]) {
    if (activeGroupId && !next.some((g) => g.id === activeGroupId)) {
      onChangeActiveGroupId("");
    }
  }

  function addGroup() {
    const next = [...groups, createTerminologyGroup()];
    onChange(next);
    onChangeActiveGroupId(next[next.length - 1].id);
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
        <button className="modal-close" onClick={onClose} aria-label={t("tasks.terminology.close")}>×</button>
        <div className="terms-header">
          <h3 id="terms-modal-title" className="apple-heading-medium">{t("tasks.terminology.title")}</h3>
          <span className="terms-count">{groups.reduce((acc, g) => acc + g.terms.length, 0)}</span>
        </div>
        <div className="terms-body">
          <div className="settings-section">
            <div className="terms-groups-row">
              <div className="terminology-groups-tabs">
                {groups.map((group) => (
                  <div
                    key={group.id}
                    className={`terminology-group-tab ${activeGroupId === group.id ? "active" : ""}`}
                    role="button"
                    tabIndex={0}
                    onClick={() => toggleActive(group.id)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter" || e.key === " ") {
                        e.preventDefault();
                        toggleActive(group.id);
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
                            aria-label={t("tasks.terminology.saveGroup")}
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
                            aria-label={t("tasks.terminology.cancelEdit")}
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
                            aria-label={t("tasks.terminology.editGroup")}
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
                            aria-label={t("tasks.terminology.deleteGroup")}
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
                title={t("tasks.terminology.addGroup")}
                aria-label={t("tasks.terminology.addGroup")}
              >
                <PlusIcon />
                <span>{t("tasks.terminology.add")}</span>
              </button>
            </div>
          </div>

          <div className="settings-section terms-editor-section">
            <div className="terms-list-header">
              <h4 className="apple-heading-small">{t("tasks.terminology.editTitle")}</h4>
            </div>
            {!selectedGroup ? (
              <div className="terms-empty">
                {groups.length === 0
                  ? t("tasks.terminology.emptyCreate")
                  : t("tasks.terminology.emptyHint")}
              </div>
            ) : (
              <>
                <div className="terms-add-form">
                  <input
                    className="terms-input"
                    value={singleInput}
                    onChange={(e) => setSingleInput(e.target.value)}
                    placeholder={t("tasks.terminology.addPlaceholder")}
                  />
                  <button type="button" className="nav-button" onClick={addSingleTerm}>{t("tasks.terminology.add")}</button>
                </div>

                <div className="terms-table-wrap terms-chip-wrap">
                  {selectedGroup.terms.length === 0 ? (
                    <div className="terms-empty-row">{t("tasks.terminology.emptyTerms")}</div>
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
                            title={t("tasks.terminology.deleteTerm")}
                            aria-label={t("tasks.terminology.deleteTerm")}
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
            title={t("tasks.terminology.save")}
            aria-label={t("tasks.terminology.save")}
          >
            <CheckIcon />
            <span>{t("common:button.save")}</span>
          </button>
        </div>
      </div>
    </div>
  );
}
