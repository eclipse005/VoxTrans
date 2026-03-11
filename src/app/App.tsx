import { useCallback, useMemo, useReducer, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { SavedSettings } from "../features/media/types";
import type { LlmTestConnectionResponse } from "../features/media/types";
import MediaList from "./components/MediaList";
import Navbar from "./components/Navbar";
import SettingsModal from "./components/SettingsModal";
import SubtitleEditorModal from "./components/SubtitleEditorModal";
import TermsModal from "./components/TermsModal";
import Toast from "./components/Toast";
import UploadPanel from "./components/UploadPanel";
import { useAppPersistence } from "./hooks/useAppPersistence";
import { useQueueWorkflow } from "./hooks/useQueueWorkflow";
import { useSubtitleWorkflow } from "./hooks/useSubtitleWorkflow";
import { useToast } from "./hooks/useToast";
import { appReducer, initialAppState } from "./state/appReducer";
import type { TermEntry } from "./types";
import { parseImportedTerms } from "./utils/termsImport";

function App() {
  const [state, dispatch] = useReducer(appReducer, initialAppState);
  const {
    queue,
    activeId,
    dragActive,
    activeTab,
    showSettings,
    showGlossary,
    settings,
    draftProvider,
    draftChunkInput,
    settingsTab,
    draftApiKey,
    draftApiBase,
    draftApiModel,
    draftAutoPunc,
    hotwordCorrection,
    terms,
    termSource,
    termTarget,
    termNote,
    termSearch,
    showImportTerms,
    importTermsText,
    selectedTermId,
    editingTermId,
    editSource,
    editTarget,
    editNote,
    youtubeUrl,
    youtubeQuality,
    toast,
    subtitleTaskId,
    subtitleTaskName,
    subtitleMediaPath,
    subtitleSrtPath,
    subtitleCues,
    subtitleCueWarnings,
    subtitleSaveState,
    subtitleDirty,
  } = state;

  const patch = useCallback((payload: Partial<typeof state>) => dispatch({ type: "patch", payload }), []);
  const { pushToast } = useToast(patch);
  const [testingLlm, setTestingLlm] = useState(false);

  useAppPersistence(terms, hotwordCorrection, dispatch, patch);

  const {
    queueCount,
    queueBusy,
    pickFiles,
    processQueue,
    processSingle,
    clearQueue,
    translateSingle,
    removeItem,
  } = useQueueWorkflow({
    queue,
    settings,
    dispatch,
    patch,
    pushToast,
  });

  const {
    activeItem,
    updateCue,
    addCueAfter,
    mergeSelectedCues,
    splitSelectedCues,
    replaceTextInCues,
    removeCue,
  } = useSubtitleWorkflow({
    queue,
    activeId,
    subtitleTaskId,
    subtitleTaskName,
    subtitleMediaPath,
    subtitleSrtPath,
    subtitleCues,
    subtitleDirty,
    patch,
    pushToast,
  });

  const termsCount = terms.length;
  const settingsTabIndex = settingsTab === "transcribe" ? 0 : settingsTab === "translate" ? 1 : settingsTab === "hotword" ? 2 : 3;
  const tabIndicatorStyle = { ["--tab-index" as string]: settingsTabIndex } as Record<string, number>;

  const openSettings = useCallback(() => {
    patch({
      draftProvider: settings.provider,
      draftChunkInput: String(settings.chunkTargetSeconds),
      settingsTab: "transcribe",
      showSettings: true,
    });
  }, [patch, settings.chunkTargetSeconds, settings.provider]);

  const saveSettings = useCallback(() => {
    const parsed = Number.parseInt(draftChunkInput.trim(), 10);
    if (!Number.isFinite(parsed)) {
      pushToast("分段时长必须是数字", "error");
      return;
    }

    const clamped = Math.max(60, Math.min(1800, parsed));
    const nextSettings = {
      provider: draftProvider,
      chunkTargetSeconds: clamped,
    } satisfies SavedSettings;

    patch({
      settings: nextSettings,
      draftChunkInput: String(clamped),
    });
    localStorage.setItem("voxtrans.settings", JSON.stringify(nextSettings));
    localStorage.setItem("voxtrans.llm", JSON.stringify({
      apiKey: draftApiKey,
      apiBase: draftApiBase,
      apiModel: draftApiModel,
    }));
    pushToast("设置已保存（后续任务生效）", "success");
  }, [draftApiBase, draftApiKey, draftApiModel, draftChunkInput, draftProvider, patch, pushToast]);

  const testLlmConnection = useCallback(async () => {
    if (!draftApiKey.trim()) {
      pushToast("请先填写 API Key", "error");
      return;
    }
    if (!draftApiModel.trim()) {
      pushToast("请先填写 Model", "error");
      return;
    }

    setTestingLlm(true);
    try {
      const res = await invoke<LlmTestConnectionResponse>("llm_test_connection", {
        request: {
          apiKey: draftApiKey,
          baseUrl: draftApiBase || null,
          model: draftApiModel,
          timeoutSecs: 30,
        },
      });
      if (res.ok) {
        pushToast(`连通成功：${res.model}`, "success");
      } else {
        pushToast(res.message || "连通失败", "error");
      }
    } catch (error) {
      const message = error instanceof Error ? error.message : "连通失败";
      pushToast(message, "error");
    } finally {
      setTestingLlm(false);
    }
  }, [draftApiBase, draftApiKey, draftApiModel, pushToast]);

  const addTerm = useCallback(() => {
    const source = termSource.trim();
    const target = termTarget.trim();
    if (!source || !target) {
      pushToast("术语的源词和目标词不能为空", "error");
      return;
    }

    const exists = terms.some((item) => item.source.toLowerCase() === source.toLowerCase());
    if (exists) {
      pushToast("术语已存在，请直接修改", "error");
      return;
    }

    const next = {
      id: `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
      source,
      target,
      note: termNote.trim(),
    } satisfies TermEntry;

    dispatch({ type: "add_term", term: next });
    patch({
      termSource: "",
      termTarget: "",
      termNote: "",
    });
  }, [patch, pushToast, termNote, termSource, termTarget, terms]);

  const removeTerm = useCallback((id: string) => {
    dispatch({ type: "remove_term", id });
  }, []);

  const startEditTerm = useCallback((term: TermEntry) => {
    patch({
      editingTermId: term.id,
      selectedTermId: null,
      editSource: term.source,
      editTarget: term.target,
      editNote: term.note,
    });
  }, [patch]);

  const cancelEditTerm = useCallback(() => {
    patch({
      editingTermId: null,
      editSource: "",
      editTarget: "",
      editNote: "",
    });
  }, [patch]);

  const saveEditTerm = useCallback(() => {
    if (!editingTermId) return;
    const source = editSource.trim();
    const target = editTarget.trim();
    if (!source || !target) {
      pushToast("请输入原词和目标词", "error");
      return;
    }

    dispatch({
      type: "update_term",
      id: editingTermId,
      source,
      target,
      note: editNote.trim(),
    });
    patch({ editingTermId: null });
    pushToast("术语已更新", "success");
  }, [editNote, editSource, editTarget, editingTermId, patch, pushToast]);

  const importTerms = useCallback(() => {
    const { imported, duplicateCount, invalidCount } = parseImportedTerms(importTermsText, terms);
    if (!imported.length) {
      pushToast("没有可导入术语，请检查格式", "error");
      return;
    }

    dispatch({ type: "set_terms", terms: [...imported, ...terms] });
    patch({
      importTermsText: "",
      showImportTerms: false,
    });

    const stats: string[] = [`已导入 ${imported.length} 条`];
    if (duplicateCount > 0) stats.push(`重复 ${duplicateCount} 条`);
    if (invalidCount > 0) stats.push(`无效 ${invalidCount} 条`);
    pushToast(stats.join("，"), "success");
  }, [importTermsText, patch, pushToast, terms]);

  const filteredTerms = useMemo(() => {
    const keyword = termSearch.trim().toLowerCase();
    if (!keyword) return terms;
    return terms.filter((item) => (
      item.source.toLowerCase().includes(keyword)
      || item.target.toLowerCase().includes(keyword)
      || item.note.toLowerCase().includes(keyword)
    ));
  }, [termSearch, terms]);

  return (
    <div className="apple-style app-root">
      <Navbar termsCount={termsCount} onOpenTerms={() => patch({ showGlossary: true })} onOpenSettings={openSettings} />

      <main className="apple-container apple-section">
        <section className="workspace-left">
          <UploadPanel
            activeTab={activeTab}
            dragActive={dragActive}
            youtubeUrl={youtubeUrl}
            youtubeQuality={youtubeQuality}
            onTabChange={(tab) => patch({ activeTab: tab })}
            onPickFiles={pickFiles}
            onYoutubeUrlChange={(value) => patch({ youtubeUrl: value })}
            onYoutubeQualityChange={(value) => patch({ youtubeQuality: value })}
            onYoutubeDownload={() => pushToast("YouTube 下载功能即将接入", "info")}
          />

          <MediaList
            queue={queue}
            queueCount={queueCount}
            activeId={activeId}
            isProcessing={queueBusy}
            onSetActiveId={(id) => patch({ activeId: activeId === id ? "" : id })}
            onProcessQueue={processQueue}
            onClearQueue={clearQueue}
            onTranslateSingle={translateSingle}
            onProcessSingle={processSingle}
            onRemoveItem={removeItem}
          />
        </section>

        <section className="workspace-right">
          <div className={`subtitle-panel-layer subtitle-panel-layer-editor ${activeItem ? "is-visible" : "is-hidden"}`}>
            <SubtitleEditorModal
              embedded
              visible
              taskName={subtitleTaskName}
              srtPath={subtitleSrtPath}
              cues={subtitleCues}
              cueWarningsById={subtitleCueWarnings}
              saveState={subtitleSaveState}
              onUpdateCue={updateCue}
              onAddCueAfter={addCueAfter}
              onMergeSelected={mergeSelectedCues}
              onSplitSelected={splitSelectedCues}
              onReplaceText={replaceTextInCues}
              onDeleteCue={removeCue}
              onClose={() => {}}
            />
          </div>
          <div className={`subtitle-panel-layer subtitle-panel-layer-empty ${activeItem ? "is-hidden" : "is-visible"}`}>
            <div className="subtitle-panel-empty">
              <h3 className="apple-heading-medium">字幕编辑器</h3>
              <p className="apple-body">请在左侧任务列表中选择一个媒体任务开始编辑字幕。</p>
            </div>
          </div>
        </section>
      </main>

      <SettingsModal
        visible={showSettings}
        settingsTab={settingsTab}
        tabIndicatorStyle={tabIndicatorStyle}
        draftProvider={draftProvider}
        draftChunkInput={draftChunkInput}
        draftApiKey={draftApiKey}
        draftAutoPunc={draftAutoPunc}
        draftApiBase={draftApiBase}
        draftApiModel={draftApiModel}
        testingLlm={testingLlm}
        hotwordCorrection={hotwordCorrection}
        onClose={() => patch({ showSettings: false })}
        onSave={saveSettings}
        onTestLlmConnection={testLlmConnection}
        onSettingsTabChange={(tab) => patch({ settingsTab: tab })}
        onDraftProviderChange={(value) => patch({ draftProvider: value })}
        onDraftChunkInputChange={(value) => patch({ draftChunkInput: value })}
        onDraftApiKeyChange={(value) => patch({ draftApiKey: value })}
        onDraftAutoPuncChange={(value) => patch({ draftAutoPunc: value })}
        onDraftApiBaseChange={(value) => patch({ draftApiBase: value })}
        onDraftApiModelChange={(value) => patch({ draftApiModel: value })}
        onHotwordCorrectionChange={(value) => patch({ hotwordCorrection: value })}
      />

      <TermsModal
        visible={showGlossary}
        termsCount={termsCount}
        termSource={termSource}
        termTarget={termTarget}
        termNote={termNote}
        termSearch={termSearch}
        showImportTerms={showImportTerms}
        importTermsText={importTermsText}
        filteredTerms={filteredTerms}
        selectedTermId={selectedTermId}
        editingTermId={editingTermId}
        editSource={editSource}
        editTarget={editTarget}
        editNote={editNote}
        onClose={() => patch({ showGlossary: false })}
        onAddTerm={addTerm}
        onClearTerms={() => dispatch({ type: "set_terms", terms: [] })}
        onToggleImportTerms={() => patch({ showImportTerms: !showImportTerms })}
        onImportTerms={importTerms}
        onRemoveTerm={removeTerm}
        onStartEditTerm={startEditTerm}
        onCancelEditTerm={cancelEditTerm}
        onSaveEditTerm={saveEditTerm}
        onTermSourceChange={(value) => patch({ termSource: value })}
        onTermTargetChange={(value) => patch({ termTarget: value })}
        onTermNoteChange={(value) => patch({ termNote: value })}
        onTermSearchChange={(value) => patch({ termSearch: value })}
        onImportTermsTextChange={(value) => patch({ importTermsText: value })}
        onSelectedTermIdChange={(id) => patch({ selectedTermId: id })}
        onEditSourceChange={(value) => patch({ editSource: value })}
        onEditTargetChange={(value) => patch({ editTarget: value })}
        onEditNoteChange={(value) => patch({ editNote: value })}
      />

      <Toast toast={toast} />
    </div>
  );
}

export default App;
