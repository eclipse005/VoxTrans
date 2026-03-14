import { useCallback, useEffect, useMemo, useReducer, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  ModelDownloadStateSnapshot,
  ModelStatusResponse,
  SavedSettings,
} from "../features/media/types";
import MediaList from "./components/MediaList";
import LogsModal from "./components/LogsModal";
import Navbar from "./components/Navbar";
import SettingsModal from "./components/SettingsModal";
import SubtitleEditorModal from "./components/SubtitleEditorModal";
import Toast from "./components/Toast";
import UploadPanel from "./components/UploadPanel";
import { useAppPersistence } from "./hooks/useAppPersistence";
import { useQueueWorkflow } from "./hooks/useQueueWorkflow";
import { useSubtitleWorkflow } from "./hooks/useSubtitleWorkflow";
import { useToast } from "./hooks/useToast";
import { useWorkspacePersistence } from "./hooks/useWorkspacePersistence";
import { appReducer, initialAppState } from "./state/appReducer";

function App() {
  const [state, dispatch] = useReducer(appReducer, initialAppState);
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
    youtubeUrl,
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
  const [logTaskContext, setLogTaskContext] = useState<{
    taskId: string;
    mediaPath: string;
    taskName: string;
  } | null>(null);
  const [logContent, setLogContent] = useState("");
  const [loadingLogs, setLoadingLogs] = useState(false);
  const [modelDir, setModelDir] = useState("");
  const [modelReady, setModelReady] = useState(false);
  const [modelDownload, setModelDownload] = useState<ModelDownloadStateSnapshot>({
    phase: "idle",
    downloadedBytes: 0,
    totalBytes: 0,
    speedBytesPerSec: 0,
    message: "",
  });
  const [modelBusy, setModelBusy] = useState(false);
  const lastModelStatusRefreshAtRef = useRef(0);

  useAppPersistence(dispatch);
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
  const loadLogs = useCallback(async () => {
    if (!logTaskContext) return;
    setLoadingLogs(true);
    try {
      const content = await invoke<string>("read_task_log", {
        request: {
          taskId: logTaskContext.taskId,
          mediaPath: logTaskContext.mediaPath,
          channel: "main",
        },
      });
      setLogContent(content || "");
    } catch (error) {
      const message = error instanceof Error ? error.message : "加载日志失败";
      pushToast(message, "error");
    } finally {
      setLoadingLogs(false);
    }
  }, [logTaskContext, pushToast]);

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
    setLogContent("");
    dispatch({ type: "set_ui", payload: { showLogs: true } });
  }, [activeQueueItem, dispatch, pushToast]);

  const openSubtitleDir = useCallback(async () => {
    try {
      if (subtitleTaskId && subtitleMediaPath) {
        await invoke("open_task_output_dir", {
          request: {
            taskId: subtitleTaskId,
            mediaPath: subtitleMediaPath,
          },
        });
      } else {
        await invoke("open_output_dir");
      }
    } catch (error) {
      const message = error instanceof Error ? error.message : "打开字幕目录失败";
      pushToast(message, "error");
    }
  }, [pushToast, subtitleMediaPath, subtitleTaskId]);

  const clearLogs = useCallback(async () => {
    if (!logTaskContext) return;
    try {
      await invoke("clear_task_logs", {
        request: {
          taskId: logTaskContext.taskId,
          mediaPath: logTaskContext.mediaPath,
          channel: "main",
        },
      });
      setLogContent("");
      pushToast("日志已清空", "success");
    } catch (error) {
      const message = error instanceof Error ? error.message : "清空日志失败";
      pushToast(message, "error");
    }
  }, [logTaskContext, pushToast]);

  const openLogDir = useCallback(async () => {
    try {
      if (logTaskContext?.taskId && logTaskContext.mediaPath) {
        await invoke("open_task_output_dir", {
          request: {
            taskId: logTaskContext.taskId,
            mediaPath: logTaskContext.mediaPath,
          },
        });
      } else {
        await invoke("open_output_dir");
      }
    } catch (error) {
      const message = error instanceof Error ? error.message : "打开日志目录失败";
      pushToast(message, "error");
    }
  }, [logTaskContext, pushToast]);

  useEffect(() => {
    if (!showLogs || !logTaskContext) return;
    void loadLogs();
  }, [showLogs, logTaskContext, loadLogs]);

  const refreshModelStatus = useCallback(async () => {
    try {
      const status = await invoke<ModelStatusResponse>("get_model_status");
      setModelDir(status.modelDir);
      setModelReady(status.ready);
      setModelDownload(status.download);
      setModelBusy(status.download.phase === "downloading");
    } catch (error) {
      const message = error instanceof Error ? error.message : "读取模型状态失败";
      pushToast(message, "error");
    }
  }, [pushToast]);

  useEffect(() => {
    void refreshModelStatus();
  }, [refreshModelStatus]);

  useEffect(() => {
    let unlisten: undefined | (() => void);
    listen<ModelDownloadStateSnapshot>("model-download-progress", (event) => {
      const payload = event.payload;
      if (!payload) return;
      setModelDownload(payload);
      if (payload.phase === "downloading") {
        const now = Date.now();
        if (now - lastModelStatusRefreshAtRef.current >= 1000) {
          lastModelStatusRefreshAtRef.current = now;
          void refreshModelStatus();
        }
      } else {
        setModelBusy(false);
        void refreshModelStatus();
      }
    })
      .then((fn) => {
        unlisten = fn;
      })
      .catch(() => {});

    return () => {
      if (unlisten) unlisten();
    };
  }, [refreshModelStatus]);

  const openSettings = useCallback(() => {
    void refreshModelStatus();
    dispatch({ type: "set_draft", payload: {
      draftProvider: settings.provider,
      draftChunkInput: String(settings.chunkTargetSeconds),
      draftSubtitleMaxWordsInput: String(settings.subtitleMaxWordsPerSegment),
    }});
    dispatch({ type: "set_ui", payload: { showSettings: true } });
  }, [dispatch, refreshModelStatus, settings.chunkTargetSeconds, settings.provider, settings.subtitleMaxWordsPerSegment]);

  const startModelDownload = useCallback(async () => {
    setModelBusy(true);
    try {
      await invoke("start_model_download");
      pushToast("开始后台下载模型", "info");
      await refreshModelStatus();
    } catch (error) {
      setModelBusy(false);
      const message = error instanceof Error ? error.message : "启动模型下载失败";
      pushToast(message, "error");
    }
  }, [pushToast, refreshModelStatus]);

  const cancelModelDownload = useCallback(async () => {
    setModelBusy(true);
    try {
      await invoke("cancel_model_download");
      pushToast("已请求取消下载", "info");
      await refreshModelStatus();
    } catch (error) {
      const message = error instanceof Error ? error.message : "取消下载失败";
      pushToast(message, "error");
    } finally {
      setModelBusy(false);
    }
  }, [pushToast, refreshModelStatus]);

  const openModelDir = useCallback(async () => {
    try {
      await invoke("open_model_dir");
    } catch (error) {
      const message = error instanceof Error ? error.message : "打开模型目录失败";
      pushToast(message, "error");
    }
  }, [pushToast]);

  const saveSettings = useCallback(async () => {
    const parsed = Number.parseInt(draftChunkInput.trim(), 10);
    if (!Number.isFinite(parsed)) {
      pushToast("分段时长必须是数字", "error");
      return;
    }
    const clamped = Math.max(60, Math.min(300, parsed));
    const parsedSubtitleWords = Number.parseInt(draftSubtitleMaxWordsInput.trim(), 10);
    if (!Number.isFinite(parsedSubtitleWords)) {
      pushToast("字幕长度必须是数字", "error");
      return;
    }
    const clampedSubtitleWords = Math.max(8, Math.min(40, parsedSubtitleWords));
    const nextSettings = {
      provider: draftProvider,
      chunkTargetSeconds: clamped,
      subtitleMaxWordsPerSegment: clampedSubtitleWords,
    } satisfies SavedSettings;

    dispatch({
      type: "set_settings",
      settings: nextSettings,
    });
    dispatch({ type: "set_draft", payload: {
      draftChunkInput: String(clamped),
      draftSubtitleMaxWordsInput: String(clampedSubtitleWords),
    }});

    try {
      await invoke("save_app_settings", {
        request: {
          settings: nextSettings,
        },
      });
      pushToast("设置已保存（后续任务生效）", "success");
    } catch (error) {
      const message = error instanceof Error ? error.message : "设置保存失败";
      pushToast(message, "error");
    }
  }, [
    dispatch,
    draftChunkInput,
    draftProvider,
    draftSubtitleMaxWordsInput,
    pushToast,
  ]);

  return (
    <div className="apple-style app-root">
      <Navbar
        onOpenSettings={openSettings}
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
              onOpenSrtDir={openSubtitleDir}
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
        modelDir={modelDir}
        modelReady={modelReady}
        modelDownload={modelDownload}
        modelBusy={modelBusy}
        onClose={() => dispatch({ type: "set_ui", payload: { showSettings: false } })}
        onSave={saveSettings}
        onDraftProviderChange={(value) => dispatch({ type: "set_draft", payload: { draftProvider: value } })}
        onDraftChunkInputChange={(value) => dispatch({ type: "set_draft", payload: { draftChunkInput: value } })}
        onDraftSubtitleMaxWordsInputChange={(value) => dispatch({ type: "set_draft", payload: { draftSubtitleMaxWordsInput: value } })}
        onOpenModelDir={openModelDir}
        onStartModelDownload={startModelDownload}
        onCancelModelDownload={cancelModelDownload}
      />

      <LogsModal
        visible={showLogs}
        loading={loadingLogs}
        taskName={logTaskContext?.taskName || ""}
        content={logContent}
        onClose={() => dispatch({ type: "set_ui", payload: { showLogs: false } })}
        onRefresh={loadLogs}
        onClear={clearLogs}
        onOpenDir={openLogDir}
      />

      <Toast toast={toast} />
    </div>
  );
}

export default App;





