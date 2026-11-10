import type { SubtitleCue } from "./types";

/**
 * Structural quality checks for the subtitle editor.
 *
 * This is a *display* heuristic: it decides what the editor shows, never what
 * gets written. The transform itself lives in Rust
 * (`services::subtitle_beautify`), so the button and the automatic pipeline
 * cannot disagree. The thresholds below mirror that module — keep
 * `subtitleDiagnostics.test.ts` in sync when either side moves.
 */
export const MIN_HOLD_MS = 800;
export const MAX_GAP_FILL_MS = 800;
export const MAX_CUE_MS = 60_000;

export type DiagnosticKind =
  | "overlap"
  | "startReversed"
  | "emptyText"
  | "missingTranslation"
  | "tooShort"
  | "gapTiny"
  | "tooLong"
  | "cjkPunctuation";

export type DiagnosticSeverity = "error" | "warning";

export type Diagnostic = {
  /** Index in the editor's cue array (0-based). */
  index: number;
  cueId: string;
  /** 1-based cue number as the user sees it. */
  cueNumber: number;
  kind: DiagnosticKind;
  severity: DiagnosticSeverity;
  /** True when the beautify action is expected to clear this. */
  autoFixable: boolean;
  /** Extra numbers the message needs (gap ms, seconds, cue number…). */
  params: Record<string, number>;
};

const CJK_TARGET_PREFIXES = ["zh", "yue"];
const BOUNDARY_PUNCTUATION = new Set([".", "。", ",", "，"]);

export function isCjkTarget(targetLang: string | undefined): boolean {
  const lower = (targetLang ?? "").trim().toLowerCase();
  if (!lower) return false;
  return CJK_TARGET_PREFIXES.some(
    (prefix) => lower === prefix || lower.startsWith(`${prefix}-`),
  );
}

function hasBoundaryPunctuation(text: string): boolean {
  const trimmed = text.trim();
  if (!trimmed) return false;
  const chars = Array.from(trimmed);
  const first = chars[0];
  const last = chars[chars.length - 1];
  return BOUNDARY_PUNCTUATION.has(first) || BOUNDARY_PUNCTUATION.has(last);
}

/**
 * Collect every problem in one pass. `targetLang` gates the text checks that
 * only make sense for CJK output (a trailing "." is normal in English).
 *
 * Missing translations are only reported when the task actually has
 * translations — a source-only task is not "missing" anything.
 */
export function diagnoseCues(
  cues: SubtitleCue[],
  targetLang?: string,
): Diagnostic[] {
  const diagnostics: Diagnostic[] = [];
  const cjkTarget = isCjkTarget(targetLang);
  const expectsTranslation = cues.some((cue) => cue.translatedText.trim().length > 0);

  for (let index = 0; index < cues.length; index += 1) {
    const cue = cues[index];
    const startMs = Math.max(0, Math.round(cue.startMs));
    const endMs = Math.round(cue.endMs);
    const durationMs = endMs - startMs;
    const next = index + 1 < cues.length ? cues[index + 1] : null;
    const base = {
      index,
      cueId: cue.id,
      cueNumber: index + 1,
    } as const;
    const push = (
      kind: DiagnosticKind,
      severity: DiagnosticSeverity,
      autoFixable: boolean,
      params: Record<string, number> = {},
    ) => {
      diagnostics.push({ ...base, kind, severity, autoFixable, params });
    };

    if (!cue.text.trim()) {
      push("emptyText", "error", false);
    }
    if (expectsTranslation && !cue.translatedText.trim()) {
      push("missingTranslation", "warning", false);
    }
    if (endMs < startMs) {
      // Unreachable through the editor's inputs (they clamp on commit), but
      // imported data can still carry it.
      push("startReversed", "error", false, { startMs, endMs });
    }
    if (durationMs > MAX_CUE_MS) {
      push("tooLong", "warning", false, { seconds: Math.round(durationMs / 1000) });
    }

    if (next) {
      const nextStartMs = Math.max(0, Math.round(next.startMs));
      if (nextStartMs < startMs) {
        push("startReversed", "error", false, { startMs, nextStartMs });
      } else if (nextStartMs < endMs) {
        push("overlap", "error", true, {
          overlapMs: endMs - nextStartMs,
          nextCueNumber: index + 2,
        });
      } else {
        const gapMs = nextStartMs - endMs;
        if (gapMs > 0 && gapMs < MAX_GAP_FILL_MS) {
          push("gapTiny", "warning", true, { gapMs });
        }
      }
    }

    if (durationMs >= 0 && durationMs < MIN_HOLD_MS) {
      push("tooShort", "warning", true, { durationMs });
    }

    if (cjkTarget && hasBoundaryPunctuation(cue.translatedText)) {
      push("cjkPunctuation", "warning", true);
    }
  }

  return diagnostics;
}

export type DiagnosticSummary = {
  total: number;
  errors: number;
  warnings: number;
  autoFixable: number;
  manual: number;
  /** One count per kind, in the canonical order used by the filter chips. */
  byKind: Array<{ kind: DiagnosticKind; count: number }>;
};

const KIND_ORDER: DiagnosticKind[] = [
  "overlap",
  "emptyText",
  "missingTranslation",
  "tooShort",
  "tooLong",
  "gapTiny",
  "cjkPunctuation",
  "startReversed",
];

export function summarizeDiagnostics(diagnostics: Diagnostic[]): DiagnosticSummary {
  const counts = new Map<DiagnosticKind, number>();
  let errors = 0;
  let warnings = 0;
  let autoFixable = 0;

  for (const diagnostic of diagnostics) {
    counts.set(diagnostic.kind, (counts.get(diagnostic.kind) ?? 0) + 1);
    if (diagnostic.severity === "error") errors += 1;
    else warnings += 1;
    if (diagnostic.autoFixable) autoFixable += 1;
  }

  return {
    total: diagnostics.length,
    errors,
    warnings,
    autoFixable,
    manual: diagnostics.length - autoFixable,
    byKind: KIND_ORDER.filter((kind) => (counts.get(kind) ?? 0) > 0).map((kind) => ({
      kind,
      count: counts.get(kind) ?? 0,
    })),
  };
}

/** Cues touched by at least one diagnostic, for row-level badges. */
export function groupDiagnosticsByCueId(
  diagnostics: Diagnostic[],
): Record<string, Diagnostic[]> {
  const byCueId: Record<string, Diagnostic[]> = {};
  for (const diagnostic of diagnostics) {
    if (!byCueId[diagnostic.cueId]) byCueId[diagnostic.cueId] = [];
    byCueId[diagnostic.cueId].push(diagnostic);
  }
  return byCueId;
}
