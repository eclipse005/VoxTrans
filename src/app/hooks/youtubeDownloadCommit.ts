import type { RegisterTaskUploadRequest } from "../api/workspace";
import type { DownloadYoutubeTaskResponse } from "../api/youtube";
import {
  DEFAULT_SOURCE_LANGUAGE,
  DEFAULT_TARGET_LANGUAGE,
} from "../../features/media/languages";
import { createEmptyTaskProgress, type QueueItem } from "../../features/media/types";

type DeleteRegisteredTaskRequest = {
  taskId: string;
  mediaPath: string;
};

type DownloadedYoutubeQueueItemLanguages = Pick<
  QueueItem,
  "sourceLang" | "targetLang"
>;

type CommitDownloadedYoutubeTaskArgs = {
  placeholderTaskId: string;
  response: DownloadYoutubeTaskResponse;
  isRemoved: (taskId: string) => boolean;
  registerTask: (request: RegisterTaskUploadRequest) => Promise<unknown>;
  deleteRegisteredTask: (request: DeleteRegisteredTaskRequest) => Promise<void>;
  commitLocal: (response: DownloadYoutubeTaskResponse) => void;
};

type CommitDownloadedYoutubeTaskResult =
  | { status: "committed" }
  | {
      status: "deferred";
      placeholderTaskId: string;
      response: DownloadYoutubeTaskResponse;
    }
  | {
      status: "commitFailed";
      error: unknown;
      placeholderTaskId: string;
      response: DownloadYoutubeTaskResponse;
    }
  | {
      status: "compensationFailed";
      error: unknown;
      registeredTaskId: string;
      registeredMediaPath: string;
    };

type RestoreDeferredYoutubeCompletionArgs = {
  taskId: string;
  deferredCompletions: Map<string, DownloadYoutubeTaskResponse>;
  restore: (
    response: DownloadYoutubeTaskResponse,
  ) => Promise<CommitDownloadedYoutubeTaskResult>;
};

type RestoreDeferredYoutubeCompletionResult =
  | { status: "missing" }
  | { status: "restored" }
  | { status: "restoreFailed"; error: unknown }
  | Extract<
      CommitDownloadedYoutubeTaskResult,
      { status: "commitFailed" | "compensationFailed" }
    >
  | Extract<CommitDownloadedYoutubeTaskResult, { status: "deferred" }>;

export async function commitDownloadedYoutubeTask({
  placeholderTaskId,
  response,
  isRemoved,
  registerTask,
  deleteRegisteredTask,
  commitLocal,
}: CommitDownloadedYoutubeTaskArgs): Promise<CommitDownloadedYoutubeTaskResult> {
  if (isRemoved(placeholderTaskId)) {
    return {
      status: "deferred",
      placeholderTaskId,
      response,
    };
  }

  try {
    await registerTask({
      id: response.task.id,
      mediaPath: response.task.mediaPath,
      name: response.task.name,
      mediaKind: response.task.mediaKind,
      sizeBytes: response.task.sizeBytes,
    });
  } catch (error) {
    return {
      status: "commitFailed",
      error,
      placeholderTaskId,
      response,
    };
  }

  if (isRemoved(placeholderTaskId)) {
    try {
      await deleteRegisteredTask({
        taskId: response.task.id,
        mediaPath: response.task.mediaPath,
      });
    } catch (error) {
      return {
        status: "compensationFailed",
        error,
        registeredTaskId: response.task.id,
        registeredMediaPath: response.task.mediaPath,
      };
    }
    return {
      status: "deferred",
      placeholderTaskId,
      response,
    };
  }

  commitLocal(response);
  return { status: "committed" };
}

export async function restoreDeferredYoutubeCompletion({
  taskId,
  deferredCompletions,
  restore,
}: RestoreDeferredYoutubeCompletionArgs): Promise<RestoreDeferredYoutubeCompletionResult> {
  const response = deferredCompletions.get(taskId);
  if (!response) return { status: "missing" };

  try {
    const result = await restore(response);
    if (result.status !== "committed") return result;
    deferredCompletions.delete(taskId);
    return { status: "restored" };
  } catch (error) {
    return { status: "restoreFailed", error };
  }
}

export function createDownloadedYoutubeQueueItem(
  response: DownloadYoutubeTaskResponse,
  languages: DownloadedYoutubeQueueItemLanguages = {
    sourceLang: DEFAULT_SOURCE_LANGUAGE,
    targetLang: DEFAULT_TARGET_LANGUAGE,
  },
): QueueItem {
  return {
    id: response.task.id,
    path: response.task.mediaPath,
    name: response.task.name,
    mediaKind: response.task.mediaKind,
    sizeBytes: response.task.sizeBytes,
    sourceLang: languages.sourceLang,
    targetLang: languages.targetLang,
    transcribeStatus: "pending",
    taskProgress: createEmptyTaskProgress(),
    transcribeError: "",
    resultText: "",
    resultSrt: "",
    subtitleSegmentsJson: "[]",
  };
}
