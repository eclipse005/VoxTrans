import { EditIcon, TrashIcon } from "./Icons";
import type { TermEntry } from "../types";

type TermsModalProps = {
  visible: boolean;
  termsCount: number;
  termSource: string;
  termTarget: string;
  termNote: string;
  termSearch: string;
  showImportTerms: boolean;
  importTermsText: string;
  filteredTerms: TermEntry[];
  selectedTermId: string | null;
  editingTermId: string | null;
  editSource: string;
  editTarget: string;
  editNote: string;
  onClose: () => void;
  onAddTerm: () => void;
  onExportTerms: () => void;
  onClearTerms: () => void;
  onToggleImportTerms: () => void;
  onImportTerms: () => void;
  onRemoveTerm: (id: string) => void;
  onStartEditTerm: (term: TermEntry) => void;
  onCancelEditTerm: () => void;
  onSaveEditTerm: () => void;
  onTermSourceChange: (value: string) => void;
  onTermTargetChange: (value: string) => void;
  onTermNoteChange: (value: string) => void;
  onTermSearchChange: (value: string) => void;
  onImportTermsTextChange: (value: string) => void;
  onSelectedTermIdChange: (id: string | null) => void;
  onEditSourceChange: (value: string) => void;
  onEditTargetChange: (value: string) => void;
  onEditNoteChange: (value: string) => void;
};

export default function TermsModal(props: TermsModalProps) {
  const {
    visible,
    termsCount,
    termSource,
    termTarget,
    termNote,
    termSearch,
    showImportTerms,
    importTermsText,
    filteredTerms,
    selectedTermId,
    editingTermId,
    editSource,
    editTarget,
    editNote,
    onClose,
    onAddTerm,
    onExportTerms,
    onClearTerms,
    onToggleImportTerms,
    onImportTerms,
    onRemoveTerm,
    onStartEditTerm,
    onCancelEditTerm,
    onSaveEditTerm,
    onTermSourceChange,
    onTermTargetChange,
    onTermNoteChange,
    onTermSearchChange,
    onImportTermsTextChange,
    onSelectedTermIdChange,
    onEditSourceChange,
    onEditTargetChange,
    onEditNoteChange,
  } = props;

  if (!visible) return null;

  return (
    <div className="modal-overlay">
      <div className="modal-content modal-content-terms">
        <button className="modal-close" onClick={onClose}>×</button>
        <div className="terms-header">
          <h3 className="apple-heading-medium">术语表</h3>
          <span className="terms-count">{termsCount}</span>
        </div>
        <div className="terms-body">
          <div className="settings-section">
            <h3 className="apple-heading-small">新增术语</h3>
            <div className="terms-add-form">
              <input className="terms-input" placeholder="源词" value={termSource} onChange={(e) => onTermSourceChange(e.target.value)} />
              <input className="terms-input" placeholder="目标词" value={termTarget} onChange={(e) => onTermTargetChange(e.target.value)} />
              <input className="terms-input terms-input-notes" placeholder="备注（可选）" value={termNote} onChange={(e) => onTermNoteChange(e.target.value)} />
              <button className="apple-button" onClick={onAddTerm}>添加</button>
            </div>
          </div>

          <div className="settings-section">
            <div className="terms-actions">
              <button className="apple-button apple-button-secondary" onClick={onExportTerms}>导出(复制JSON)</button>
              <button className="apple-button apple-button-secondary" onClick={onToggleImportTerms}>导入文本</button>
              <button className="apple-button apple-button-secondary" onClick={onClearTerms}>清空术语</button>
            </div>
            {showImportTerms ? (
              <div className="terms-import-panel">
                <textarea
                  className="terms-import-textarea"
                  placeholder={"每行一个术语：\nApple = 苹果\nstreaming = 流式"}
                  value={importTermsText}
                  onChange={(e) => onImportTermsTextChange(e.target.value)}
                />
                <div className="terms-import-actions">
                  <button className="apple-button" onClick={onImportTerms}>确认导入</button>
                  <button className="apple-button apple-button-secondary" onClick={onToggleImportTerms}>取消</button>
                </div>
              </div>
            ) : null}
          </div>

          <div className="settings-section">
            <div className="terms-list-header">
              <h3 className="apple-heading-small">术语列表</h3>
              <input className="terms-search-input" placeholder="搜索术语" value={termSearch} onChange={(e) => onTermSearchChange(e.target.value)} />
            </div>
            <div className="terms-list" onClick={() => onSelectedTermIdChange(null)}>
              {filteredTerms.length === 0 ? (
                <div className="terms-empty">暂无术语</div>
              ) : (
                filteredTerms.map((term) => (
                  editingTermId === term.id ? (
                    <div key={term.id} className="term-item-editing" onClick={(e) => e.stopPropagation()}>
                      <div className="term-edit-form">
                        <div className="term-edit-row">
                          <div className="term-edit-field">
                            <label>原文</label>
                            <input value={editSource} onChange={(e) => onEditSourceChange(e.target.value)} placeholder="原文" />
                          </div>
                          <div className="term-edit-field">
                            <label>译文</label>
                            <input value={editTarget} onChange={(e) => onEditTargetChange(e.target.value)} placeholder="译文" />
                          </div>
                          <div className="term-edit-field">
                            <label>说明</label>
                            <input value={editNote} onChange={(e) => onEditNoteChange(e.target.value)} placeholder="说明（可选）" />
                          </div>
                        </div>
                        <div className="term-edit-actions">
                          <button className="apple-button" onClick={onSaveEditTerm}>保存</button>
                          <button className="apple-button apple-button-secondary" onClick={onCancelEditTerm}>取消</button>
                        </div>
                      </div>
                    </div>
                  ) : (
                    <div
                      key={term.id}
                      className={`term-item ${selectedTermId === term.id ? "selected" : ""}`}
                      onClick={(e) => {
                        e.stopPropagation();
                        onSelectedTermIdChange(selectedTermId === term.id ? null : term.id);
                      }}
                    >
                      <div className="term-tag-content">
                        <span className="term-original">{term.source}</span>
                        <span className="term-arrow">→</span>
                        <span className="term-translation">{term.target}</span>
                      </div>
                      <div className="term-actions" onClick={(e) => e.stopPropagation()}>
                        <button className="term-action-btn" title="编辑术语" onClick={() => onStartEditTerm(term)}>
                          <EditIcon />
                        </button>
                        <button className="term-action-btn term-action-delete" title="删除术语" onClick={() => onRemoveTerm(term.id)}>
                          <TrashIcon />
                        </button>
                      </div>
                    </div>
                  )
                ))
              )}
            </div>
          </div>
        </div>
        <div className="settings-footer">
          <button className="apple-button" onClick={onClose}>完成</button>
          <button className="apple-button apple-button-secondary" onClick={onClose}>关闭</button>
        </div>
      </div>
    </div>
  );
}
