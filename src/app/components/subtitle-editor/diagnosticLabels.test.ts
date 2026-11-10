import { describe, expect, it } from "vitest";
import i18n from "../../../i18n";
import { describeDiagnostic, diagnosticLabel, kindLabel } from "./diagnosticLabels";
import type {
  Diagnostic,
  DiagnosticKind,
} from "../../../features/media/subtitleDiagnostics";

const ALL_KINDS: DiagnosticKind[] = [
  "overlap",
  "startReversed",
  "emptyText",
  "missingTranslation",
  "tooShort",
  "gapTiny",
  "tooLong",
  "cjkPunctuation",
];

function diagnostic(overrides: Partial<Diagnostic> = {}): Diagnostic {
  return {
    index: 0,
    cueId: "cue-1",
    cueNumber: 1,
    kind: "overlap",
    severity: "warning",
    autoFixable: true,
    params: {},
    ...overrides,
  };
}

describe("diagnostic labels", () => {
  it("has a translated label for every kind", () => {
    // A new DiagnosticKind without an i18n entry renders the raw key to the
    // user, which no type or lint catches.
    for (const kind of ALL_KINDS) {
      const label = kindLabel(i18n.t.bind(i18n), kind);
      expect(label).not.toContain("quality.kind");
      expect(label.trim().length).toBeGreaterThan(0);
    }
  });

  it("describes every kind without leaking a raw key", () => {
    const params: Record<DiagnosticKind, Record<string, number>> = {
      overlap: { overlapMs: 1500, nextCueNumber: 2 },
      startReversed: { startMs: 1000, nextStartMs: 900 },
      emptyText: {},
      missingTranslation: {},
      tooShort: { durationMs: 580, roomMs: 580 },
      gapTiny: { gapMs: 300 },
      tooLong: { seconds: 75 },
      cjkPunctuation: {},
    };
    for (const kind of ALL_KINDS) {
      const text = describeDiagnostic(
        i18n.t.bind(i18n),
        diagnostic({ kind, params: params[kind] }),
      );
      expect(text).not.toContain("quality.detail");
      expect(text.trim().length).toBeGreaterThan(0);
    }
  });

  it("explains why a blocked short cue cannot be lengthened", () => {
    const blocked = diagnostic({
      kind: "tooShort",
      autoFixable: false,
      params: { durationMs: 400, roomMs: 580 },
    });
    const fixable = diagnostic({
      kind: "tooShort",
      autoFixable: true,
      params: { durationMs: 400, roomMs: 5000 },
    });
    const blockedText = describeDiagnostic(i18n.t.bind(i18n), blocked);
    const fixableText = describeDiagnostic(i18n.t.bind(i18n), fixable);
    // BONE-BREAK target: removing the `tooShortTight` branch makes both read
    // the same copy, so `roomMs` would never reach the user.
    expect(blockedText).toContain("400");
    expect(blockedText).toContain("580");
    expect(fixableText).toContain("400");
    expect(fixableText).not.toContain("580");
  });

  it("prefixes the tooltip with the kind", () => {
    const text = diagnosticLabel(
      i18n.t.bind(i18n),
      diagnostic({ kind: "gapTiny", params: { gapMs: 200 } }),
    );
    expect(text).toContain(":");
    expect(text).toContain("200");
  });
});
