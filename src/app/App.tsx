import { useCallback, useMemo, useReducer, useState } from "react";
import MediaList from "./components/MediaList";
import LogsModal from "./components/LogsModal";
import Navbar from "./components/Navbar";
import SettingsModal from "./components/SettingsModal";
import SubtitleEditorModal from "./components/SubtitleEditorModal";
import TerminologyModal from "./components/TerminologyModal";
import Toast from "./components/Toast";
import UploadPanel from "./components/UploadPanel";
import { openTaskOutputDir } from "./api/system";
import { useAppPersistence } from "./hooks/useAppPersistence";
import { useModelManager } from "./hooks/useModelManager";
import { useQueueWorkflow } from "./hooks/useQueueWorkflow";
import { useSettingsController } from "./hooks/useSettingsController";
import { useSubtitleWorkflow } from "./hooks/useSubtitleWorkflow";
import { useTaskLogs } from "./hooks/useTaskLogs";
import { useToast } from "./hooks/useToast";
import { useWorkspacePersistence } from "./hooks/useWorkspacePersistence";
import { appReducer, initialAppState } from "./state/appReducer";

function App() {
  const [state, dispatch] = useReducer(appReducer, initialAppState);
  const [showTerminologyModal, setShowTerminologyModal] = useState(false);
  const {
    queue,
    activeId,
    dragActive,
    activeTab,
    showSettings,
    showLogs,
    settings,
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
    draftTerminologyGroups,
    draftEnableTerminology,
    draftEnablePunctuationOptimization,
    youtubeUrl,
    toast,
    subtitleTaskId,
    subtitleTaskName,
    subtitleMediaPath,
    subtitleSrtPath,
    subtitleCues,
    subtitleCueWarnings,
    subtitleDirty,
  } = state;

  const { pushToast } = useToast(dispatch);

  useAppPersistence(dispatch);
  useWorkspacePersistence({
    dispatch,
  });

  const {
    queueCount,
    queueBusy,
    pickFiles,
    processQueue,
    processSingle,
    processSingleTranscribeTranslate,
    clearQueue,
    removeItem,
  } = useQueueWorkflow({
    queue,
    settings,
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
    exportSubtitleSrt,
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

  const activeQueueItem = useMemo(
    () => queue.find((item) => item.id === activeId) ?? null,
    [queue, activeId],
  );
  const {
    asrStatus,
    demucsStatus,
    refreshModelStatus,
    startModelDownload,
    cancelModelDownload,
    openModelDir,
  } = useModelManager({
    pushToast,
    demucsModel: draftDemucsModel,
  });
  const {
    taskName: logTaskName,
    logContent,
    logChannel,
    loadingLogs,
    loadLogs,
    setLogChannel,
    openLogs,
    clearLogs,
    openLogDir,
  } = useTaskLogs({
    showLogs,
    activeQueueItem,
    dispatch,
    pushToast,
  });
  const {
    openSettings,
    saveSettings,
    saveTerminologyGroups,
    testTranslateConnection,
  } = useSettingsController({
    settings,
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
    draftTerminologyGroups,
    draftEnableTerminology,
    draftEnablePunctuationOptimization,
    dispatch,
    pushToast,
    refreshModelStatus,
  });

  const openSubtitleDir = useCallback(async () => {
    try {
      await openTaskOutputDir({
        taskId: subtitleTaskId,
        mediaPath: subtitleMediaPath,
      });
    } catch (error) {
      const message = error instanceof Error ? error.message : "打开字幕目录失败";
      pushToast(message, "error");
    }
  }, [pushToast, subtitleMediaPath, subtitleTaskId]);

  return (
    <div className="apple-style app-root">
      <Navbar
        onOpenSettings={openSettings}
        onOpenTerminology={() => setShowTerminologyModal(true)}
      />

      <main className="apple-container apple-section">
        <section className="workspace-left">
          <UploadPanel
            activeTab={activeTab}
            dragActive={dragActive}
            youtubeUrl={youtubeUrl}
            onTabChange={(tab) => dispatch({ type: "set_ui", payload: { activeTab: tab } })}
            onPickFiles={pickFiles}
            onYoutubeUrlChange={(value) => dispatch({ type: "set_ui", payload: { youtubeUrl: value } })}
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
            onProcessSingle={processSingle}
            onProcessSingleTranscribeTranslate={processSingleTranscribeTranslate}
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
              onUpdateCue={updateCue}
              onAddCueAfter={addCueAfter}
              onMergeSelected={mergeSelectedCues}
              onSplitSelected={splitSelectedCues}
              onReplaceText={replaceTextInCues}
              onDeleteCue={removeCue}
              onOpenSrtDir={openSubtitleDir}
              onExportSrt={exportSubtitleSrt}
              onOpenLogs={openLogs}
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
        draftProvider={draftProvider}
        draftChunkInput={draftChunkInput}
        draftSubtitleMaxWordsInput={draftSubtitleMaxWordsInput}
        draftAsrModel={draftAsrModel}
        draftDemucsModel={draftDemucsModel}
        draftEnableVocalSeparation={draftEnableVocalSeparation}
        draftTranslateApiKey={draftTranslateApiKey}
        draftTranslateBaseUrl={draftTranslateBaseUrl}
        draftTranslateModel={draftTranslateModel}
        draftLlmConcurrencyInput={draftLlmConcurrencyInput}
        draftEnableTerminology={draftEnableTerminology}
        draftEnablePunctuationOptimization={draftEnablePunctuationOptimization}
        asrStatus={asrStatus}
        demucsStatus={demucsStatus}
        onClose={() => dispatch({ type: "set_ui", payload: { showSettings: false } })}
        onSave={saveSettings}
        onDraftProviderChange={(value) => dispatch({ type: "set_draft", payload: { draftProvider: value } })}
        onDraftChunkInputChange={(value) => dispatch({ type: "set_draft", payload: { draftChunkInput: value } })}
        onDraftSubtitleMaxWordsInputChange={(value) => dispatch({ type: "set_draft", payload: { draftSubtitleMaxWordsInput: value } })}
        onDraftAsrModelChange={(value) => dispatch({ type: "set_draft", payload: { draftAsrModel: value } })}
        onDraftDemucsModelChange={(value) => dispatch({ type: "set_draft", payload: { draftDemucsModel: value } })}
        onDraftEnableVocalSeparationChange={(value) => dispatch({ type: "set_draft", payload: { draftEnableVocalSeparation: value } })}
        onDraftTranslateApiKeyChange={(value) => dispatch({ type: "set_draft", payload: { draftTranslateApiKey: value } })}
        onDraftTranslateBaseUrlChange={(value) => dispatch({ type: "set_draft", payload: { draftTranslateBaseUrl: value } })}
        onDraftTranslateModelChange={(value) => dispatch({ type: "set_draft", payload: { draftTranslateModel: value } })}
        onDraftLlmConcurrencyInputChange={(value) => dispatch({ type: "set_draft", payload: { draftLlmConcurrencyInput: value } })}
        onDraftEnableTerminologyChange={(value) => dispatch({ type: "set_draft", payload: { draftEnableTerminology: value } })}
        onDraftEnablePunctuationOptimizationChange={(value) => dispatch({ type: "set_draft", payload: { draftEnablePunctuationOptimization: value } })}
        onTestTranslateConnection={testTranslateConnection}
        onOpenModelDir={openModelDir}
        onStartModelDownload={startModelDownload}
        onCancelModelDownload={cancelModelDownload}
      />

      <LogsModal
        visible={showLogs}
        loading={loadingLogs}
        taskName={logTaskName}
        content={logContent}
        channel={logChannel}
        onChannelChange={setLogChannel}
        onClose={() => dispatch({ type: "set_ui", payload: { showLogs: false } })}
        onRefresh={loadLogs}
        onClear={clearLogs}
        onOpenDir={openLogDir}
      />

      <TerminologyModal
        visible={showTerminologyModal}
        groups={draftTerminologyGroups}
        onClose={() => setShowTerminologyModal(false)}
        onChange={(value) => dispatch({ type: "set_draft", payload: { draftTerminologyGroups: value } })}
        onSave={async (groups) => {
          await saveTerminologyGroups(groups);
        }}
      />

      <Toast toast={toast} />
    </div>
  );
}

export default App;





