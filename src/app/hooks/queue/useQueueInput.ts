import { useCallback, useEffect } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWindow, type DragDropEvent } from "@tauri-apps/api/window";
import { getFileSize } from "../../api/transcribe";
import type { QueueItem } from "../../../features/media/types";
import { detectMediaKind, fileName } from "../../../features/media/utils";
import type { AppAction } from "../../state/appReducer";
import { addQueueItems } from "../../state/queueDomainActions";
import { reportError, toUserErrorMessage } from "../../utils/errors";

type DispatchState = (action: AppAction) => void;
type PushToast = (message: string, tone?: "info" | "success" | "error") => void;

type UseQueueInputArgs = {
  dispatch: DispatchState;
  pushToast: PushToast;
};

export function useQueueInput({ dispatch, pushToast }: UseQueueInputArgs) {
  const appendPaths = useCallback(async (paths: string[]) => {
    if (!paths.length) return;

    const incoming = await Promise.all(
      paths.map(async (path) => {
        let sizeBytes = 0;
        try {
          sizeBytes = await getFileSize(path);
        } catch {
          sizeBytes = 0;
        }

        return {
          id: `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
          path,
          name: fileName(path),
          mediaKind: detectMediaKind(path),
          sizeBytes,
          transcribeStatus: "pending",
          transcribeProgress: 0,
          transcribeSegmentCurrent: 0,
          transcribeSegmentTotal: 0,
          transcribePhase: "",
          transcribeError: "",
          resultText: "",
          resultSrt: "",
          subtitleSegmentsJson: "[]",
        } satisfies QueueItem;
      }),
    );

    addQueueItems(dispatch, incoming);
    pushToast(`已加入队列 ${paths.length} 个文件`, "success");
  }, [dispatch, pushToast]);

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
  }, [appendPaths, pushToast]);

  return {
    appendPaths,
    pickFiles,
  };
}

