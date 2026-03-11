import { useCallback, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { QueueItem, SubtitleCue, SubtitleLoadResponse, SubtitleSaveResponse } from "../../features/media/types";
import { buildFallbackCue, createCueAfter, cuesToSrt, parseSrtContent } from "../../features/media/srt";
import type { AppState } from "../state/appReducer";
import { reportError, toUserErrorMessage } from "../utils/errors";
import { buildCueWarningsById } from "../utils/subtitleWarnings";

type PatchState = (payload: Partial<AppState>) => void;
type PushToast = (message: string, tone?: "info" | "success" | "error") => void;

type UseSubtitleWorkflowArgs = {
  queue: QueueItem[];
  activeId: string;
  subtitleTaskId: string;
  subtitleTaskName: string;
  subtitleMediaPath: string;
  subtitleSrtPath: string;
  subtitleCues: SubtitleCue[];
  subtitleDirty: boolean;
  patch: PatchState;
  pushToast: PushToast;
};

export function useSubtitleWorkflow({
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
}: UseSubtitleWorkflowArgs) {
  const subtitleSaveTimerRef = useRef<number | null>(null);
  const subtitleSavedIndicatorTimerRef = useRef<number | null>(null);

  const clearSubtitleSavedIndicatorTimer = useCallback(() => {
    if (subtitleSavedIndicatorTimerRef.current != null) {
      window.clearTimeout(subtitleSavedIndicatorTimerRef.current);
      subtitleSavedIndicatorTimerRef.current = null;
    }
  }, []);

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
        subtitleCueWarnings: buildCueWarningsById(subtitleCues, response.warnings),
      });

      subtitleSavedIndicatorTimerRef.current = window.setTimeout(() => {
        patch({ subtitleSaveState: "idle" });
        subtitleSavedIndicatorTimerRef.current = null;
      }, 1200);

      if (finalSave) {
        pushToast("字幕已保存", "success");
      }
    } catch (err) {
      reportError(err, "saveSubtitle");
      patch({ subtitleSaveState: "error" });
      if (finalSave) {
        pushToast(toUserErrorMessage(err, "字幕保存失败"), "error");
      }
    }
  }, [clearSubtitleSavedIndicatorTimer, patch, pushToast, subtitleCues, subtitleMediaPath]);

  const loadSubtitleEditor = useCallback(async (item: QueueItem) => {
    try {
      clearSubtitleSavedIndicatorTimer();
      patch({
        subtitleTaskId: item.id,
        subtitleTaskName: item.name,
        subtitleMediaPath: item.path,
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
        subtitleCueWarnings: buildCueWarningsById(effectiveCues, response.warnings),
        subtitleDirty: false,
        subtitleSaveState: "idle",
      });

      if (response.usingDraft) {
        pushToast("已恢复自动保存草稿", "info");
      }
    } catch (error) {
      reportError(error, "loadSubtitleEditor");
      pushToast("字幕格式有误，无法加载编辑器", "error");
      patch({
        subtitleTaskId: item.id,
        subtitleTaskName: item.name,
        subtitleMediaPath: item.path,
        subtitleDraftPath: "",
        subtitleSrtPath: "",
        subtitleCues: [],
        subtitleCueWarnings: {},
        subtitleDirty: false,
        subtitleSaveState: "error",
      });
    }
  }, [clearSubtitleSavedIndicatorTimer, patch, pushToast]);

  const markSubtitleEdited = useCallback((nextCues: SubtitleCue[]) => {
    clearSubtitleSavedIndicatorTimer();
    patch({
      subtitleCues: nextCues,
      subtitleCueWarnings: {},
      subtitleDirty: true,
      subtitleSaveState: "idle",
    });
  }, [clearSubtitleSavedIndicatorTimer, patch]);

  useEffect(() => {
    if (!subtitleMediaPath || !subtitleDirty) {
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
  }, [saveSubtitle, subtitleMediaPath, subtitleCues, subtitleDirty]);

  const updateCue = useCallback((cueId: string, patchCue: Partial<SubtitleCue>) => {
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
  }, [markSubtitleEdited, subtitleCues]);

  const addCueAfter = useCallback((selectedCueId: string | null) => {
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
  }, [markSubtitleEdited, subtitleCues]);

  const mergeSelectedCues = useCallback((selectedCueIds: string[]) => {
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
  }, [markSubtitleEdited, subtitleCues]);

  const splitSelectedCues = useCallback((selectedCueIds: string[]): Array<{ sourceCueId: string; bornCueId: string }> => {
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
  }, [markSubtitleEdited, subtitleCues]);

  const replaceTextInCues = useCallback((findText: string, replaceText: string, scopeCueIds: string[] | null): number => {
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
  }, [markSubtitleEdited, subtitleCues]);

  const removeCue = useCallback((cueId: string) => {
    const next = subtitleCues.filter((cue) => cue.id !== cueId);
    markSubtitleEdited(next);
  }, [markSubtitleEdited, subtitleCues]);

  useEffect(() => {
    const activeItem = queue.find((item) => item.id === activeId);
    if (!activeItem) {
      if (subtitleSaveTimerRef.current != null) {
        window.clearTimeout(subtitleSaveTimerRef.current);
        subtitleSaveTimerRef.current = null;
      }
      clearSubtitleSavedIndicatorTimer();
      patch({
        subtitleTaskId: "",
        subtitleTaskName: "",
        subtitleMediaPath: "",
        subtitleDraftPath: "",
        subtitleSrtPath: "",
        subtitleCues: [],
        subtitleCueWarnings: {},
        subtitleSaveState: "idle",
        subtitleDirty: false,
      });
      return;
    }
    if (subtitleTaskId === activeItem.id) return;
    void loadSubtitleEditor(activeItem);
  }, [activeId, clearSubtitleSavedIndicatorTimer, loadSubtitleEditor, patch, queue, subtitleTaskId]);

  const activeItem = queue.find((item) => item.id === activeId) ?? null;

  return {
    activeItem,
    subtitleTaskName,
    subtitleSrtPath,
    subtitleCues,
    saveSubtitle,
    updateCue,
    addCueAfter,
    mergeSelectedCues,
    splitSelectedCues,
    replaceTextInCues,
    removeCue,
  };
}
