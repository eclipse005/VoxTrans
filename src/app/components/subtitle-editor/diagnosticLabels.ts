import type { TFunction } from "i18next";
import type { Diagnostic, DiagnosticKind } from "../../../features/media/subtitleDiagnostics";

/** One i18n key per diagnostic kind — shared by the quality bar and the row
 *  badge tooltip so a new kind cannot show up in only one of them. */
export const KIND_LABEL_KEY: Record<DiagnosticKind, string> = {
  overlap: "quality.kind.overlap",
  emptyText: "quality.kind.emptyText",
  missingTranslation: "quality.kind.missingTranslation",
  tooShort: "quality.kind.tooShort",
  tooLong: "quality.kind.tooLong",
  gapTiny: "quality.kind.gapTiny",
  cjkPunctuation: "quality.kind.cjkPunctuation",
  startReversed: "quality.kind.startReversed",
};

export function kindLabel(t: TFunction, kind: DiagnosticKind): string {
  return t(`subtitles:${KIND_LABEL_KEY[kind]}`);
}

/** Human sentence for one diagnostic, used in the popover list. */
export function describeDiagnostic(t: TFunction, diagnostic: Diagnostic): string {
  switch (diagnostic.kind) {
    case "overlap":
      return t("subtitles:quality.detail.overlap", {
        next: diagnostic.params.nextCueNumber,
        ms: diagnostic.params.overlapMs,
      });
    case "gapTiny":
      return t("subtitles:quality.detail.gapTiny", { ms: diagnostic.params.gapMs });
    case "tooShort":
      return t("subtitles:quality.detail.tooShort", { ms: diagnostic.params.durationMs });
    case "tooLong":
      return t("subtitles:quality.detail.tooLong", { seconds: diagnostic.params.seconds });
    case "startReversed":
      return t("subtitles:quality.detail.startReversed");
    case "emptyText":
      return t("subtitles:quality.detail.emptyText");
    case "missingTranslation":
      return t("subtitles:quality.detail.missingTranslation");
    case "cjkPunctuation":
      return t("subtitles:quality.detail.cjkPunctuation");
  }
}

/** One line for a native tooltip: "Overlap: overlaps cue 18 by 61 ms". */
export function diagnosticLabel(t: TFunction, diagnostic: Diagnostic): string {
  return `${kindLabel(t, diagnostic.kind)}: ${describeDiagnostic(t, diagnostic)}`;
}
