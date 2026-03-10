import { useCallback, useEffect, useReducer, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWindow, type DragDropEvent } from "@tauri-apps/api/window";
import type { QueueItem, QueueStatus, SavedSettings, TranscribeResponse } from "../features/media/types";
import { detectMediaKind, fileName } from "../features/media/utils";
import MediaList from "./components/MediaList";
import Navbar from "./components/Navbar";
import SettingsModal from "./components/SettingsModal";
import TermsModal from "./components/TermsModal";
import Toast from "./components/Toast";
import UploadPanel from "./components/UploadPanel";
import type { TermEntry, ToastTone } from "./types";
import { appReducer, initialAppState } from "./state/appReducer";
import { reportError, toUserErrorMessage } from "./utils/errors";

function App() {
  const [state, dispatch] = useReducer(appReducer, initialAppState);
  const toastTimerRef = useRef<number | null>(null);

  const {
    queue,
    activeId,
    isProcessing,
    dragActive,
    activeTab,
    showSettings,
    showGlossary,
    settings,
    draftProvider,
    draftChunkInput,
    settingsTab,
    draftApiBase,
    draftAutoPunc,
    draftHotwordCorrection,
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
  } = state;

  const queueCount = queue.length;
  const termsCount = terms.length;
  const settingsTabIndex = settingsTab === "basic" ? 0 : settingsTab === "transcribe" ? 1 : 2;
  const tabIndicatorStyle = { ["--tab-index" as string]: settingsTabIndex } as Record<string, number>;

  const patch = useCallback((payload: Partial<typeof state>) => dispatch({ type: "patch", payload }), []);

  const pushToast = useCallback((message: string, tone: ToastTone = "info") => {
    if (toastTimerRef.current) {
      window.clearTimeout(toastTimerRef.current);
    }
    const id = Date.now();
    patch({ toast: { id, message, tone } });
    toastTimerRef.current = window.setTimeout(() => {
      patch({ toast: null });
      toastTimerRef.current = null;
    }, 2200);
  }, [patch]);

  const appendPaths = useCallback(async (paths: string[]) => {
    if (!paths.length) return;

    const incoming = await Promise.all(
      paths.map(async (path) => {
        let sizeBytes = 0;
        try {
          sizeBytes = await invoke<number>("get_file_size", { path });
        } catch {
          sizeBytes = 0;
        }

        return {
          id: `${Date.now()}-${Math.random().toString(36).slice(2, 8)}-${path}`,
          path,
          name: fileName(path),
          mediaKind: detectMediaKind(path),
          sizeBytes,
          status: "pending" as QueueStatus,
          progress: 0,
          resultText: "",
          resultSrt: "",
          rtfx: null,
          error: "",
        } satisfies QueueItem;
      }),
    );

    dispatch({ type: "add_queue_items", items: incoming });
    pushToast(`已加入队列 ${paths.length} 个文件`, "success");
  }, [pushToast]);

  useEffect(() => {
    let unlisten: undefined | (() => void);

    getCurrentWindow()
      .onDragDropEvent((event: { payload: DragDropEvent }) => {
        const payload = event.payload;
        if (!payload) return;

        if (payload.type === "enter" || payload.type === "over") {
          patch({ dragActive: true });
        } else if (payload.type === "leave") {
          patch({ dragActive: false });
        } else if (payload.type === "drop") {
          patch({ dragActive: false });
          const paths = Array.isArray(payload.paths) ? payload.paths : [];
          void appendPaths(paths);
        }
      })
      .then((fn) => {
        unlisten = fn;
      })
      .catch(() => {
        // Drag-drop listener is optional, click-upload always works.
      });

    return () => {
      if (unlisten) unlisten();
    };
  }, [appendPaths, patch]);

  useEffect(() => {
    try {
      const rawTerms = localStorage.getItem("voxtrans.terms");
      if (rawTerms) {
        const parsed = JSON.parse(rawTerms) as TermEntry[];
        if (Array.isArray(parsed)) {
          dispatch({ type: "set_terms", terms: parsed });
        }
      }
      const rawSettings = localStorage.getItem("voxtrans.settings");
      if (rawSettings) {
        const parsed = JSON.parse(rawSettings) as SavedSettings;
        if (parsed?.provider && parsed?.chunkTargetSeconds) {
          patch({
            settings: parsed,
            draftProvider: parsed.provider,
            draftChunkInput: String(parsed.chunkTargetSeconds),
          });
        }
      }
    } catch {
      // Ignore corrupted local storage.
    }
  }, [patch]);

  useEffect(() => {
    localStorage.setItem("voxtrans.terms", JSON.stringify(terms));
  }, [terms]);

  const pickFiles = async () => {
    try {
      const picked = await open({
        multiple: true,
        directory: false,
        filters: [
          {
            name: "Media",
            extensions: ["mp3", "wav", "m4a", "mp4", "mkv", "flac", "aac", "mov", "webm", "avi"],
          },
        ],
      });

      if (!picked) return;
      const paths = Array.isArray(picked) ? picked : [picked];
      await appendPaths(paths);
    } catch (error) {
      reportError(error, "pickFiles");
      pushToast(toUserErrorMessage(error, "打开文件选择器失败"), "error");
    }
  };

  const openSettings = () => {
    patch({
      draftProvider: settings.provider,
      draftChunkInput: String(settings.chunkTargetSeconds),
      settingsTab: "basic",
      showSettings: true,
    });
  };

  const saveSettings = () => {
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
    pushToast("设置已保存（后续任务生效）", "success");
  };

  const addTerm = () => {
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
  };

  const removeTerm = (id: string) => {
    dispatch({ type: "remove_term", id });
  };

  const startEditTerm = (term: TermEntry) => {
    patch({
      editingTermId: term.id,
      selectedTermId: null,
      editSource: term.source,
      editTarget: term.target,
      editNote: term.note,
    });
  };

  const cancelEditTerm = () => {
    patch({
      editingTermId: null,
      editSource: "",
      editTarget: "",
      editNote: "",
    });
  };

  const saveEditTerm = () => {
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
  };

  const exportTerms = async () => {
    try {
      const payload = JSON.stringify(terms, null, 2);
      await navigator.clipboard.writeText(payload);
      pushToast("术语已复制到剪贴板", "success");
    } catch (error) {
      reportError(error, "exportTerms");
      pushToast(toUserErrorMessage(error, "复制失败，请检查系统剪贴板权限"), "error");
    }
  };

  const importTerms = () => {
    const rows = importTermsText
      .split(/\r?\n/)
      .map((line) => line.trim())
      .filter(Boolean);
    if (!rows.length) return;

    const parsed: TermEntry[] = [];
    for (const line of rows) {
      const parts = line.split("=");
      if (parts.length < 2) continue;
      const source = parts[0].trim();
      const target = parts.slice(1).join("=").trim();
      if (!source || !target) continue;
      parsed.push({
        id: `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
        source,
        target,
        note: "",
      });
    }
    if (!parsed.length) {
      pushToast("导入格式不正确，请使用 源词 = 目标词", "error");
      return;
    }

    const existed = new Set(terms.map((item) => item.source.toLowerCase()));
    const merged = parsed.filter((item) => !existed.has(item.source.toLowerCase()));
    dispatch({ type: "set_terms", terms: [...merged, ...terms] });
    patch({
      importTermsText: "",
      showImportTerms: false,
    });
    pushToast(`已导入 ${parsed.length} 条术语`, "success");
  };

  const clearQueue = () => {
    if (isProcessing) {
      pushToast("正在处理时不能清空队列", "error");
      return;
    }
    dispatch({ type: "clear_queue" });
    pushToast("队列已清空", "info");
  };

  const runTranscribe = async (item: Pick<QueueItem, "id" | "path" | "name">) => {
    dispatch({
      type: "patch_queue_item",
      id: item.id,
      updater: (old) => ({
        ...old,
        status: "processing",
        progress: 15,
        error: "",
      }),
    });
    patch({ activeId: item.id });

    try {
      const response = await invoke<TranscribeResponse>("transcribe", {
        request: {
          audioPath: item.path,
          provider: settings.provider,
          chunkTargetSeconds: settings.chunkTargetSeconds,
        },
      });

      dispatch({
        type: "patch_queue_item",
        id: item.id,
        updater: (old) => ({
          ...old,
          status: "done",
          progress: 100,
          resultText: response.text,
          resultSrt: response.srt,
          rtfx: response.rtfx,
          error: "",
        }),
      });
      pushToast(`已完成：${item.name}，SRT 已保存到 ${response.srtOutputPath}`, "success");
    } catch (err) {
      reportError(err, "runTranscribe");
      const errorMessage = toUserErrorMessage(err, "转录失败，请检查模型和运行时配置");
      dispatch({
        type: "patch_queue_item",
        id: item.id,
        updater: (old) => ({
          ...old,
          status: "error",
          progress: 0,
          error: errorMessage,
        }),
      });
      pushToast(`失败：${item.name}，${errorMessage}`, "error");
    }
  };

  const processQueue = async () => {
    if (isProcessing) return;
    const targets = queue
      .filter((item) => item.status === "pending")
      .map((item) => ({ id: item.id, path: item.path, name: item.name }));
    if (!targets.length) {
      pushToast("没有待处理文件", "error");
      return;
    }

    patch({ isProcessing: true });
    pushToast(`开始批量处理，共 ${targets.length} 个文件`, "info");

    try {
      for (const item of targets) {
        await runTranscribe(item);
      }
    } finally {
      patch({ isProcessing: false });
    }
  };

  const processSingle = async (item: QueueItem) => {
    if (isProcessing) return;
    patch({ isProcessing: true });
    dispatch({
      type: "patch_queue_item",
      id: item.id,
      updater: (old) => ({ ...old, status: "pending", progress: 0, error: "" }),
    });

    try {
      await runTranscribe(item);
    } finally {
      patch({ isProcessing: false });
    }
  };

  const translateSingle = (item: QueueItem) => {
    patch({ activeId: item.id });
    pushToast(`转译排期中：${item.name}（功能即将接入）`, "info");
  };

  const openFolderForItem = async () => {
    try {
      await invoke("open_output_dir");
    } catch (err) {
      reportError(err, "openFolderForItem");
      pushToast(toUserErrorMessage(err, "打开 output 目录失败"), "error");
    }
  };

  const removeItem = (id: string) => {
    if (isProcessing) return;
    dispatch({ type: "remove_queue_item", id });
  };

  const filteredTerms = terms.filter((item) => {
    const keyword = termSearch.trim().toLowerCase();
    if (!keyword) return true;
    return (
      item.source.toLowerCase().includes(keyword) ||
      item.target.toLowerCase().includes(keyword) ||
      item.note.toLowerCase().includes(keyword)
    );
  });

  return (
    <div className="apple-style app-root">
      <Navbar termsCount={termsCount} onOpenTerms={() => patch({ showGlossary: true })} onOpenSettings={openSettings} />

      <main className="apple-container apple-section">
        <div className="apple-animate-on-scroll hero-section animated">
          <h2 className="apple-heading-hero">音视频转写翻译工具</h2>
          <p className="apple-body-large hero-description">Parakeet 转录 • 精准时间戳 • 智能断句 • AI 术语矫正</p>
        </div>

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
          isProcessing={isProcessing}
          onSetActiveId={(id) => patch({ activeId: id })}
          onProcessQueue={processQueue}
          onClearQueue={clearQueue}
          onTranslateSingle={translateSingle}
          onProcessSingle={processSingle}
          onOpenFolder={openFolderForItem}
          onRemoveItem={removeItem}
        />
      </main>

      <SettingsModal
        visible={showSettings}
        settingsTab={settingsTab}
        tabIndicatorStyle={tabIndicatorStyle}
        draftProvider={draftProvider}
        draftChunkInput={draftChunkInput}
        draftAutoPunc={draftAutoPunc}
        draftHotwordCorrection={draftHotwordCorrection}
        draftApiBase={draftApiBase}
        onClose={() => patch({ showSettings: false })}
        onSave={saveSettings}
        onSettingsTabChange={(tab) => patch({ settingsTab: tab })}
        onDraftProviderChange={(value) => patch({ draftProvider: value })}
        onDraftChunkInputChange={(value) => patch({ draftChunkInput: value })}
        onDraftAutoPuncChange={(value) => patch({ draftAutoPunc: value })}
        onDraftHotwordCorrectionChange={(value) => patch({ draftHotwordCorrection: value })}
        onDraftApiBaseChange={(value) => patch({ draftApiBase: value })}
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
        onExportTerms={exportTerms}
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
