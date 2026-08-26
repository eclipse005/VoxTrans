import { describe, expect, it } from "vitest";
import {
  holdsPipelineSlot,
  isBusyStatus,
} from "./taskStatus";
import { normalizeTranscribeStatus } from "./stateMachine";

describe("taskStatus helpers", () => {
  it("pipeline slot is held by processing and review parks", () => {
    expect(holdsPipelineSlot("processing")).toBe(true);
    expect(holdsPipelineSlot("review_source")).toBe(true);
    expect(holdsPipelineSlot("review_target")).toBe(true);
    expect(holdsPipelineSlot("queued")).toBe(false);
    expect(holdsPipelineSlot("pending")).toBe(false);
    expect(holdsPipelineSlot("done")).toBe(false);
  });

  it("busy for delete/edit is only queued/processing", () => {
    expect(isBusyStatus("queued")).toBe(true);
    expect(isBusyStatus("processing")).toBe(true);
    expect(isBusyStatus("review_source")).toBe(false);
    expect(isBusyStatus("review_target")).toBe(false);
    expect(isBusyStatus("done")).toBe(false);
  });
});

describe("normalizeTranscribeStatus review values", () => {
  it("keeps review_* statuses", () => {
    expect(normalizeTranscribeStatus("review_source")).toBe("review_source");
    expect(normalizeTranscribeStatus("review_target")).toBe("review_target");
  });
});
