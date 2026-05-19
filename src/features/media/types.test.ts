import { describe, expect, it } from "vitest";
import {
  createEmptyTaskProgress,
  createTaskProgress,
  normalizeTaskProgress,
  normalizeTaskStageCode,
} from "./types";

describe("normalizeTaskStageCode", () => {
  it("returns valid stage codes", () => {
    expect(normalizeTaskStageCode("preparing")).toBe("preparing");
    expect(normalizeTaskStageCode("recognizing")).toBe("recognizing");
    expect(normalizeTaskStageCode("translating")).toBe("translating");
    expect(normalizeTaskStageCode("subtitleLayout")).toBe("subtitleLayout");
  });

  it("returns empty string for unknown codes", () => {
    expect(normalizeTaskStageCode("unknown")).toBe("");
    expect(normalizeTaskStageCode("")).toBe("");
  });

  it("returns empty string for non-string values", () => {
    expect(normalizeTaskStageCode(null)).toBe("");
    expect(normalizeTaskStageCode(123)).toBe("");
    expect(normalizeTaskStageCode(undefined)).toBe("");
  });
});

describe("createEmptyTaskProgress", () => {
  it("returns a progress with all zeros and empty strings", () => {
    const progress = createEmptyTaskProgress();
    expect(progress.stage.code).toBe("");
    expect(progress.stage.label).toBe("");
    expect(progress.stage.order).toBe(0);
    expect(progress.stage.detail).toBe("");
    expect(progress.stage.current).toBe(0);
    expect(progress.stage.total).toBe(0);
  });
});

describe("createTaskProgress", () => {
  it("assigns order from TASK_STAGE_ORDER when code is known", () => {
    const progress = createTaskProgress({ code: "recognizing" });
    expect(progress.stage.code).toBe("recognizing");
    expect(progress.stage.order).toBe(30);
  });

  it("uses provided order when code is empty", () => {
    const progress = createTaskProgress({ order: 42 });
    expect(progress.stage.order).toBe(42);
  });

  it("prefers explicit order over TASK_STAGE_ORDER", () => {
    const progress = createTaskProgress({ code: "preparing", order: 99 });
    expect(progress.stage.order).toBe(99);
  });

  it("normalizes string order to number", () => {
    const progress = createTaskProgress({ order: "25" as unknown as number });
    expect(progress.stage.order).toBe(25);
  });

  it("clamps negative values to 0", () => {
    const progress = createTaskProgress({ current: -5, total: -10 });
    expect(progress.stage.current).toBe(0);
    expect(progress.stage.total).toBe(0);
  });
});

describe("normalizeTaskProgress", () => {
  it("returns empty progress for non-object input", () => {
    expect(normalizeTaskProgress(null)).toEqual(createEmptyTaskProgress());
    expect(normalizeTaskProgress("string")).toEqual(createEmptyTaskProgress());
  });

  it("normalizes a partial progress object", () => {
    const result = normalizeTaskProgress({
      stage: {
        code: "translating",
        label: "翻译中",
        current: 3,
        total: 10,
      },
    });
    expect(result.stage.code).toBe("translating");
    expect(result.stage.label).toBe("翻译中");
    expect(result.stage.order).toBe(70);
    expect(result.stage.current).toBe(3);
    expect(result.stage.total).toBe(10);
  });

  it("rejects invalid stage codes", () => {
    const result = normalizeTaskProgress({
      stage: { code: "invalid", order: 50 },
    });
    expect(result.stage.code).toBe("");
    expect(result.stage.order).toBe(50);
  });
});
