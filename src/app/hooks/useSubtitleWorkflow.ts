import { useCallback, useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { open } from "@tauri-apps/plugin-dialog";

import type { QueueItem, SubtitleCue } from "../../features/media/types";
import { isSubtitleEditMode, resolveSubtitleEditorMode } from "../../features/media/subtitleEditorMode";
import { buildSubtitleSegmentsFromCues, subtitleSegmentsToSrt } from "../../features/media/subtitleSegments";
import type { ExportSrtItem } from "../api/transcribe";
import { getTaskRunQueueItem } from "../api/workspace";
import type { AppAction } from "../state/appReducer";
import { reportError, toUserErrorMessage } from "../utils/errors";
import { buildCueWarningsById } from "../utils/subtitleWarnings";
import {
  addCueAfterSelection,
  mergeSelectedCueList,
  removeCueFromList,
  replaceTextInCueList,
  splitSelectedCueList,
  updateCueList,
} from "./subtitleWorkflow/cueOperations";
import { exportSubtitleVariantsToDirectory, loadSubtitleEditorData, saveSubtitleEditor } from "./subtitleWorkflow/io";
import { buildSubtitleVersion } from "./subtitleWorkflow/versionSync";

type DispatchState = (action: AppAction) => void;
type PushToast = (message: string, tone?: "info" | "success" | "error") => void;

type UseSubtitleWorkflowArgs = {
  queue: QueueItem[];
  activeId: string;
  subtitleTaskId: string;
  subtitleTaskName: string;
  subtitleSrtPath: string;
  subtitleCues: SubtitleCue[];
  subtitleDirty: boolean;
  dispatch: DispatchState;
  pushToast: PushToast;
};

/**
 * Owns subtitle editor binding to the active queue task.
 *
 * Design (two modes — see `subtitleEditorMode.ts`):
 * - **preview**: cues track `item.subtitleSegmentsJson` every version tick
 *   (streaming translation / recognition). Dirty is always false.
 * - **edit**: cues are a local draft for review/done; dirty blocks remote
 *   overwrites until the user advances or reloads.
 *
 * Mode is derived only from `transcribeStatus`. Transitions do not need
 * special-case dirty clearing patches — switching to preview simply
 * re-projects from queue JSON.
 */
export function useSubtitleWorkflow({
  queue,
  activeId,
  subtitleTaskId,
  subtitleTaskName,
  subtitleSrtPath,
  subtitleCues,
  subtitleDirty,
  dispatch,
  pushToast,
}: UseSubtitleWorkflowArgs) {
  const { t } = useTranslation(["toasts", "subtitles"]);
  const loadedSubtitleVersionRef = useRef<string>("");
  const persistSeqRef = useRef(0);
  /** Serial queue of IPC saves; always settles so the chain never stalls. */
  const persistChainRef = useRef(Promise.resolve());
  /** Outcome of the most recently enqueued save (rejects if that snapshot fails). */
  const latestPersistRef = useRef(Promise.resolve());
  const loadSeqRef = useRef(0);
  const currentSubtitleTaskIdRef = useRef<string>(subtitleTaskId);
  const activeTaskIdRef = useRef<string>(activeId);
  const existingTaskIdsRef = useRef<Set<string>>(new Set(queue.map((item) => item.id)));
  const queueRef = useRef<QueueItem[]>(queue);
  const subtitleCuesRef = useRef<SubtitleCue[]>(subtitleCues);
  const persistTimerRef = useRef<number | null>(null);
  const pendingPersistRef = useRef<{ taskId: string; cues: SubtitleCue[] } | null>(null);

  useEffect(() => {
    currentSubtitleTaskIdRef.current = subtitleTaskId;
  }, [subtitleTaskId]);

  useEffect(() => {
    activeTaskIdRef.current = activeId;
  }, [activeId]);

  useEffect(() => {
    queueRef.current = queue;
    existingTaskIdsRef.current = new Set(queue.map((item) => item.id));
  }, [queue]);

  useEffect(() => {
    subtitleCuesRef.current = subtitleCues;
  }, [subtitleCues]);

  const canEditTask = useCallback((taskId: string) => {
    return queueRef.current.some((item) => item.id === taskId && isSubtitleEditMode(item.transcribeStatus));
  }, []);

  /**
   * Enqueue a serialized backend save for `cues`. Older jobs still in the
   * queue are skipped when a newer snapshot supersedes them. The returned
   * promise tracks this job: it resolves when the job is skipped or the write
   * succeeds, and rejects only when this still-latest snapshot fails to save.
   */
  const enqueuePersist = useCallback((
    taskId: string,
    cues: SubtitleCue[],
  ): Promise<void> => {
    const seq = ++persistSeqRef.current;
    const job = persistChainRef.current.catch(() => undefined).then(async () => {
      // Superseded while waiting in the serial queue — do not write stale cues.
      if (seq !== persistSeqRef.current) {
        return;
      }
      try {
        await saveSubtitleEditor(taskId, cues);
      } catch (err) {
        // A newer snapshot was enqueued during the IPC call; let that job write.
        if (seq !== persistSeqRef.current) {
          return;
        }
        reportError(err, "saveSubtitleEditor");
        const message = toUserErrorMessage(err, t("toasts:workflow.saveFailed"));
        const soft = /task not found|taskId is required/i.test(message);
        if (
          !soft
          && currentSubtitleTaskIdRef.current === taskId
          && existingTaskIdsRef.current.has(taskId)
        ) {
          pushToast(message, "error");
        }
        throw err;
      }
    });
    // Keep the chain unbroken so one failure does not drop later saves.
    persistChainRef.current = job.then(() => undefined, () => undefined);
    latestPersistRef.current = job;
    return job;
  }, [pushToast, t]);

  const applyCuesFromItem = useCallback(
    async (item: QueueItem, mode: "preview" | "edit") => {
      const seq = ++loadSeqRef.current;
      const taskId = item.id;
      const isStaleRequest = () => (
        seq !== loadSeqRef.current
        || activeTaskIdRef.current !== taskId
        || !existingTaskIdsRef.current.has(taskId)
      );

      if (mode === "preview") {
        // Zero-IPC projection of the live stream snapshot on the queue item.
        const { hydratedCues } = await loadSubtitleEditorData(item);
        if (isStaleRequest()) return;
        dispatch({
          type: "set_subtitle",
          payload: {
            subtitleTaskId: item.id,
            subtitleTaskName: item.name,
            subtitleMediaPath: item.path,
            subtitleSrtPath: "",
            subtitleCues: hydratedCues,
            subtitleCueWarnings: buildCueWarningsById(hydratedCues, []),
            // Preview never owns a draft.
            subtitleDirty: false,
          },
        });
        loadedSubtitleVersionRef.current = buildSubtitleVersion(item);
        return;
      }

      // Edit mode: snapshot authoritative task detail + segments into a draft.
      try {
        const taskDetail = await getTaskRunQueueItem(taskId);
        if (isStaleRequest()) return;

        const enrichedItem: QueueItem = {
          ...item,
          ...taskDetail,
        };
        dispatch({
          type: "set_subtitle",
          payload: {
            subtitleTaskId: enrichedItem.id,
            subtitleTaskName: enrichedItem.name,
            subtitleMediaPath: enrichedItem.path,
          },
        });

        const { response, hydratedCues } = await loadSubtitleEditorData(enrichedItem);
        if (isStaleRequest()) return;

        dispatch({
          type: "set_subtitle",
          payload: {
            subtitleSrtPath: response.srtPath,
            subtitleCues: hydratedCues,
            subtitleCueWarnings: buildCueWarningsById(hydratedCues, response.warnings),
            subtitleDirty: false,
          },
        });
        loadedSubtitleVersionRef.current = buildSubtitleVersion(enrichedItem);
      } catch (error) {
        if (isStaleRequest()) return;
        reportError(error, "loadSubtitleEditor");
        pushToast(t("toasts:workflow.loadInvalid"), "error");
        dispatch({
          type: "set_subtitle",
          payload: {
            subtitleTaskId: taskId,
            subtitleTaskName: item.name,
            subtitleMediaPath: item.path,
            subtitleSrtPath: "",
            subtitleCues: [],
            subtitleCueWarnings: {},
            subtitleDirty: false,
          },
        });
        loadedSubtitleVersionRef.current = buildSubtitleVersion(item);
      }
    },
    [dispatch, pushToast, t],
  );

  // Serializing + persisting is O(n) in cue count, so keystroke-driven edits
  // update local state (and subtitleCuesRef) immediately, then coalesce the
  // queue patch + IPC save into a 400ms trailing debounce.
  //
  // `flushPendingPersist` is a durability barrier: it drains the debounced
  // snapshot, waits for any in-flight IPC (via latestPersistRef), and rejects
  // if the latest save fails. Callers that read backend SoT (export / review)
  // must await it first. Edits that arrive mid-await are re-drained in a loop.
  const restageLiveDraftForRetry = useCallback(() => {
    const taskId = currentSubtitleTaskIdRef.current;
    if (!taskId) return;
    if (!queueRef.current.some((item) => item.id === taskId && isSubtitleEditMode(item.transcribeStatus))) {
      return;
    }
    pendingPersistRef.current = { taskId, cues: subtitleCuesRef.current };
  }, []);

  const awaitLatestPersist = useCallback(async () => {
    const tip = latestPersistRef.current;
    try {
      await tip;
    } catch (err) {
      // Keep the draft eligible for a later barrier and clear the sticky
      // rejection so a future flush can re-enqueue rather than re-hit this tip.
      restageLiveDraftForRetry();
      if (latestPersistRef.current === tip) {
        latestPersistRef.current = Promise.resolve();
      }
      throw err;
    }
  }, [restageLiveDraftForRetry]);

  const drainPendingOnce = useCallback(() => {
    if (persistTimerRef.current != null) {
      window.clearTimeout(persistTimerRef.current);
      persistTimerRef.current = null;
    }
    const pending = pendingPersistRef.current;
    pendingPersistRef.current = null;
    if (!pending || !queueRef.current.some((item) => item.id === pending.taskId)) {
      return;
    }
    const { taskId, cues } = pending;
    const segments = buildSubtitleSegmentsFromCues(cues);
    const resultSrt = subtitleSegmentsToSrt(segments);
    const subtitleSegmentsJson = JSON.stringify(segments);
    dispatch({
      type: "patch_queue_item",
      id: taskId,
      updater: (item) => ({
        ...item,
        resultSrt,
        subtitleSegmentsJson,
      }),
    });
    // Enqueue without awaiting the individual job; barrier waits on tip below.
    void enqueuePersist(taskId, cues);
  }, [dispatch, enqueuePersist]);

  const flushPendingPersist = useCallback(async (): Promise<void> => {
    const MAX_ROUNDS = 32;
    for (let round = 0; round < MAX_ROUNDS; round++) {
      drainPendingOnce();
      // Wait for the most recent job (in-flight or just enqueued), even when
      // there was nothing pending — that covers timer-started IPC still open.
      await awaitLatestPersist();
      if (pendingPersistRef.current == null && persistTimerRef.current == null) {
        return;
      }
    }
    // Continuous typing during flush is pathological; force one last drain.
    drainPendingOnce();
    await awaitLatestPersist();
  }, [awaitLatestPersist, drainPendingOnce]);

  const flushPendingPersistRef = useRef(flushPendingPersist);
  useEffect(() => {
    flushPendingPersistRef.current = flushPendingPersist;
  }, [flushPendingPersist]);

  // A pending trailing save always carries its own taskId, so flushing on
  // task switch or unmount writes to the correct task and loses no edits.
  useEffect(() => {
    return () => {
      void flushPendingPersistRef.current().catch(() => undefined);
    };
  }, [subtitleTaskId]);

  const markSubtitleEdited = useCallback(
    (nextCues: SubtitleCue[]) => {
      const taskId = currentSubtitleTaskIdRef.current;
      // Edits only exist in edit mode; preview ignores mutations.
      if (!taskId || !canEditTask(taskId)) {
        return;
      }
      // Keep the ref in lockstep with the draft so consecutive mutations in the
      // same turn (before the subtitleCues effect runs) compose correctly.
      subtitleCuesRef.current = nextCues;
      dispatch({
        type: "set_subtitle",
        payload: {
          subtitleCues: nextCues,
          subtitleCueWarnings: {},
          subtitleDirty: true,
        },
      });
      const taskStillExists = queueRef.current.some((item) => item.id === taskId);
      if (!taskStillExists) {
        return;
      }
      pendingPersistRef.current = { taskId, cues: nextCues };
      if (persistTimerRef.current == null) {
        persistTimerRef.current = window.setTimeout(() => {
          persistTimerRef.current = null;
          void flushPendingPersist().catch(() => undefined);
        }, 400);
      }
    },
    [canEditTask, dispatch, flushPendingPersist],
  );

  /** Live draft JSON for review advance: flush barrier first, then read ref. */
  const prepareReviewFlushJson = useCallback(async (taskId: string): Promise<string | undefined> => {
    if (taskId !== currentSubtitleTaskIdRef.current) {
      return undefined;
    }
    await flushPendingPersist();
    const cues = subtitleCuesRef.current;
    if (cues.length === 0) {
      return undefined;
    }
    return JSON.stringify(buildSubtitleSegmentsFromCues(cues));
  }, [flushPendingPersist]);

  const updateCue = useCallback(
    (cueId: string, patchCue: Partial<SubtitleCue>) => {
      markSubtitleEdited(updateCueList(subtitleCuesRef.current, cueId, patchCue));
    },
    [markSubtitleEdited],
  );

  const addCueAfter = useCallback(
    (selectedCueId: string | null) => {
      markSubtitleEdited(addCueAfterSelection(subtitleCuesRef.current, selectedCueId));
    },
    [markSubtitleEdited],
  );

  const mergeSelectedCues = useCallback(
    (selectedCueIds: string[]) => {
      const next = mergeSelectedCueList(subtitleCuesRef.current, selectedCueIds);
      if (next !== subtitleCuesRef.current) {
        markSubtitleEdited(next);
      }
    },
    [markSubtitleEdited],
  );

  const splitSelectedCues = useCallback(
    (selectedCueIds: string[]): Array<{ sourceCueId: string; bornCueId: string }> => {
      const { nextCues, bornCueIds } = splitSelectedCueList(subtitleCuesRef.current, selectedCueIds);
      if (bornCueIds.length > 0) {
        markSubtitleEdited(nextCues);
      }
      return bornCueIds;
    },
    [markSubtitleEdited],
  );

  const replaceTextInCues = useCallback(
    (findText: string, replaceText: string, scopeCueIds: string[] | null, maxReplacements?: number): number => {
      const { nextCues, replacedCount } = replaceTextInCueList(
        subtitleCuesRef.current,
        findText,
        replaceText,
        scopeCueIds,
        maxReplacements,
      );
      if (replacedCount > 0) {
        markSubtitleEdited(nextCues);
      }
      return replacedCount;
    },
    [markSubtitleEdited],
  );

  const removeCue = useCallback(
    (cueId: string) => {
      markSubtitleEdited(removeCueFromList(subtitleCuesRef.current, cueId));
    },
    [markSubtitleEdited],
  );

  const exportSubtitleSrt = useCallback(async (items: ExportSrtItem[]) => {
    if (!subtitleTaskId) {
      pushToast(t("toasts:workflow.noExportTask"), "error");
      return;
    }
    if (items.length === 0) {
      pushToast(t("toasts:workflow.selectAtLeastOne"), "error");
      return;
    }

    // Durability barrier: save failures are already toasted by enqueuePersist.
    try {
      await flushPendingPersist();
    } catch {
      return;
    }

    try {
      const picked = await open({
        directory: true,
        multiple: false,
        title: t("subtitles:export.dirPickerTitle"),
      });
      if (!picked || Array.isArray(picked)) return;

      // Re-flush in case the user kept typing while the dir picker was open.
      try {
        await flushPendingPersist();
      } catch {
        return;
      }

      const paths = await exportSubtitleVariantsToDirectory(
        subtitleTaskId,
        picked,
        subtitleTaskName,
        items,
      );
      if (paths.length === 1) {
        pushToast(t("toasts:workflow.exportedOne", { path: paths[0] }), "success");
      } else {
        pushToast(t("toasts:workflow.exportedMany", { count: paths.length }), "success");
      }
    } catch (error) {
      reportError(error, "exportSubtitleSrt");
      pushToast(toUserErrorMessage(error, t("toasts:workflow.exportFailed")), "error");
    }
  }, [flushPendingPersist, pushToast, subtitleTaskId, subtitleTaskName, t]);

  // Bind editor to active task according to mode (preview vs edit).
  useEffect(() => {
    const activeItem = queue.find((item) => item.id === activeId);
    if (!activeItem) {
      loadSeqRef.current += 1;
      dispatch({
        type: "set_subtitle",
        payload: {
          subtitleTaskId: "",
          subtitleTaskName: "",
          subtitleMediaPath: "",
          subtitleSrtPath: "",
          subtitleCues: [],
          subtitleCueWarnings: {},
          subtitleDirty: false,
        },
      });
      loadedSubtitleVersionRef.current = "";
      return;
    }

    const mode = resolveSubtitleEditorMode(activeItem.transcribeStatus);

    if (subtitleTaskId !== activeItem.id) {
      void applyCuesFromItem(activeItem, mode);
      return;
    }

    // Edit mode: protect local draft while dirty.
    if (mode === "edit" && subtitleDirty) {
      return;
    }

    // Preview mode: always follow stream version ticks (dirty is irrelevant).
    // Edit mode (clean): refresh when backend version changes (e.g. re-open).
    const currentVersion = buildSubtitleVersion(activeItem);
    if (loadedSubtitleVersionRef.current === currentVersion) {
      return;
    }
    void applyCuesFromItem(activeItem, mode);
  }, [activeId, applyCuesFromItem, dispatch, queue, subtitleDirty, subtitleTaskId]);

  const activeItem = queue.find((item) => item.id === activeId) ?? null;
  const canEditSubtitle = activeItem != null && isSubtitleEditMode(activeItem.transcribeStatus);

  return {
    activeItem,
    canEditSubtitle,
    subtitleTaskName,
    subtitleSrtPath,
    subtitleCues,
    updateCue,
    addCueAfter,
    mergeSelectedCues,
    splitSelectedCues,
    replaceTextInCues,
    removeCue,
    exportSubtitleSrt,
    /** Durability barrier for backend SoT (export / review). Rejects on save failure. */
    flushPendingPersist,
    /** Flush barrier then return live draft JSON from subtitleCuesRef. */
    prepareReviewFlushJson,
  };
}
