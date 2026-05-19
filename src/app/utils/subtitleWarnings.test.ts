import { describe, expect, it } from "vitest";
import { buildCueWarningsById } from "./subtitleWarnings";
import type { SubtitleCue } from "../../features/media/types";

function makeCue(id: string, startMs: number, endMs: number): SubtitleCue {
  return { id, startMs, endMs, text: "text", translatedText: "" };
}

describe("buildCueWarningsById", () => {
  it("maps warnings to cue ids by cue number", () => {
    const cues = [makeCue("a", 0, 1000), makeCue("b", 1000, 2000), makeCue("c", 2000, 3000)];
    const warnings = ["Cue 1 has empty text", "Cue 3 is longer than 60 seconds"];
    const result = buildCueWarningsById(cues, warnings);
    expect(result["a"]).toEqual(["文本为空"]);
    expect(result["c"]).toEqual(["时长超过 60 秒"]);
    expect(result["b"]).toBeUndefined();
  });

  it("translates warning messages to Chinese", () => {
    const cues = [makeCue("a", 0, 1000)];
    const warnings = [
      "Cue 1 has end before start",
      "Cue 1 overlaps with cue 2",
    ];
    const result = buildCueWarningsById(cues, warnings);
    expect(result["a"]).toEqual(["结束时间早于开始时间", "与第 2 条时间重叠"]);
  });

  it("ignores warnings without cue number", () => {
    const cues = [makeCue("a", 0, 1000)];
    const warnings = ["some generic warning", "Cue 1 has empty text"];
    const result = buildCueWarningsById(cues, warnings);
    expect(result["a"]).toEqual(["文本为空"]);
  });

  it("ignores out-of-range cue numbers", () => {
    const cues = [makeCue("a", 0, 1000)];
    const warnings = ["Cue 0 has empty text", "Cue 99 has empty text"];
    const result = buildCueWarningsById(cues, warnings);
    expect(Object.keys(result)).toHaveLength(0);
  });

  it("matches cues by sorted order, not input order", () => {
    const cues = [
      makeCue("z", 2000, 3000),
      makeCue("a", 0, 1000),
      makeCue("m", 1000, 2000),
    ];
    const warnings = ["Cue 2 has empty text"];
    const result = buildCueWarningsById(cues, warnings);
    expect(result["m"]).toEqual(["文本为空"]);
    expect(result["a"]).toBeUndefined();
    expect(result["z"]).toBeUndefined();
  });

  it("accumulates multiple warnings for the same cue", () => {
    const cues = [makeCue("a", 0, 1000)];
    const warnings = ["Cue 1 has empty text", "Cue 1 has end before start"];
    const result = buildCueWarningsById(cues, warnings);
    expect(result["a"]).toEqual(["文本为空", "结束时间早于开始时间"]);
  });

  it("returns empty object when no cues or warnings", () => {
    expect(buildCueWarningsById([], [])).toEqual({});
    expect(buildCueWarningsById([makeCue("a", 0, 1000)], [])).toEqual({});
  });
});
