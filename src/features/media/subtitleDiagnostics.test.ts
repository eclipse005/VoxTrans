import { describe, expect, it } from "vitest";
import {
  MAX_CUE_MS,
  MIN_HOLD_MS,
  diagnoseCues,
  groupDiagnosticsByCueId,
  isCjkTarget,
  summarizeDiagnostics,
} from "./subtitleDiagnostics";
import type { SubtitleCue } from "./types";

function cue(
  id: string,
  startMs: number,
  endMs: number,
  text = "text",
  translatedText = "译文",
): SubtitleCue {
  return { id, startMs, endMs, text, translatedText };
}

describe("diagnoseCues timing", () => {
  it("reports an end that runs past the next start as an error", () => {
    // The reported symptom: cue 17 ends at 15,889 while cue 18 starts at 15,828.
    const diagnostics = diagnoseCues([cue("a", 1000, 2600), cue("b", 2000, 3000)]);
    const overlap = diagnostics.find((d) => d.kind === "overlap");
    expect(overlap).toBeDefined();
    expect(overlap?.severity).toBe("error");
    expect(overlap?.autoFixable).toBe(true);
    expect(overlap?.params.nextCueNumber).toBe(2);
    expect(overlap?.params.overlapMs).toBe(600);
  });

  it("does not report touching cues", () => {
    const diagnostics = diagnoseCues([cue("a", 1000, 2000), cue("b", 2000, 3000)]);
    expect(diagnostics.filter((d) => d.kind === "overlap")).toHaveLength(0);
  });

  it("reports a backwards next start without inventing an overlap", () => {
    const diagnostics = diagnoseCues([cue("a", 2000, 2600), cue("b", 1500, 3000)]);
    expect(diagnostics.some((d) => d.kind === "startReversed")).toBe(true);
    expect(diagnostics.some((d) => d.kind === "overlap")).toBe(false);
  });

  it("flags a sub-minimum hold and a tiny gap separately", () => {
    const diagnostics = diagnoseCues([cue("a", 0, 300), cue("b", 900, 2000)]);
    expect(diagnostics.find((d) => d.kind === "tooShort")?.params.durationMs).toBe(300);
    // gap 900-300 = 600 < 800
    expect(diagnostics.find((d) => d.kind === "gapTiny")?.params.gapMs).toBe(600);
  });

  it("promises a short-cue fix only when there is room to grow", () => {
    // Real-data case: the next cue starts 580ms after this one, so beautify
    // cannot pad it to 800 no matter what. Reporting it as auto-fixable made
    // the button claim "fix 11" and then change nothing.
    const tight = diagnoseCues([cue("a", 8700, 9280), cue("b", 9280, 10_000)]);
    const tightShort = tight.find((d) => d.kind === "tooShort");
    expect(tightShort?.params.durationMs).toBe(580);
    expect(tightShort?.autoFixable).toBe(false);

    const roomy = diagnoseCues([cue("a", 0, 300), cue("b", 5000, 7000)]);
    expect(roomy.find((d) => d.kind === "tooShort")?.autoFixable).toBe(true);

    // The last cue has no successor, so it can always be padded.
    const last = diagnoseCues([cue("a", 0, 300)]);
    expect(last.find((d) => d.kind === "tooShort")?.autoFixable).toBe(true);
  });

  it("does not flag a gap of exactly the threshold", () => {
    const diagnostics = diagnoseCues([cue("a", 0, 1000), cue("b", 1800, 2600)]);
    expect(diagnostics.some((d) => d.kind === "gapTiny")).toBe(false);
    expect(diagnostics.some((d) => d.kind === "tooShort")).toBe(false);
  });

  it("flags cues longer than the maximum", () => {
    const diagnostics = diagnoseCues([cue("a", 0, MAX_CUE_MS + 1)]);
    expect(diagnostics.find((d) => d.kind === "tooLong")?.params.seconds).toBe(60);
  });

  it("mirrors the Rust minimum hold constant", () => {
    expect(MIN_HOLD_MS).toBe(800);
  });
});

describe("diagnoseCues content", () => {
  it("always flags empty source text, never as auto-fixable", () => {
    const diagnostics = diagnoseCues([cue("a", 0, 1000, "   ", "译文")]);
    const empty = diagnostics.find((d) => d.kind === "emptyText");
    expect(empty?.autoFixable).toBe(false);
    expect(empty?.severity).toBe("error");
  });

  it("reports missing translations only when the task has some", () => {
    const withTranslations = diagnoseCues([
      cue("a", 0, 1000, "a", "译文"),
      cue("b", 1000, 2000, "b", ""),
    ]);
    expect(withTranslations.filter((d) => d.kind === "missingTranslation")).toHaveLength(1);

    const sourceOnly = diagnoseCues([
      cue("a", 0, 1000, "a", ""),
      cue("b", 1000, 2000, "b", ""),
    ]);
    expect(sourceOnly.some((d) => d.kind === "missingTranslation")).toBe(false);
  });

  it("only checks CJK punctuation for CJK targets", () => {
    const cues = [cue("a", 0, 1000, "hello.", "你好，")];
    expect(diagnoseCues(cues, "zh-CN").some((d) => d.kind === "cjkPunctuation")).toBe(true);
    expect(diagnoseCues(cues, "en").some((d) => d.kind === "cjkPunctuation")).toBe(false);
    expect(diagnoseCues(cues).some((d) => d.kind === "cjkPunctuation")).toBe(false);
  });

  it("does not flag internal punctuation as a boundary problem", () => {
    const cues = [cue("a", 0, 1000, "hi", "你好，世界")];
    expect(diagnoseCues(cues, "zh-CN").some((d) => d.kind === "cjkPunctuation")).toBe(false);
  });
});

describe("isCjkTarget", () => {
  it("accepts every zh/yue shape and nothing else", () => {
    for (const tag of ["zh", "zh-CN", "zh-TW", "yue", "yue-HK", "ZH-cn"]) {
      expect(isCjkTarget(tag)).toBe(true);
    }
    for (const tag of ["en", "ja", "zhuang", "", undefined]) {
      expect(isCjkTarget(tag)).toBe(false);
    }
  });
});

describe("summarizeDiagnostics", () => {
  it("separates auto-fixable from manual and keeps a stable kind order", () => {
    const diagnostics = diagnoseCues(
      [cue("a", 0, 300, "", "译文"), cue("b", 400, 3200)],
      "zh-CN",
    );
    const summary = summarizeDiagnostics(diagnostics);
    expect(summary.total).toBe(diagnostics.length);
    expect(summary.autoFixable + summary.manual).toBe(summary.total);
    const kinds = summary.byKind.map((entry) => entry.kind);
    expect(kinds).toEqual([...kinds].sort(
      (a, b) => ["overlap", "emptyText", "missingTranslation", "tooShort", "tooLong", "gapTiny", "cjkPunctuation", "startReversed"].indexOf(a)
        - ["overlap", "emptyText", "missingTranslation", "tooShort", "tooLong", "gapTiny", "cjkPunctuation", "startReversed"].indexOf(b),
    ));
    expect(summary.byKind.reduce((sum, entry) => sum + entry.count, 0)).toBe(summary.total);
  });

  it("handles an empty list and a clean list", () => {
    expect(summarizeDiagnostics([]).total).toBe(0);
    expect(summarizeDiagnostics(diagnoseCues([cue("a", 0, 2000)])).total).toBe(0);
  });
});

describe("groupDiagnosticsByCueId", () => {
  it("keeps every diagnostic for a cue and skips clean cues", () => {
    const diagnostics = diagnoseCues([
      cue("a", 0, 300, "", "译文"),
      cue("b", 5000, 7000),
    ]);
    const grouped = groupDiagnosticsByCueId(diagnostics);
    expect(grouped.a.length).toBeGreaterThan(1);
    expect(grouped.b).toBeUndefined();
  });
});
