import { useCallback, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { QueueItem, SubtitleCue, SubtitleLoadResponse, SubtitleSaveResponse } from "../../features/media/types";
import { buildFallbackCue, createCueAfter, cuesToSrt, parseSrtContent } from "../../features/media/srt";
import type { AppAction } from "../state/appReducer";
import { reportError, toUserErrorMessage } from "../utils/errors";
import { buildCueWarningsById } from "../utils/subtitleWarnings";

type DispatchState = (action: AppAction) => void;
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
  dispatch: DispatchState;
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
  dispatch,
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
    if (!subtitleMediaPath || !subtitleTaskId) return;

    try {
      clearSubtitleSavedIndicatorTimer();
      dispatch({ type: "set_subtitle", payload: { subtitleSaveState: "saving" } });
      const content = cuesToSrt(subtitleCues);
      const response = await invoke<SubtitleSaveResponse>("save_subtitle_editor", {
        request: {
          taskId: subtitleTaskId,
          mediaPath: subtitleMediaPath,
          content,
          autosave: !finalSave,
        },
      });

      dispatch({
        type: "set_subtitle",
        payload: {
        subtitleSaveState: "saved",
        subtitleDirty: false,
        subtitleSrtPath: response.srtPath,
        subtitleCueWarnings: buildCueWarningsById(subtitleCues, response.warnings),
      }});

      subtitleSavedIndicatorTimerRef.current = window.setTimeout(() => {
        dispatch({ type: "set_subtitle", payload: { subtitleSaveState: "idle" } });
        subtitleSavedIndicatorTimerRef.current = null;
      }, 1200);

      if (finalSave) {
        pushToast("字幕已保存", "success");
      }
    } catch (err) {
      reportError(err, "saveSubtitle");
      dispatch({ type: "set_subtitle", payload: { subtitleSaveState: "error" } });
      if (finalSave) {
        pushToast(toUserErrorMessage(err, "字幕保存失败"), "error");
      }
    }
  }, [clearSubtitleSavedIndicatorTimer, dispatch, pushToast, subtitleCues, subtitleMediaPath, subtitleTaskId]);

  const loadSubtitleEditor = useCallback(async (item: QueueItem) => {
    try {
      clearSubtitleSavedIndicatorTimer();
      dispatch({ type: "set_subtitle", payload: {
        subtitleTaskId: item.id,
        subtitleTaskName: item.name,
        subtitleMediaPath: item.path,
        subtitleSaveState: "idle",
      }});

      const response = await invoke<SubtitleLoadResponse>("load_subtitle_editor", {
        request: {
          taskId: item.id,
          mediaPath: item.path,
          fallbackSrt: item.resultSrt || null,
        },
      });

      const parsedCues = parseSrtContent(response.content);
      const effectiveCues = parsedCues.length > 0 ? parsedCues : buildFallbackCue(response.content);
      const hydratedCues = hydrateTranslatedCues(effectiveCues, item);
      dispatch({ type: "set_subtitle", payload: {
        subtitleSrtPath: response.srtPath,
        subtitleDraftPath: response.draftPath,
        subtitleCues: hydratedCues,
        subtitleCueWarnings: buildCueWarningsById(hydratedCues, response.warnings),
        subtitleDirty: false,
        subtitleSaveState: "idle",
      }});

      if (response.usingDraft) {
        pushToast("已恢复自动保存草稿", "info");
      }
    } catch (error) {
      reportError(error, "loadSubtitleEditor");
      pushToast("字幕格式有误，无法加载编辑器", "error");
      dispatch({ type: "set_subtitle", payload: {
        subtitleTaskId: item.id,
        subtitleTaskName: item.name,
        subtitleMediaPath: item.path,
        subtitleDraftPath: "",
        subtitleSrtPath: "",
        subtitleCues: [],
        subtitleCueWarnings: {},
        subtitleDirty: false,
        subtitleSaveState: "error",
      }});
    }
  }, [clearSubtitleSavedIndicatorTimer, dispatch, pushToast]);

  const markSubtitleEdited = useCallback((nextCues: SubtitleCue[]) => {
    clearSubtitleSavedIndicatorTimer();
    dispatch({ type: "set_subtitle", payload: {
      subtitleCues: nextCues,
      subtitleCueWarnings: {},
      subtitleDirty: true,
      subtitleSaveState: "idle",
    }});
  }, [clearSubtitleSavedIndicatorTimer, dispatch]);

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
    const mergedTranslatedText = selectedIndices
      .map(({ cue }) => cue.translatedText.trim())
      .filter(Boolean)
      .join("\n");

    const mergedCue: SubtitleCue = {
      ...first.cue,
      startMs: Math.min(...selectedIndices.map(({ cue }) => cue.startMs)),
      endMs: Math.max(...selectedIndices.map(({ cue }) => cue.endMs)),
      text: mergedText,
      translatedText: mergedTranslatedText,
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
      const [leftTranslatedText, rightTranslatedText] = splitCueText(cue.translatedText);

      const leftCue: SubtitleCue = {
        ...cue,
        id: `${cue.id}-a-${Math.random().toString(36).slice(2, 6)}`,
        startMs: cue.startMs,
        endMs: splitAt,
        text: leftText,
        translatedText: leftTranslatedText,
      };
      const rightCue: SubtitleCue = {
        ...cue,
        id: `${cue.id}-b-${Math.random().toString(36).slice(2, 6)}`,
        startMs: splitAt,
        endMs: cue.endMs,
        text: rightText,
        translatedText: rightTranslatedText,
      };

      bornCueIds.push({ sourceCueId: cue.id, bornCueId: rightCue.id });
      next.push(leftCue, rightCue);
    }

    markSubtitleEdited(next);
    return bornCueIds;
  }, [markSubtitleEdited, subtitleCues]);

  const replaceTextInCues = useCallback((findText: string, replaceText: string, scopeCueIds: string[] | null, maxReplacements?: number): number => {
    const source = findText;
    if (!source) return 0;

    const targetSet = scopeCueIds && scopeCueIds.length > 0 ? new Set(scopeCueIds) : null;
    const limit = maxReplacements && maxReplacements > 0 ? maxReplacements : Number.POSITIVE_INFINITY;
    let replacedCount = 0;

    const next = subtitleCues.map((cue) => {
      if (targetSet && !targetSet.has(cue.id)) {
        return cue;
      }
      if (replacedCount >= limit) {
        return cue;
      }

      const text = cue.text;
      if (!text.includes(source)) {
        return cue;
      }

      let cursor = 0;
      let nextText = "";
      let localCount = 0;
      while (cursor < text.length && replacedCount + localCount < limit) {
        const index = text.indexOf(source, cursor);
        if (index < 0) break;
        nextText += text.slice(cursor, index) + replaceText;
        cursor = index + source.length;
        localCount += 1;
      }

      if (localCount === 0) {
        return cue;
      }

      nextText += text.slice(cursor);
      replacedCount += localCount;
      return {
        ...cue,
        text: nextText,
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
      dispatch({ type: "set_subtitle", payload: {
        subtitleTaskId: "",
        subtitleTaskName: "",
        subtitleMediaPath: "",
        subtitleDraftPath: "",
        subtitleSrtPath: "",
        subtitleCues: [],
        subtitleCueWarnings: {},
        subtitleSaveState: "idle",
        subtitleDirty: false,
      }});
      return;
    }
    if (subtitleTaskId === activeItem.id) return;
    void loadSubtitleEditor(activeItem);
  }, [activeId, clearSubtitleSavedIndicatorTimer, dispatch, loadSubtitleEditor, queue, subtitleTaskId]);

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

function splitCueText(text: string): [string, string] {
  const trimmed = text.trim();
  if (!trimmed) return ["", ""];

  const words = trimmed.split(/\s+/).filter(Boolean);
  if (words.length >= 2) {
    const midWord = Math.floor(words.length / 2);
    return [words.slice(0, midWord).join(" "), words.slice(midWord).join(" ")];
  }

  const midChar = Math.max(1, Math.floor(trimmed.length / 2));
  return [trimmed.slice(0, midChar).trim(), trimmed.slice(midChar).trim()];
}

function hydrateTranslatedCues(cues: SubtitleCue[], item: QueueItem): SubtitleCue[] {
  const segments = parseSubtitleSegments(item.subtitleSegmentsJson);
  if (segments.length === 0) return cues.map((cue) => ({ ...cue, translatedText: cue.translatedText || "" }));

  return cues.map((cue, index) => {
    const byIndex = segments[index];
    const byTime = segments.find((segment) => overlapMs(cue.startMs, cue.endMs, segment.startMs, segment.endMs) > 0);
    const translatedText = (byTime?.translatedText ?? byIndex?.translatedText ?? "").trim();
    return translatedText ? { ...cue, translatedText } : { ...cue, translatedText: cue.translatedText || "" };
  });
}

function parseSubtitleSegments(raw?: string): Array<{ startMs: number; endMs: number; sourceText: string; translatedText: string }> {
  if (!raw?.trim()) return [];
  try {
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed
      .map((segment) => {
        const start = Number(segment?.startMs ?? 0);
        const end = Number(segment?.endMs ?? start);
        const sourceText = typeof segment?.sourceText === "string" ? segment.sourceText : "";
        const translatedText = typeof segment?.translatedText === "string" ? segment.translatedText : "";
        if (!Number.isFinite(start) || !Number.isFinite(end)) return null;
        return {
          startMs: Math.max(0, Math.round(start)),
          endMs: Math.max(0, Math.round(end)),
          sourceText,
          translatedText,
        };
      })
      .filter((segment): segment is { startMs: number; endMs: number; sourceText: string; translatedText: string } => segment !== null);
  } catch {
    return [];
  }
}

function overlapMs(aStart: number, aEnd: number, bStart: number, bEnd: number): number {
  return Math.max(0, Math.min(aEnd, bEnd) - Math.max(aStart, bStart));
}




