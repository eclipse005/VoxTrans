import { describe, expect, it } from "vitest";
import {
  mergeTaskStateChanged,
  shouldKeepCurrentProcessingStage,
  stageOrder,
  stageRatio,
  toEnqueuePayload,
  type QueueRunMode,
} from "./queueUtils";
import { createEmptyTaskProgress, createTaskProgress } from "./types";

describe("stageOrder", () => {
  it("extracts order from stage", () => {
    expect(stageOrder({ order: 3, current: 1, total: 5 })).toBe(3);
  });

  it("returns 0 for null/undefined", () => {
    expect(stageOrder(null)).toBe(0);
    expect(stageOrder(undefined)).toBe(0);
  });

  it("returns 0 for non-finite", () => {
    expect(stageOrder({ order: NaN })).toBe(0);
  });
});

describe("stageRatio", () => {
  it("computes ratio correctly", () => {
    expect(stageRatio({ current: 2, total: 4 })).toBe(0.5);
  });

  it("returns 0 for zero total", () => {
    expect(stageRatio({ current: 1, total: 0 })).toBe(0);
  });

  it("clamps to 0-1 range", () => {
    expect(stageRatio({ current: 10, total: 4 })).toBe(1);
    expect(stageRatio({ current: -1, total: 4 })).toBe(0);
  });
});

describe("shouldKeepCurrentProcessingStage", () => {
  const baseItem = {
    id: "1",
    path: "/x",
    name: "Test",
    mediaKind: "video" as const,
    sizeBytes: 0,
    sourceLang: "zh" as const,
    targetLang: "en" as const,
    transcribeStatus: "processing" as const,
    taskProgress: createTaskProgress({ code: "recognizing", current: 80, total: 100 }),
    transcribeError: "",
    resultText: "",
    resultSrt: "",
    subtitleSegmentsJson: "[]",
  };

  const basePayload = {
    id: "1",
    path: "/x",
    name: "Test",
    mediaKind: "video",
    sizeBytes: 0,
    transcribeStatus: "processing",
    taskProgress: createTaskProgress({ code: "recognizing", current: 10, total: 100 }),
    transcribeError: "",
    resultText: "",
    resultSrt: "",
    subtitleSegmentsJson: "[]",
  };

  it("accepts incoming same-stage event (backend is source of truth)", () => {
    expect(shouldKeepCurrentProcessingStage(baseItem, basePayload)).toBe(false);
  });

  it("does not keep when not processing", () => {
    const notProcessing = { ...baseItem, transcribeStatus: "pending" as const };
    expect(shouldKeepCurrentProcessingStage(notProcessing, basePayload)).toBe(false);
  });

  it("does not keep when incoming is ahead", () => {
    const aheadPayload = {
      ...basePayload,
      taskProgress: createTaskProgress({ code: "segmenting", current: 1, total: 100 }),
    };
    expect(shouldKeepCurrentProcessingStage(baseItem, aheadPayload)).toBe(false);
  });
});

describe("mergeTaskStateChanged", () => {
  it("merges payload into current item", () => {
    const current = {
      id: "1",
      path: "/old",
      name: "Old",
      mediaKind: "video" as const,
      sizeBytes: 0,
      sourceLang: "zh" as const,
      targetLang: "en" as const,
      transcribeStatus: "pending" as const,
      taskProgress: createEmptyTaskProgress(),
      transcribeError: "",
      resultText: "",
      resultSrt: "",
      subtitleSegmentsJson: "[]",
      reviewSource: true,
      reviewTarget: true,
    };

    const payload = {
      id: "1",
      path: "/new",
      name: "New",
      mediaKind: "audio",
      sizeBytes: 1024,
      sourceLang: "en",
      targetLang: "ja",
      transcribeStatus: "review_source",
      taskProgress: createTaskProgress({ code: "translating", current: 0, total: 0 }),
      transcribeError: "",
      resultText: "hello",
      resultSrt: "1\n00:00:00,000 --> 00:00:01,000\nhello",
      subtitleSegmentsJson: "[]",
      reviewSource: true,
      reviewTarget: true,
    };

    const merged = mergeTaskStateChanged(current, payload);
    expect(merged.path).toBe("/new");
    expect(merged.name).toBe("New");
    expect(merged.mediaKind).toBe("audio");
    expect(merged.sizeBytes).toBe(1024);
    expect(merged.sourceLang).toBe("en");
    expect(merged.targetLang).toBe("ja");
    expect(merged.transcribeStatus).toBe("review_source");
    expect(merged.resultText).toBe("hello");
    expect(merged.reviewSource).toBe(true);
    expect(merged.reviewTarget).toBe(true);
  });

  it("keeps review flags when payload omits them", () => {
    const current = {
      id: "1",
      path: "/a",
      name: "A",
      mediaKind: "video" as const,
      sizeBytes: 1,
      sourceLang: "en" as const,
      targetLang: "zh-CN" as const,
      transcribeStatus: "processing" as const,
      taskProgress: createEmptyTaskProgress(),
      transcribeError: "",
      resultText: "",
      resultSrt: "",
      subtitleSegmentsJson: "[]",
      reviewSource: true,
      reviewTarget: false,
    };
    const payload = {
      id: "1",
      path: "/a",
      name: "A",
      mediaKind: "video",
      sizeBytes: 1,
      sourceLang: "en",
      targetLang: "zh-CN",
      transcribeStatus: "review_source",
      taskProgress: createEmptyTaskProgress(),
      transcribeError: "",
      resultText: "x",
      resultSrt: "",
      subtitleSegmentsJson: "[]",
    };
    const merged = mergeTaskStateChanged(current, payload);
    expect(merged.reviewSource).toBe(true);
    expect(merged.reviewTarget).toBe(false);
  });
});

describe("toEnqueuePayload", () => {
  it("maps item fields correctly", () => {
    const item = {
      id: "1",
      path: "/x.mp4",
      name: "Test",
      mediaKind: "video" as const,
      sizeBytes: 1024,
      sourceLang: "en" as const,
      targetLang: "zh-CN" as const,
      transcribeStatus: "pending" as const,
      taskProgress: createEmptyTaskProgress(),
      transcribeError: "",
      resultText: "",
      resultSrt: "",
      subtitleSegmentsJson: "[]",
    };

    const payload = toEnqueuePayload(item, "transcribe");
    expect(payload.id).toBe("1");
    expect(payload.mediaPath).toBe("/x.mp4");
    expect(payload.intent).toBe("TRANSCRIBE");
    expect(payload.sourceLang).toBe("en");
    expect(payload.targetLang).toBe("zh-CN");
    expect(payload.maxRetries).toBe(0);
  });

  it("maps transcribe_translate mode", () => {
    const item = {
      id: "1",
      path: "/x.mp4",
      name: "Test",
      mediaKind: "video" as const,
      sizeBytes: 0,
      sourceLang: "zh" as const,
      targetLang: "en" as const,
      transcribeStatus: "pending" as const,
      taskProgress: createEmptyTaskProgress(),
      transcribeError: "",
      resultText: "",
      resultSrt: "",
      subtitleSegmentsJson: "[]",
    };

    const payload = toEnqueuePayload(item, "transcribe_translate" as QueueRunMode);
    expect(payload.intent).toBe("TRANSCRIBE_TRANSLATE");
  });
});
