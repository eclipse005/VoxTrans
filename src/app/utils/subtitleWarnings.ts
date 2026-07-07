import i18n from "../../i18n";
import type { SubtitleCue } from "../../features/media/types";

const CUE_NUMBER_RE = /\bcue\s+(\d+)\b/i;

function warningText(raw: string): string {
  const text = raw.trim();
  if (/has empty text/i.test(text)) return i18n.t("subtitles:warnings.emptyText");
  if (/has end before start/i.test(text)) return i18n.t("subtitles:warnings.endBeforeStart");
  if (/is longer than 60 seconds/i.test(text)) return i18n.t("subtitles:warnings.tooLong");
  const overlap = /overlaps with cue\s+(\d+)/i.exec(text);
  if (overlap) return i18n.t("subtitles:warnings.overlap", { n: overlap[1] });
  return text;
}

export function buildCueWarningsById(cues: SubtitleCue[], warnings: string[]): Record<string, string[]> {
  const warningsById: Record<string, string[]> = {};
  const pushWarning = (cueId: string, warning: string) => {
    if (!warningsById[cueId]) warningsById[cueId] = [];
    warningsById[cueId].push(warning);
  };

  const ordered = [...cues].sort((a, b) => (a.startMs - b.startMs) || (a.endMs - b.endMs));
  for (const rawWarning of warnings) {
    const matched = CUE_NUMBER_RE.exec(rawWarning);
    if (!matched) continue;
    const cueNo = Number.parseInt(matched[1], 10);
    if (!Number.isFinite(cueNo) || cueNo <= 0) continue;
    const cue = ordered[cueNo - 1];
    if (!cue) continue;
    pushWarning(cue.id, warningText(rawWarning));
  }
  return warningsById;
}
