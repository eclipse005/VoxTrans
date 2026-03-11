import { useCallback, useEffect, useReducer, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWindow, type DragDropEvent } from "@tauri-apps/api/window";
import type {
  BuildSegmentsResponse,
  QueueItem,
  QueueStatus,
  SavedSettings,
  SubtitleCue,
  SubtitleLoadResponse,
  SubtitleSaveResponse,
  TranscribeResponse,
} from "../features/media/types";
import { detectMediaKind, fileName } from "../features/media/utils";
import { buildFallbackCue, createCueAfter, cuesToSrt, parseSrtContent } from "../features/media/srt";
import MediaList from "./components/MediaList";
import Navbar from "./components/Navbar";
import SettingsModal from "./components/SettingsModal";
import SubtitleEditorModal from "./components/SubtitleEditorModal";
import TermsModal from "./components/TermsModal";
import Toast from "./components/Toast";
import UploadPanel from "./components/UploadPanel";
import type { TermEntry, ToastTone } from "./types";
import { appReducer, initialAppState } from "./state/appReducer";
import { reportError, toUserErrorMessage } from "./utils/errors";

type TranscribeProgressEvent = {
  taskId: string;
  currentSegment: number;
  totalSegments: number;
};

function App() {
  const [state, dispatch] = useReducer(appReducer, initialAppState);
  const toastTimerRef = useRef<number | null>(null);
  const subtitleSaveTimerRef = useRef<number | null>(null);
  const subtitleSavedIndicatorTimerRef = useRef<number | null>(null);

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
    showSubtitleEditor,
    subtitleTaskName,
    subtitleMediaPath,
    subtitleSrtPath,
    subtitleCues,
    subtitleSaveState,
    subtitleDirty,
  } = state;

  const queueCount = queue.length;
  const hasProcessingTask = queue.some((item) => item.status === "processing");
  const hasQueuedTask = queue.some((item) => item.status === "queued");
  const queueBusy = hasProcessingTask || hasQueuedTask;
  const termsCount = terms.length;
  const settingsTabIndex = settingsTab === "basic" ? 0 : settingsTab === "transcribe" ? 1 : 2;
  const tabIndicatorStyle = { ["--tab-index" as string]: settingsTabIndex } as Record<string, number>;

  const patch = useCallback((payload: Partial<typeof state>) => dispatch({ type: "patch", payload }), []);

  const clearSubtitleSavedIndicatorTimer = useCallback(() => {
    if (subtitleSavedIndicatorTimerRef.current != null) {
      window.clearTimeout(subtitleSavedIndicatorTimerRef.current);
      subtitleSavedIndicatorTimerRef.current = null;
    }
  }, []);

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
          segmentCurrent: 0,
          segmentTotal: 0,
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
    let unlistenProgress: undefined | (() => void);

    listen<TranscribeProgressEvent>("transcribe-progress", (event) => {
      const payload = event.payload;
      if (!payload?.taskId) return;
      dispatch({
        type: "patch_queue_item",
        id: payload.taskId,
        updater: (old) => ({
          ...old,
          segmentCurrent: Math.max(0, payload.currentSegment || 0),
          segmentTotal: Math.max(0, payload.totalSegments || 0),
          progress:
            payload.totalSegments > 0
              ? Math.min(99, Math.round((Math.max(0, payload.currentSegment || 0) / payload.totalSegments) * 100))
              : old.progress,
        }),
      });
    })
      .then((fn) => {
        unlistenProgress = fn;
      })
      .catch(() => {
        // Progress events are optional.
      });

    return () => {
      if (unlistenProgress) unlistenProgress();
    };
  }, []);

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

  useEffect(() => {
    return () => {
      if (subtitleSaveTimerRef.current != null) {
        window.clearTimeout(subtitleSaveTimerRef.current);
      }
      if (subtitleSavedIndicatorTimerRef.current != null) {
        window.clearTimeout(subtitleSavedIndicatorTimerRef.current);
      }
    };
  }, []);

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
    if (queueBusy) {
      pushToast("正在处理时不能清空队列", "error");
      return;
    }
    dispatch({ type: "clear_queue" });
    pushToast("队列已清空", "info");
  };

  const runTranscribe = useCallback(async (item: Pick<QueueItem, "id" | "path" | "name">) => {
    dispatch({
      type: "patch_queue_item",
      id: item.id,
      updater: (old) => ({
        ...old,
        status: "processing",
        progress: 15,
        segmentCurrent: 0,
        segmentTotal: 0,
        error: "",
      }),
    });
    patch({ activeId: item.id });

    try {
      const response = await invoke<TranscribeResponse>("transcribe", {
        request: {
          taskId: item.id,
          audioPath: item.path,
          provider: settings.provider,
          chunkTargetSeconds: settings.chunkTargetSeconds,
        },
      });
      const built = await invoke<BuildSegmentsResponse>("build_segments_from_words", {
        request: {
          audioPath: item.path,
          words: response.words,
        },
      });

      dispatch({
        type: "patch_queue_item",
        id: item.id,
        updater: (old) => ({
          ...old,
          status: "done",
          progress: 100,
          segmentCurrent: response.segmentTotal > 0 ? response.segmentTotal : old.segmentCurrent,
          segmentTotal: response.segmentTotal > 0 ? response.segmentTotal : old.segmentTotal,
          resultText: built.text,
          resultSrt: built.srt,
          rtfx: response.rtfx,
          error: "",
        }),
      });
      pushToast(`已完成：${item.name}，SRT 已保存到 ${built.srtOutputPath}`, "success");
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
          segmentCurrent: 0,
          segmentTotal: 0,
          error: errorMessage,
        }),
      });
      pushToast(`失败：${item.name}，${errorMessage}`, "error");
    }
  }, [patch, pushToast, settings.chunkTargetSeconds, settings.provider]);

  const processQueue = async () => {
    const pendingCount = queue.filter((item) => item.status === "pending").length;
    if (!pendingCount) {
      pushToast("没有待处理文件", "error");
      return;
    }

    const queuedIds = queue
      .filter((q) => q.status === "pending")
      .map((q) => q.id);
    if (!queuedIds.length) {
      pushToast("没有待处理文件", "error");
      return;
    }
    for (const id of queuedIds) {
      dispatch({
        type: "patch_queue_item",
        id,
        updater: (old) => ({ ...old, status: "queued", progress: 0, segmentCurrent: 0, segmentTotal: 0, error: "" }),
      });
    }

    pushToast(`开始批量处理，共 ${pendingCount} 个文件`, "info");
  };

  const processSingle = async (item: QueueItem) => {
    if (item.status === "processing" || item.status === "queued") return;
    dispatch({
      type: "patch_queue_item",
      id: item.id,
      updater: (old) => ({ ...old, status: "queued", progress: 0, segmentCurrent: 0, segmentTotal: 0, error: "" }),
    });

    if (queueBusy) {
      pushToast(`已加入排队：${item.name}`, "info");
    }
  };

  useEffect(() => {
    if (hasProcessingTask) return;
    const next = queue.find((item) => item.status === "queued");
    if (!next) return;
    void runTranscribe({ id: next.id, path: next.path, name: next.name });
  }, [hasProcessingTask, queue, runTranscribe]);

  const translateSingle = (item: QueueItem) => {
    patch({ activeId: item.id });
    pushToast(`转译排期中：${item.name}（功能即将接入）`, "info");
  };

  const saveSubtitle = useCallback(async (finalSave: boolean) => {
    if (!subtitleMediaPath) return;

    try {
      clearSubtitleSavedIndicatorTimer();
      patch({ subtitleSaveState: "saving" });
      const content = cuesToSrt(subtitleCues);
      const response = await invoke<SubtitleSaveResponse>("save_subtitle_editor", {
        request: {
          mediaPath: subtitleMediaPath,
          content,
          autosave: !finalSave,
        },
      });

      patch({
        subtitleSaveState: "saved",
        subtitleDirty: false,
        subtitleSrtPath: response.srtPath,
      });

      subtitleSavedIndicatorTimerRef.current = window.setTimeout(() => {
        patch({ subtitleSaveState: "idle" });
        subtitleSavedIndicatorTimerRef.current = null;
      }, 1200);

      if (finalSave) {
        if (response.warnings.length > 0) {
          pushToast(`字幕已保存，存在 ${response.warnings.length} 条提示`, "info");
        } else {
          pushToast("字幕已保存", "success");
        }
      }
    } catch (err) {
      reportError(err, "saveSubtitle");
      patch({ subtitleSaveState: "error" });
      if (finalSave) {
        pushToast(toUserErrorMessage(err, "字幕保存失败"), "error");
      }
    }
  }, [clearSubtitleSavedIndicatorTimer, patch, pushToast, subtitleCues, subtitleMediaPath]);

  const openSubtitleEditor = async (item: QueueItem) => {
    try {
      clearSubtitleSavedIndicatorTimer();
      patch({
        subtitleTaskId: item.id,
        subtitleTaskName: item.name,
        subtitleMediaPath: item.path,
        showSubtitleEditor: true,
        subtitleSaveState: "idle",
      });

      const response = await invoke<SubtitleLoadResponse>("load_subtitle_editor", {
        request: {
          mediaPath: item.path,
          fallbackSrt: item.resultSrt || null,
        },
      });

      const parsedCues = parseSrtContent(response.content);
      const effectiveCues = parsedCues.length > 0 ? parsedCues : buildFallbackCue(response.content);
      patch({
        subtitleSrtPath: response.srtPath,
        subtitleDraftPath: response.draftPath,
        subtitleCues: effectiveCues,
        subtitleDirty: false,
        subtitleSaveState: "idle",
      });

      if (response.usingDraft) {
        pushToast("已恢复自动保存草稿", "info");
      }
      if (response.warnings.length > 0) {
        pushToast(`字幕加载完成，存在 ${response.warnings.length} 条格式提示`, "info");
      }
    } catch (error) {
      reportError(error, "openSubtitleEditor");
      pushToast(toUserErrorMessage(error, "打开字幕编辑器失败"), "error");
      patch({
        showSubtitleEditor: false,
      });
    }
  };

  const closeSubtitleEditor = async () => {
    if (subtitleDirty) {
      await saveSubtitle(true);
    }

    clearSubtitleSavedIndicatorTimer();
    patch({
      showSubtitleEditor: false,
      subtitleTaskId: "",
      subtitleTaskName: "",
      subtitleMediaPath: "",
      subtitleDraftPath: "",
      subtitleSrtPath: "",
      subtitleCues: [],
      subtitleSaveState: "idle",
      subtitleDirty: false,
    });
  };

  const markSubtitleEdited = useCallback((nextCues: SubtitleCue[]) => {
    clearSubtitleSavedIndicatorTimer();
    patch({
      subtitleCues: nextCues,
      subtitleDirty: true,
      subtitleSaveState: "idle",
    });
  }, [clearSubtitleSavedIndicatorTimer, patch]);

  useEffect(() => {
    if (!showSubtitleEditor || !subtitleMediaPath || !subtitleDirty) {
      return;
    }

    if (subtitleSaveTimerRef.current) {
      window.clearTimeout(subtitleSaveTimerRef.current);
    }

    subtitleSaveTimerRef.current = window.setTimeout(() => {
      void saveSubtitle(false);
    }, 800);

    return () => {
      if (subtitleSaveTimerRef.current) {
        window.clearTimeout(subtitleSaveTimerRef.current);
      }
    };
  }, [saveSubtitle, showSubtitleEditor, subtitleMediaPath, subtitleCues, subtitleDirty]);

  const updateCue = (cueId: string, patchCue: Partial<SubtitleCue>) => {
    markSubtitleEdited(
      subtitleCues.map((cue) =>
        cue.id === cueId
          ? {
              ...cue,
              ...patchCue,
            }
          : cue,
      ),
    );
  };

  const addCueAfter = (selectedCueId: string | null) => {
    const selectedIndex = selectedCueId ? subtitleCues.findIndex((cue) => cue.id === selectedCueId) : -1;
    if (selectedIndex < 0) {
      const lastCue = subtitleCues[subtitleCues.length - 1];
      const newCue = createCueAfter(lastCue);
      markSubtitleEdited([...subtitleCues, newCue]);
      return;
    }

    const anchorCue = subtitleCues[selectedIndex];
    const newCue = createCueAfter(anchorCue);
    const next = [...subtitleCues];
    next.splice(selectedIndex + 1, 0, newCue);
    markSubtitleEdited(next);
  };

  const mergeSelectedCues = (selectedCueIds: string[]) => {
    const selectedSet = new Set(selectedCueIds);
    const selectedIndices = subtitleCues
      .map((cue, index) => ({ cue, index }))
      .filter(({ cue }) => selectedSet.has(cue.id));

    if (selectedIndices.length < 2) return;

    selectedIndices.sort((a, b) => a.index - b.index);
    const first = selectedIndices[0];
    const mergedText = selectedIndices
      .map(({ cue }) => cue.text.trim())
      .filter(Boolean)
      .join("\n");

    const mergedCue: SubtitleCue = {
      ...first.cue,
      startMs: Math.min(...selectedIndices.map(({ cue }) => cue.startMs)),
      endMs: Math.max(...selectedIndices.map(({ cue }) => cue.endMs)),
      text: mergedText,
    };

    const mergedIds = new Set(selectedIndices.map(({ cue }) => cue.id));
    const base = subtitleCues.filter((cue) => !mergedIds.has(cue.id));
    const insertAt = Math.min(first.index, base.length);
    const next = [...base.slice(0, insertAt), mergedCue, ...base.slice(insertAt)];

    markSubtitleEdited(next);
  };

  const splitSelectedCues = (selectedCueIds: string[]): Array<{ sourceCueId: string; bornCueId: string }> => {
    if (!selectedCueIds.length) return [];

    const selectedSet = new Set(selectedCueIds);
    const next: SubtitleCue[] = [];
    const bornCueIds: Array<{ sourceCueId: string; bornCueId: string }> = [];

    for (const cue of subtitleCues) {
      if (!selectedSet.has(cue.id)) {
        next.push(cue);
        continue;
      }

      const duration = Math.max(2, cue.endMs - cue.startMs);
      const middle = cue.startMs + Math.floor(duration / 2);
      const splitAt = Math.max(cue.startMs + 1, Math.min(cue.endMs - 1, middle));

      const trimmed = cue.text.trim();
      let leftText = "";
      let rightText = "";
      if (!trimmed) {
        leftText = "";
        rightText = "";
      } else {
        const words = trimmed.split(/\s+/).filter(Boolean);
        if (words.length >= 2) {
          const midWord = Math.floor(words.length / 2);
          leftText = words.slice(0, midWord).join(" ");
          rightText = words.slice(midWord).join(" ");
        } else {
          const midChar = Math.max(1, Math.floor(trimmed.length / 2));
          leftText = trimmed.slice(0, midChar).trim();
          rightText = trimmed.slice(midChar).trim();
        }
      }

      const leftCue: SubtitleCue = {
        ...cue,
        id: `${cue.id}-a-${Math.random().toString(36).slice(2, 6)}`,
        startMs: cue.startMs,
        endMs: splitAt,
        text: leftText,
      };
      const rightCue: SubtitleCue = {
        ...cue,
        id: `${cue.id}-b-${Math.random().toString(36).slice(2, 6)}`,
        startMs: splitAt,
        endMs: cue.endMs,
        text: rightText,
      };

      bornCueIds.push({ sourceCueId: cue.id, bornCueId: rightCue.id });
      next.push(leftCue, rightCue);
    }

    markSubtitleEdited(next);
    return bornCueIds;
  };

  const replaceTextInCues = (findText: string, replaceText: string, scopeCueIds: string[] | null): number => {
    const source = findText;
    if (!source) return 0;

    const targetSet = scopeCueIds && scopeCueIds.length > 0 ? new Set(scopeCueIds) : null;
    let replacedCount = 0;

    const next = subtitleCues.map((cue) => {
      if (targetSet && !targetSet.has(cue.id)) {
        return cue;
      }

      if (!cue.text.includes(source)) {
        return cue;
      }

      const segments = cue.text.split(source);
      const occurrences = segments.length - 1;
      if (occurrences <= 0) {
        return cue;
      }

      replacedCount += occurrences;
      return {
        ...cue,
        text: segments.join(replaceText),
      };
    });

    if (replacedCount > 0) {
      markSubtitleEdited(next);
    }

    return replacedCount;
  };

  const removeCue = (cueId: string) => {
    const next = subtitleCues.filter((cue) => cue.id !== cueId);
    markSubtitleEdited(next);
  };

  const removeItem = (id: string) => {
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
          isProcessing={queueBusy}
          onSetActiveId={(id) => patch({ activeId: id })}
          onProcessQueue={processQueue}
          onClearQueue={clearQueue}
          onTranslateSingle={translateSingle}
          onProcessSingle={processSingle}
          onOpenSubtitleEditor={openSubtitleEditor}
          onRemoveItem={removeItem}
        />
      </main>

      <SubtitleEditorModal
        visible={showSubtitleEditor}
        taskName={subtitleTaskName}
        srtPath={subtitleSrtPath}
        cues={subtitleCues}
        saveState={subtitleSaveState}
        onUpdateCue={updateCue}
        onAddCueAfter={addCueAfter}
        onMergeSelected={mergeSelectedCues}
        onSplitSelected={splitSelectedCues}
        onReplaceText={replaceTextInCues}
        onDeleteCue={removeCue}
        onClose={closeSubtitleEditor}
      />

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
