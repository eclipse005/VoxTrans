import { useCallback, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWindow, type DragDropEvent } from "@tauri-apps/api/window";
import { getFileSize } from "../../api/transcribe";
import { registerTaskUpload } from "../../api/workspace";
import {
  createEmptyTaskProgress,
  type LanguageTag,
  type QueueItem,
} from "../../../features/media/types";
import {
  DEFAULT_SOURCE_LANGUAGE,
  DEFAULT_TARGET_LANGUAGE,
} from "../../../features/media/languages";
import { detectMediaKind, fileName, isSupportedUploadPath } from "../../../features/media/utils";
import type { AppAction } from "../../state/appReducer";
import { addQueueItems } from "../../state/queueDomainActions";
import { reportError, toUserErrorMessage } from "../../utils/errors";

type DispatchState = (action: AppAction) => void;
type PushToast = (message: string, tone?: "info" | "success" | "error") => void;

type UseQueueInputArgs = {
  dispatch: DispatchState;
  pushToast: PushToast;
  activeTerminologyGroupId: string;
};

export function useQueueInput({ dispatch, pushToast, activeTerminologyGroupId }: UseQueueInputArgs) {
  const { t } = useTranslation(["toasts", "tasks"]);
  const appendPaths = useCallback(async (paths: string[]) => {
    if (!paths.length) return;

    const supported = paths.filter((path) => isSupportedUploadPath(path));
    const skipped = paths.length - supported.length;
    if (skipped > 0) {
      pushToast(t("toasts:queue.unsupportedFilesSkipped", { count: skipped }), "info");
    }
    if (!supported.length) return;

    const incoming = await Promise.all(
      supported.map(async (path) => {
        let sizeBytes = 0;
        try {
          sizeBytes = await getFileSize(path);
        } catch {
          sizeBytes = 0;
        }

        const mediaKind = detectMediaKind(path);
        return {
          id: `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
          path,
          name: fileName(path),
          mediaKind,
          sizeBytes,
          sourceLang: DEFAULT_SOURCE_LANGUAGE satisfies LanguageTag,
          targetLang: DEFAULT_TARGET_LANGUAGE,
          transcribeStatus: "pending",
          taskProgress: createEmptyTaskProgress(),
          transcribeError: "",
          resultText: "",
          resultSrt: "",
          subtitleSegmentsJson: "[]",
          terminologyGroupId: activeTerminologyGroupId,
        } satisfies QueueItem;
      }),
    );

    const persisted: QueueItem[] = [];
    let failedCount = 0;
    for (const item of incoming) {
      try {
        const registered = await registerTaskUpload({
          id: item.id,
          mediaPath: item.path,
          name: item.name,
          mediaKind: item.mediaKind,
          sizeBytes: item.sizeBytes,
        });
        // Backend may rewrite path (SRT → task dir/source.srt) and fill segments.
        persisted.push({
          ...item,
          ...registered,
          id: registered.id || item.id,
          terminologyGroupId:
            registered.terminologyGroupId || item.terminologyGroupId || activeTerminologyGroupId,
          sourceLang: registered.sourceLang || item.sourceLang,
          targetLang: registered.targetLang || item.targetLang,
        });
      } catch (error) {
        failedCount += 1;
        reportError(error, "registerTaskUpload");
      }
    }

    if (persisted.length > 0) {
      addQueueItems(dispatch, persisted);
      pushToast(t("toasts:queue.addedCount", { count: persisted.length }), "success");
    }
    if (failedCount > 0) {
      pushToast(t("toasts:queue.addFailedCount", { count: failedCount }), "error");
    }
  }, [dispatch, pushToast, activeTerminologyGroupId, t]);

  useEffect(() => {
    let disposed = false;
    let unlisten: undefined | (() => void);
    let scaleFactor = 1;

    void getCurrentWindow()
      .scaleFactor()
      .then((value) => {
        if (!disposed && Number.isFinite(value) && value > 0) {
          scaleFactor = value;
        }
      })
      .catch(() => {});

    const isInsideUploadArea = (position: { x: number; y: number }) => {
      const zone = document.querySelector(".upload-panel-content.active .upload-area");
      if (!(zone instanceof HTMLElement)) return false;
      const rect = zone.getBoundingClientRect();
      if (rect.width <= 0 || rect.height <= 0) return false;

      const logicalX = position.x / scaleFactor;
      const logicalY = position.y / scaleFactor;
      const insideLogical = logicalX >= rect.left && logicalX <= rect.right && logicalY >= rect.top && logicalY <= rect.bottom;
      if (insideLogical) return true;

      return position.x >= rect.left && position.x <= rect.right && position.y >= rect.top && position.y <= rect.bottom;
    };

    getCurrentWindow()
      .onDragDropEvent((event: { payload: DragDropEvent }) => {
        const payload = event.payload;
        if (!payload) return;

        if (payload.type === "enter" || payload.type === "over") {
          const inside = isInsideUploadArea(payload.position);
          dispatch({ type: "set_ui", payload: { dragActive: inside } });
        } else if (payload.type === "leave") {
          dispatch({ type: "set_ui", payload: { dragActive: false } });
        } else if (payload.type === "drop") {
          dispatch({ type: "set_ui", payload: { dragActive: false } });
          if (!isInsideUploadArea(payload.position)) return;
          const paths = Array.isArray(payload.paths) ? payload.paths : [];
          void appendPaths(paths);
        }
      })
      .then((fn) => {
        if (disposed) {
          fn();
          return;
        }
        unlisten = fn;
      })
      .catch(() => {});

    return () => {
      disposed = true;
      if (unlisten) unlisten();
    };
  }, [appendPaths, dispatch]);

  const pickFiles = useCallback(async () => {
    try {
      const picked = await open({
        multiple: true,
        directory: false,
        filters: [
          {
            name: "Media & Subtitles",
            extensions: [
              "mp3", "wav", "m4a", "flac", "aac", "ogg", "opus",
              "mp4", "mkv", "mov", "avi", "webm", "m4v",
              "srt",
            ],
          },
        ],
      });

      if (!picked) return;
      const paths = Array.isArray(picked) ? picked : [picked];
      await appendPaths(paths);
    } catch (error) {
      reportError(error, "pickFiles");
      pushToast(toUserErrorMessage(error, "toasts.queue.pickFilesFailed"), "error");
    }
  }, [appendPaths, pushToast]);

  return {
    appendPaths,
    pickFiles,
  };
}
