import { describe, expect, it } from "vitest";

import {
  deleteRemoteBeforeLocalMutation,
  deleteRemoteWithLocalPreparation,
} from "./queueDeleteCommit";

describe("deleteRemoteBeforeLocalMutation", () => {
  it("does not mutate local queue when remote delete fails", async () => {
    let mutated = false;

    await expect(
      deleteRemoteBeforeLocalMutation(
        async () => {
          throw new Error("delete failed");
        },
        () => {
          mutated = true;
        },
      ),
    ).rejects.toThrow("delete failed");

    expect(mutated).toBe(false);
  });

  it("mutates local queue after remote delete succeeds", async () => {
    let mutated = false;

    await deleteRemoteBeforeLocalMutation(
      async () => {},
      () => {
        mutated = true;
      },
    );

    expect(mutated).toBe(true);
  });

  it("rolls back prepared local state when remote delete fails", async () => {
    const prepared = new Set<string>();
    const committed = new Set<string>();

    await expect(
      deleteRemoteWithLocalPreparation({
        prepareLocal: () => {
          prepared.add("task-1");
          return "task-1";
        },
        deleteRemote: async () => {
          throw new Error("delete failed");
        },
        commitLocal: (taskId) => {
          committed.add(taskId);
        },
        rollbackLocal: (taskId) => {
          prepared.delete(taskId);
        },
      }),
    ).rejects.toThrow("delete failed");

    expect(prepared.has("task-1")).toBe(false);
    expect(committed.has("task-1")).toBe(false);
  });
});
