import { useState } from "react";
import { useTranslation } from "react-i18next";
import type { SubtitleCue } from "../../features/media/types";
import { parseSrtTime } from "../../features/media/srt";

type UseSubtitleTimeValidationArgs = {
  onUpdateCue: (cueId: string, patch: Partial<SubtitleCue>) => void;
};

export function useSubtitleTimeValidation({ onUpdateCue }: UseSubtitleTimeValidationArgs) {
  const { t } = useTranslation(["subtitles"]);
  const [timeErrorByCue, setTimeErrorByCue] = useState<Record<string, string>>({});

  const applyStart = (cue: SubtitleCue, value: string) => {
    const parsed = parseSrtTime(value);
    if (parsed == null) {
      setTimeErrorByCue((old) => ({ ...old, [cue.id]: t("subtitles:timeValidation.startInvalid") }));
      return;
    }
    onUpdateCue(cue.id, { startMs: parsed, endMs: Math.max(parsed, cue.endMs) });
    setTimeErrorByCue((old) => ({ ...old, [cue.id]: "" }));
  };

  const applyEnd = (cue: SubtitleCue, value: string) => {
    const parsed = parseSrtTime(value);
    if (parsed == null) {
      setTimeErrorByCue((old) => ({ ...old, [cue.id]: t("subtitles:timeValidation.endInvalid") }));
      return;
    }
    onUpdateCue(cue.id, { endMs: Math.max(parsed, cue.startMs) });
    setTimeErrorByCue((old) => ({ ...old, [cue.id]: "" }));
  };

  return {
    timeErrorByCue,
    applyStart,
    applyEnd,
  };
}
