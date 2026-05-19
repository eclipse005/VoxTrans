import { describe, expect, it } from "vitest";

import {
  commitDownloadedYoutubeTask,
  createDownloadedYoutubeQueueItem,
  restoreDeferredYoutubeCompletion,
} from "./youtubeDownloadCommit";
import type { DownloadYoutubeTaskResponse } from "../api/youtube";

describe("commitDownloadedYoutubeTask", () => {
  it("skips registration when the placeholder was removed before commit", async () => {
    let registered = false;
    let committed = false;

    const result = await commitDownloadedYoutubeTask({
      placeholderTaskId: "yt-1",
      response: testResponse(),
      isRemoved: () => true,
      registerTask: async () => {
        registered = true;
      },
      deleteRegisteredTask: async () => {},
      commitLocal: () => {
        committed = true;
      },
    });

    expect(result).toEqual({
      status: "deferred",
      placeholderTaskId: "yt-1",
      response: testResponse(),
    });
    expect(registered).toBe(false);
    expect(committed).toBe(false);
  });

  it("compensates the registered task when the placeholder is removed during registration", async () => {
    let removed = false;
    const deleted: Array<{ taskId: string; mediaPath: string }> = [];
    let committed = false;

    const result = await commitDownloadedYoutubeTask({
      placeholderTaskId: "yt-1",
      response: testResponse(),
      isRemoved: () => removed,
      registerTask: async () => {
        removed = true;
      },
      deleteRegisteredTask: async (request) => {
        deleted.push(request);
      },
      commitLocal: () => {
        committed = true;
      },
    });

    expect(result).toEqual({
      status: "deferred",
      placeholderTaskId: "yt-1",
      response: testResponse(),
    });
    expect(deleted).toEqual([
      {
        taskId: "task-1",
        mediaPath: "D:\\media\\downloaded.mp4",
      },
    ]);
    expect(committed).toBe(false);
  });

  it("reports compensation failure without committing local state", async () => {
    let removed = false;
    let committed = false;
    const deleteError = new Error("delete failed");

    const result = await commitDownloadedYoutubeTask({
      placeholderTaskId: "yt-1",
      response: testResponse(),
      isRemoved: () => removed,
      registerTask: async () => {
        removed = true;
      },
      deleteRegisteredTask: async () => {
        throw deleteError;
      },
      commitLocal: () => {
        committed = true;
      },
    });

    expect(result).toEqual({
      status: "compensationFailed",
      error: deleteError,
      registeredTaskId: "task-1",
      registeredMediaPath: "D:\\media\\downloaded.mp4",
    });
    expect(committed).toBe(false);
  });

  it("returns commit failure with response when registration fails", async () => {
    let committed = false;
    const response = testResponse();
    const registerError = new Error("register failed");

    const result = await commitDownloadedYoutubeTask({
      placeholderTaskId: "yt-1",
      response,
      isRemoved: () => false,
      registerTask: async () => {
        throw registerError;
      },
      deleteRegisteredTask: async () => {},
      commitLocal: () => {
        committed = true;
      },
    });

    expect(result).toEqual({
      status: "commitFailed",
      error: registerError,
      placeholderTaskId: "yt-1",
      response,
    });
    expect(committed).toBe(false);
  });

  it("keeps deferred completion when rollback restore fails", async () => {
    const response = testResponse();
    const deferredCompletions = new Map([["yt-1", response]]);
    const restoreError = new Error("register failed");

    const result = await restoreDeferredYoutubeCompletion({
      taskId: "yt-1",
      deferredCompletions,
      restore: async () => {
        throw restoreError;
      },
    });

    expect(result).toEqual({
      status: "restoreFailed",
      error: restoreError,
    });
    expect(deferredCompletions.get("yt-1")).toBe(response);
  });

  it("keeps deferred completion when rollback restore returns commit failure", async () => {
    const response = testResponse();
    const deferredCompletions = new Map([["yt-1", response]]);
    const restoreError = new Error("register failed");

    const result = await restoreDeferredYoutubeCompletion({
      taskId: "yt-1",
      deferredCompletions,
      restore: async () => ({
        status: "commitFailed",
        error: restoreError,
        placeholderTaskId: "yt-1",
        response,
      }),
    });

    expect(result).toEqual({
      status: "commitFailed",
      error: restoreError,
      placeholderTaskId: "yt-1",
      response,
    });
    expect(deferredCompletions.get("yt-1")).toBe(response);
  });

  it("consumes deferred completion after rollback restore succeeds", async () => {
    const response = testResponse();
    const deferredCompletions = new Map([["yt-1", response]]);

    const result = await restoreDeferredYoutubeCompletion({
      taskId: "yt-1",
      deferredCompletions,
      restore: async (deferredResponse) => {
        expect(deferredResponse).toBe(response);
        return { status: "committed" };
      },
    });

    expect(result).toEqual({ status: "restored" });
    expect(deferredCompletions.has("yt-1")).toBe(false);
  });

  it("builds downloaded queue item with placeholder languages", () => {
    const item = createDownloadedYoutubeQueueItem(testResponse(), {
      sourceLang: "ja",
      targetLang: "en",
    });

    expect(item).toMatchObject({
      id: "task-1",
      path: "D:\\media\\downloaded.mp4",
      sourceLang: "ja",
      targetLang: "en",
      transcribeStatus: "pending",
      transcribeError: "",
    });
  });

  it("builds downloaded queue item with default languages when none are provided", () => {
    const item = createDownloadedYoutubeQueueItem(testResponse());

    expect(item.sourceLang).toBe("en");
    expect(item.targetLang).toBe("zh-CN");
  });
});

function testResponse(): DownloadYoutubeTaskResponse {
  return {
    task: {
      id: "task-1",
      mediaPath: "D:\\media\\downloaded.mp4",
      name: "downloaded.mp4",
      mediaKind: "video",
      sizeBytes: 123,
    },
    outputDir: "D:\\media",
    downloadedPath: "D:\\media\\downloaded.mp4",
  };
}
