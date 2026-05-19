import { useCallback, useMemo, useReducer, useState } from "react";
import type { ExportSrtItem } from "./api/transcribe";
import MediaList from "./components/MediaList";
import LogsModal from "./components/LogsModal";
import Navbar from "./components/Navbar";
import SettingsModal from "./components/SettingsModal";
import SubtitleExportModal from "./components/SubtitleExportModal";
import SubtitleEditorModal from "./components/SubtitleEditorModal";
import TerminologyModal from "./components/TerminologyModal";
import Toast from "./components/Toast";
import UpdateModal from "./components/UpdateModal";
import UploadPanel from "./components/UploadPanel";
import { openTaskOutputDir } from "./api/system";
import { useAppPersistence } from "./hooks/useAppPersistence";
import { useAutoUpdateCheck } from "./hooks/useAutoUpdateCheck";
import { useClickSound } from "./hooks/useClickSound";
import { useModelManager } from "./hooks/useModelManager";
import { useQueueWorkflow } from "./hooks/useQueueWorkflow";
import { useSettingsController } from "./hooks/useSettingsController";
import { useSubtitleWorkflow } from "./hooks/useSubtitleWorkflow";
import { useTaskLogs } from "./hooks/useTaskLogs";
import { useToast } from "./hooks/useToast";
import { useWorkspacePersistence } from "./hooks/useWorkspacePersistence";
import { appReducer, initialAppState } from "./state/appReducer";

const SUBTITLE_EXPORT_ITEMS_KEY = "voxtrans.subtitleExportItems.v1";
const ALL_EXPORT_ITEMS: ExportSrtItem[] = [
  "source",
  "target",
  "bilingualSourceFirst",
  "bilingualTargetFirst",
];

function loadSavedExportItems(): ExportSrtItem[] {
  try {
    const raw = window.localStorage.getItem(SUBTITLE_EXPORT_ITEMS_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed.filter((item): item is ExportSrtItem => ALL_EXPORT_ITEMS.includes(item));
  } catch {
    return [];
  }
}

function saveExportItems(items: ExportSrtItem[]) {
  try {
    window.localStorage.setItem(SUBTITLE_EXPORT_ITEMS_KEY, JSON.stringify(items));
  } catch {
    // Ignore storage errors.
  }
}

function App() {
  const [state, dispatch] = useReducer(appReducer, initialAppState);
  const [showTerminologyModal, setShowTerminologyModal] = useState(false);
  const [showSubtitleExportModal, setShowSubtitleExportModal] = useState(false);
  const [savedExportItems, setSavedExportItems] = useState<ExportSrtItem[]>(() => loadSavedExportItems());
  const {
    queue,
    activeId,
    dragActive,
    activeTab,
    showSettings,
    showLogs,
    settings,
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
  const {
    hasAvailableUpdate,
    availableUpdate,
    showUpdateDialog,
    installing,
    installProgress,
    openUpdateDialog,
    closeUpdateDialog,
    installUpdate,
    cancelInstall,
  } = useAutoUpdateCheck();

  useAppPersistence(dispatch);
  useClickSound(settings.enableClickSound);
  const { workspaceHydrated } = useWorkspacePersistence({
    dispatch,
  });

  const {
    queueCount,
    queueBusy,
    pickFiles,
    processQueue,
    processSingle,
    processSingleTranscribeTranslate,
    updateTaskLanguages,
    updateAllTaskLanguages,
    clearQueue,
    removeItem,
    downloadYoutube,
    ytDlpVersion,
    ytDlpUpdating,
    updateYtDlpBinary,
  } = useQueueWorkflow({
    queue,
    settings,
    dispatch,
    pushToast,
  });

  const {
    activeItem,
    canEditSubtitle,
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
    asrStatusByModel,
    alignStatus,
    demucsStatus,
    refreshModelStatus,
    startModelDownload,
    cancelModelDownload,
    openModelDir,
  } = useModelManager({
    pushToast,
    asrModel: settings.asrModel,
    alignModel: settings.alignModel,
    demucsModel: settings.demucsModel,
  });
  const {
    taskName: logTaskName,
    logContent,
    logChannel,
    loadingLogs,
    totalTokens,
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
    form,
    setForm,
  } = useSettingsController({
    settings,
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
  const canExportTranslated = useMemo(
    () => subtitleCues.some((cue) => cue.translatedText.trim().length > 0),
    [subtitleCues],
  );

  return (
    <div className="apple-style app-root">
      <Navbar
        onOpenSettings={openSettings}
        onOpenTerminology={() => setShowTerminologyModal(true)}
        hasAvailableUpdate={hasAvailableUpdate}
        onOpenUpdateDialog={openUpdateDialog}
      />

      <main className="apple-container apple-section">
        <section className="workspace-left">
          <UploadPanel
            activeTab={activeTab}
            dragActive={dragActive}
            youtubeUrl={youtubeUrl}
            ytDlpVersion={ytDlpVersion}
            ytDlpUpdating={ytDlpUpdating}
            onTabChange={(tab) => dispatch({ type: "set_ui", payload: { activeTab: tab } })}
            onPickFiles={pickFiles}
            onYoutubeUrlChange={(value) => dispatch({ type: "set_ui", payload: { youtubeUrl: value } })}
            onYoutubeDownload={() => {
              void downloadYoutube(youtubeUrl);
            }}
            onUpdateYtDlp={() => {
              void updateYtDlpBinary();
            }}
          />

          <MediaList
            queue={queue}
            queueCount={queueCount}
            workspaceHydrated={workspaceHydrated}
            activeId={activeId}
            isProcessing={queueBusy}
            onSetActiveId={(id) => dispatch({ type: "set_ui", payload: { activeId: activeId === id ? "" : id } })}
            onProcessQueue={processQueue}
            onClearQueue={clearQueue}
            onProcessSingle={processSingle}
            onProcessSingleTranscribeTranslate={processSingleTranscribeTranslate}
            onUpdateTaskLanguages={updateTaskLanguages}
            onUpdateAllTaskLanguages={updateAllTaskLanguages}
            onRemoveItem={removeItem}
          />
        </section>

        <section className="workspace-right">
          <div className={`subtitle-panel-layer subtitle-panel-layer-editor ${activeItem ? "is-visible" : "is-hidden"}`}>
            <SubtitleEditorModal
              embedded
              visible
              canEdit={canEditSubtitle}
              readOnlyReason={canEditSubtitle ? "" : "任务完成后才可编辑字幕"}
              taskName={subtitleTaskName}
              cues={subtitleCues}
              cueWarningsById={subtitleCueWarnings}
              onUpdateCue={updateCue}
              onAddCueAfter={addCueAfter}
              onMergeSelected={mergeSelectedCues}
              onSplitSelected={splitSelectedCues}
              onReplaceText={replaceTextInCues}
              onDeleteCue={removeCue}
              onOpenSrtDir={openSubtitleDir}
              onExportSrt={() => {
                setShowSubtitleExportModal(true);
              }}
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
        draftProvider={form.provider}
        draftChunkInput={form.chunkInput}
        draftSubtitleLengthPreset={form.subtitleLengthPreset}
        draftAsrModel={form.asrModel}
        draftAlignModel={form.alignModel}
        draftDemucsModel={form.demucsModel}
        draftEnableVocalSeparation={form.enableVocalSeparation}
        draftTranslateApiKey={form.translateApiKey}
        draftTranslateBaseUrl={form.translateBaseUrl}
        draftTranslateModel={form.translateModel}
        draftLlmConcurrencyInput={form.llmConcurrencyInput}
        draftEnableTerminology={form.enableTerminology}
        draftEnableSubtitleBeautify={form.enableSubtitleBeautify}
        draftEnableClickSound={form.enableClickSound}
        draftAutoBurnHardSubtitle={form.autoBurnHardSubtitle}
        draftSubtitleBurnMode={form.subtitleBurnMode}
        draftSubtitleRenderStyle={form.subtitleRenderStyle}
        asrStatus={asrStatus}
        asrStatusByModel={asrStatusByModel}
        alignStatus={alignStatus}
        demucsStatus={demucsStatus}
        onClose={() => dispatch({ type: "set_ui", payload: { showSettings: false } })}
        onSave={saveSettings}
        onDraftProviderChange={(value) => setForm((prev) => ({ ...prev, provider: value }))}
        onDraftChunkInputChange={(value) => setForm((prev) => ({ ...prev, chunkInput: value }))}
        onDraftSubtitleLengthPresetChange={(value) => setForm((prev) => ({ ...prev, subtitleLengthPreset: value }))}
        onDraftAsrModelChange={(value) => setForm((prev) => ({ ...prev, asrModel: value }))}
        onDraftAlignModelChange={(value) => setForm((prev) => ({ ...prev, alignModel: value }))}
        onDraftDemucsModelChange={(value) => setForm((prev) => ({ ...prev, demucsModel: value }))}
        onDraftEnableVocalSeparationChange={(value) => setForm((prev) => ({ ...prev, enableVocalSeparation: value }))}
        onDraftTranslateApiKeyChange={(value) => setForm((prev) => ({ ...prev, translateApiKey: value }))}
        onDraftTranslateBaseUrlChange={(value) => setForm((prev) => ({ ...prev, translateBaseUrl: value }))}
        onDraftTranslateModelChange={(value) => setForm((prev) => ({ ...prev, translateModel: value }))}
        onDraftLlmConcurrencyInputChange={(value) => setForm((prev) => ({ ...prev, llmConcurrencyInput: value }))}
        onDraftEnableTerminologyChange={(value) => setForm((prev) => ({ ...prev, enableTerminology: value }))}
        onDraftEnableSubtitleBeautifyChange={(value) => setForm((prev) => ({ ...prev, enableSubtitleBeautify: value }))}
        onDraftEnableClickSoundChange={(value) => setForm((prev) => ({ ...prev, enableClickSound: value }))}
        onDraftAutoBurnHardSubtitleChange={(value) => setForm((prev) => ({ ...prev, autoBurnHardSubtitle: value }))}
        onDraftSubtitleBurnModeChange={(value) => setForm((prev) => ({ ...prev, subtitleBurnMode: value }))}
        onDraftSubtitleRenderStyleChange={(value) => setForm((prev) => ({ ...prev, subtitleRenderStyle: value }))}
        onTestTranslateConnection={testTranslateConnection}
        onOpenModelDir={openModelDir}
        onStartModelDownload={startModelDownload}
        onCancelModelDownload={cancelModelDownload}
      />

      <LogsModal
        visible={showLogs}
        loading={loadingLogs}
        totalTokens={totalTokens}
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
        groups={form.terminologyGroups}
        onClose={() => setShowTerminologyModal(false)}
        onChange={(value) => setForm((prev) => ({ ...prev, terminologyGroups: value }))}
        onSave={async (groups) => {
          await saveTerminologyGroups(groups);
        }}
      />

      {showSubtitleExportModal ? (
        <SubtitleExportModal
          canExportTranslated={canExportTranslated}
          initialSelectedItems={savedExportItems}
          onClose={() => setShowSubtitleExportModal(false)}
          onConfirm={async (items) => {
            setSavedExportItems(items);
            saveExportItems(items);
            await exportSubtitleSrt(items);
            setShowSubtitleExportModal(false);
          }}
        />
      ) : null}

      <UpdateModal
        visible={showUpdateDialog}
        update={availableUpdate}
        installing={installing}
        installProgress={installProgress}
        onClose={closeUpdateDialog}
        onInstall={installUpdate}
        onCancelInstall={cancelInstall}
      />

      <Toast toast={toast} />
    </div>
  );
}

export default App;
