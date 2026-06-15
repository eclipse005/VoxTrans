import { useCallback, useMemo, useReducer, useState } from "react";
import type { ExportSrtItem } from "./api/transcribe";
import { ModalLayer } from "./components/ModalLayer";
import Navbar from "./components/Navbar";
import { WorkspaceScreen } from "./components/WorkspaceScreen";
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
import { SettingsFormContext } from "./contexts/SettingsFormContext";

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
    skipVersion,
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
    updateTaskTerminology,
    updateAllTaskLanguages,
    clearQueue,
    removeItem,
    downloadYoutube,
    ytDlpVersion,
    ytDlpUpdating,
    updateYtDlpBinary,
  } = useQueueWorkflow({
    queue,
    dispatch,
    pushToast,
    activeTerminologyGroupId: settings.activeTerminologyGroupId,
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

  const settingsFormContextValue = useMemo(
    () => ({
      form,
      setForm,
      asrStatus,
      asrStatusByModel,
      alignStatus,
      demucsStatus,
      saveSettings,
      testTranslateConnection,
      openModelDir,
      startModelDownload,
      cancelModelDownload,
    }),
    [
      form,
      setForm,
      asrStatus,
      asrStatusByModel,
      alignStatus,
      demucsStatus,
      saveSettings,
      testTranslateConnection,
      openModelDir,
      startModelDownload,
      cancelModelDownload,
    ],
  );

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

      <WorkspaceScreen
        queue={queue}
        queueCount={queueCount}
        workspaceHydrated={workspaceHydrated}
        activeId={activeId}
        activeItem={activeItem}
        activeTab={activeTab}
        dragActive={dragActive}
        youtubeUrl={youtubeUrl}
        ytDlpVersion={ytDlpVersion}
        ytDlpUpdating={ytDlpUpdating}
        queueBusy={queueBusy}
        canEditSubtitle={canEditSubtitle}
        subtitleTaskName={subtitleTaskName}
        subtitleCues={subtitleCues}
        subtitleCueWarnings={subtitleCueWarnings}
        dispatch={dispatch}
        onPickFiles={pickFiles}
        onYoutubeDownload={() => {
          void downloadYoutube(youtubeUrl);
        }}
        onUpdateYtDlp={() => {
          void updateYtDlpBinary();
        }}
        onProcessQueue={processQueue}
        onClearQueue={clearQueue}
        onProcessSingle={processSingle}
        onProcessSingleTranscribeTranslate={processSingleTranscribeTranslate}
        onUpdateTaskLanguages={updateTaskLanguages}
        onUpdateTaskTerminology={updateTaskTerminology}
        terminologyGroups={settings.terminologyGroups}
        onUpdateAllTaskLanguages={updateAllTaskLanguages}
        onRemoveItem={removeItem}
        onUpdateCue={updateCue}
        onAddCueAfter={addCueAfter}
        onMergeSelected={mergeSelectedCues}
        onSplitSelected={splitSelectedCues}
        onReplaceText={replaceTextInCues}
        onDeleteCue={removeCue}
        onOpenSubtitleDir={openSubtitleDir}
        onOpenSubtitleExport={() => setShowSubtitleExportModal(true)}
        onOpenLogs={openLogs}
      />

      <SettingsFormContext.Provider value={settingsFormContextValue}>
        <ModalLayer
          showSettings={showSettings}
          showLogs={showLogs}
          showTerminologyModal={showTerminologyModal}
          showSubtitleExportModal={showSubtitleExportModal}
          showUpdateDialog={showUpdateDialog}
          canExportTranslated={canExportTranslated}
          savedExportItems={savedExportItems}
          form={form}
          toast={toast}
          logTaskName={logTaskName}
          logContent={logContent}
          logChannel={logChannel}
          loadingLogs={loadingLogs}
          totalTokens={totalTokens}
          availableUpdate={availableUpdate}
          installing={installing}
          installProgress={installProgress}
          dispatch={dispatch}
          setForm={setForm}
          setShowTerminologyModal={setShowTerminologyModal}
          setShowSubtitleExportModal={setShowSubtitleExportModal}
          setSavedExportItems={setSavedExportItems}
          saveExportItems={saveExportItems}
          saveTerminologyGroups={saveTerminologyGroups}
          exportSubtitleSrt={exportSubtitleSrt}
          loadLogs={loadLogs}
          setLogChannel={setLogChannel}
          clearLogs={clearLogs}
          openLogDir={openLogDir}
          closeUpdateDialog={closeUpdateDialog}
          installUpdate={installUpdate}
          cancelInstall={cancelInstall}
          skipVersion={skipVersion}
        />
      </SettingsFormContext.Provider>
    </div>
  );
}

export default App;
