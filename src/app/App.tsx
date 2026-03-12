import { useCallback, useEffect, useMemo, useReducer, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { LlmTestConnectionResponse, SavedSettings, TaskLogChannel } from "../features/media/types";
import MediaList from "./components/MediaList";
import LogsModal from "./components/LogsModal";
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
import { useWorkspacePersistence } from "./hooks/useWorkspacePersistence";
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
    showLogs,
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

  const { pushToast } = useToast(dispatch);
  const [testingLlm, setTestingLlm] = useState(false);
  const [logTaskContext, setLogTaskContext] = useState<{
    taskId: string;
    mediaPath: string;
    taskName: string;
  } | null>(null);
  const [logChannel, setLogChannel] = useState<TaskLogChannel>("main");
  const [logContent, setLogContent] = useState("");
  const [loadingLogs, setLoadingLogs] = useState(false);

  useAppPersistence(terms, hotwordCorrection, dispatch);
  useWorkspacePersistence({
    queue,
    dispatch,
  });

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
    llmSettings: {
      apiKey: draftApiKey,
      apiBase: draftApiBase,
      apiModel: draftApiModel,
    },
    hotwordCorrection,
    dispatch,
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
    dispatch,
    pushToast,
  });

  const termsCount = terms.length;
  const activeQueueItem = useMemo(
    () => queue.find((item) => item.id === activeId) ?? null,
    [queue, activeId],
  );
  const settingsTabIndex = settingsTab === "transcribe" ? 0 : settingsTab === "translate" ? 1 : settingsTab === "hotword" ? 2 : 3;
  const tabIndicatorStyle = { ["--tab-index" as string]: settingsTabIndex } as Record<string, number>;
  const loadLogs = useCallback(async () => {
    if (!logTaskContext) return;
    setLoadingLogs(true);
    try {
      const content = await invoke<string>("read_task_log", {
        request: {
          taskId: logTaskContext.taskId,
          mediaPath: logTaskContext.mediaPath,
          channel: logChannel,
        },
      });
      setLogContent(content || "");
    } catch (error) {
      const message = error instanceof Error ? error.message : "加载日志失败";
      pushToast(message, "error");
    } finally {
      setLoadingLogs(false);
    }
  }, [logChannel, logTaskContext, pushToast]);

  const openLogs = useCallback(() => {
    if (!activeQueueItem) {
      pushToast("请先在左侧选中一个任务", "error");
      return;
    }
    setLogTaskContext({
      taskId: activeQueueItem.id,
      mediaPath: activeQueueItem.path,
      taskName: activeQueueItem.name,
    });
    setLogChannel("main");
    setLogContent("");
    dispatch({ type: "set_ui", payload: { showLogs: true } });
  }, [activeQueueItem, dispatch, pushToast]);

  const clearLogs = useCallback(async () => {
    if (!logTaskContext) return;
    try {
      await invoke("clear_task_logs", {
        request: {
          taskId: logTaskContext.taskId,
          mediaPath: logTaskContext.mediaPath,
          channel: logChannel,
        },
      });
      setLogContent("");
      pushToast("日志已清空", "success");
    } catch (error) {
      const message = error instanceof Error ? error.message : "清空日志失败";
      pushToast(message, "error");
    }
  }, [logChannel, logTaskContext, pushToast]);

  useEffect(() => {
    if (!showLogs || !logTaskContext) return;
    void loadLogs();
  }, [showLogs, logTaskContext, logChannel, loadLogs]);

  const openSettings = useCallback(() => {
    dispatch({ type: "set_draft", payload: {
      draftProvider: settings.provider,
      draftChunkInput: String(settings.chunkTargetSeconds),
      draftAutoPunc: settings.autoPunc,
    }});
    dispatch({ type: "set_ui", payload: {
      settingsTab: "transcribe",
      showSettings: true,
    }});
  }, [dispatch, settings.autoPunc, settings.chunkTargetSeconds, settings.provider]);

  const saveSettings = useCallback(async () => {
    const parsed = Number.parseInt(draftChunkInput.trim(), 10);
    if (!Number.isFinite(parsed)) {
      pushToast("分段时长必须是数字", "error");
      return;
    }

    const clamped = Math.max(60, Math.min(1800, parsed));
    const nextSettings = {
      provider: draftProvider,
      chunkTargetSeconds: clamped,
      autoPunc: draftAutoPunc,
    } satisfies SavedSettings;

    dispatch({
      type: "set_settings",
      settings: nextSettings,
    });
    dispatch({ type: "set_draft", payload: {
      draftChunkInput: String(clamped),
    }});

    try {
      await invoke("save_app_settings", {
        request: {
          settings: nextSettings,
          llm: {
            apiKey: draftApiKey,
            apiBase: draftApiBase,
            apiModel: draftApiModel,
          },
        },
      });
      pushToast("设置已保存（后续任务生效）", "success");
    } catch (error) {
      const message = error instanceof Error ? error.message : "设置保存失败";
      pushToast(message, "error");
    }
  }, [dispatch, draftApiBase, draftApiKey, draftApiModel, draftAutoPunc, draftChunkInput, draftProvider, pushToast]);

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
    dispatch({ type: "set_term_form", payload: {
      termSource: "",
      termTarget: "",
      termNote: "",
    }});
  }, [dispatch, pushToast, termNote, termSource, termTarget, terms]);

  const removeTerm = useCallback((id: string) => {
    dispatch({ type: "remove_term", id });
  }, []);

  const startEditTerm = useCallback((term: TermEntry) => {
    dispatch({ type: "set_term_editing", payload: {
      editingTermId: term.id,
      selectedTermId: null,
      editSource: term.source,
      editTarget: term.target,
      editNote: term.note,
    }});
  }, [dispatch]);

  const cancelEditTerm = useCallback(() => {
    dispatch({ type: "set_term_editing", payload: {
      editingTermId: null,
      editSource: "",
      editTarget: "",
      editNote: "",
    }});
  }, [dispatch]);

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
    dispatch({ type: "set_term_editing", payload: { editingTermId: null } });
    pushToast("术语已更新", "success");
  }, [dispatch, editNote, editSource, editTarget, editingTermId, pushToast]);

  const importTerms = useCallback(() => {
    const { imported, duplicateCount, invalidCount } = parseImportedTerms(importTermsText, terms);
    if (!imported.length) {
      pushToast("没有可导入术语，请检查格式", "error");
      return;
    }

    dispatch({ type: "set_terms", terms: [...imported, ...terms] });
    dispatch({ type: "set_ui", payload: {
      showImportTerms: false,
    }});
    dispatch({ type: "set_term_form", payload: {
      importTermsText: "",
    }});

    const stats: string[] = [`已导入 ${imported.length} 条`];
    if (duplicateCount > 0) stats.push(`重复 ${duplicateCount} 条`);
    if (invalidCount > 0) stats.push(`无效 ${invalidCount} 条`);
    pushToast(stats.join("，"), "success");
  }, [dispatch, importTermsText, pushToast, terms]);

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
      <Navbar
        termsCount={termsCount}
        onOpenTerms={() => dispatch({ type: "set_ui", payload: { showGlossary: true } })}
        onOpenLogs={openLogs}
        onOpenSettings={openSettings}
      />

      <main className="apple-container apple-section">
        <section className="workspace-left">
          <UploadPanel
            activeTab={activeTab}
            dragActive={dragActive}
            youtubeUrl={youtubeUrl}
            youtubeQuality={youtubeQuality}
            onTabChange={(tab) => dispatch({ type: "set_ui", payload: { activeTab: tab } })}
            onPickFiles={pickFiles}
            onYoutubeUrlChange={(value) => dispatch({ type: "set_ui", payload: { youtubeUrl: value } })}
            onYoutubeQualityChange={(value) => dispatch({ type: "set_ui", payload: { youtubeQuality: value } })}
            onYoutubeDownload={() => pushToast("YouTube 下载功能即将接入", "info")}
          />

          <MediaList
            queue={queue}
            queueCount={queueCount}
            activeId={activeId}
            isProcessing={queueBusy}
            onSetActiveId={(id) => dispatch({ type: "set_ui", payload: { activeId: activeId === id ? "" : id } })}
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
        onClose={() => dispatch({ type: "set_ui", payload: { showSettings: false } })}
        onSave={saveSettings}
        onTestLlmConnection={testLlmConnection}
        onSettingsTabChange={(tab) => dispatch({ type: "set_ui", payload: { settingsTab: tab } })}
        onDraftProviderChange={(value) => dispatch({ type: "set_draft", payload: { draftProvider: value } })}
        onDraftChunkInputChange={(value) => dispatch({ type: "set_draft", payload: { draftChunkInput: value } })}
        onDraftApiKeyChange={(value) => dispatch({ type: "set_draft", payload: { draftApiKey: value } })}
        onDraftAutoPuncChange={(value) => dispatch({ type: "set_draft", payload: { draftAutoPunc: value } })}
        onDraftApiBaseChange={(value) => dispatch({ type: "set_draft", payload: { draftApiBase: value } })}
        onDraftApiModelChange={(value) => dispatch({ type: "set_draft", payload: { draftApiModel: value } })}
        onHotwordCorrectionChange={(value) => dispatch({ type: "set_draft", payload: { hotwordCorrection: value } })}
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
        onClose={() => dispatch({ type: "set_ui", payload: { showGlossary: false } })}
        onAddTerm={addTerm}
        onClearTerms={() => dispatch({ type: "set_terms", terms: [] })}
        onToggleImportTerms={() => dispatch({ type: "set_ui", payload: { showImportTerms: !showImportTerms } })}
        onImportTerms={importTerms}
        onRemoveTerm={removeTerm}
        onStartEditTerm={startEditTerm}
        onCancelEditTerm={cancelEditTerm}
        onSaveEditTerm={saveEditTerm}
        onTermSourceChange={(value) => dispatch({ type: "set_term_form", payload: { termSource: value } })}
        onTermTargetChange={(value) => dispatch({ type: "set_term_form", payload: { termTarget: value } })}
        onTermNoteChange={(value) => dispatch({ type: "set_term_form", payload: { termNote: value } })}
        onTermSearchChange={(value) => dispatch({ type: "set_term_form", payload: { termSearch: value } })}
        onImportTermsTextChange={(value) => dispatch({ type: "set_term_form", payload: { importTermsText: value } })}
        onSelectedTermIdChange={(id) => dispatch({ type: "set_term_editing", payload: { selectedTermId: id } })}
        onEditSourceChange={(value) => dispatch({ type: "set_term_editing", payload: { editSource: value } })}
        onEditTargetChange={(value) => dispatch({ type: "set_term_editing", payload: { editTarget: value } })}
        onEditNoteChange={(value) => dispatch({ type: "set_term_editing", payload: { editNote: value } })}
      />

      <LogsModal
        visible={showLogs}
        loading={loadingLogs}
        taskName={logTaskContext?.taskName || ""}
        activeChannel={logChannel}
        content={logContent}
        onClose={() => dispatch({ type: "set_ui", payload: { showLogs: false } })}
        onRefresh={loadLogs}
        onClear={clearLogs}
        onChannelChange={setLogChannel}
      />

      <Toast toast={toast} />
    </div>
  );
}

export default App;





