import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import type { SubtitleCue } from "../../features/media/types";

type UseSubtitleTimeValidationArgs = {
  onUpdateCue: (cueId: string, patch: Partial<SubtitleCue>) => void;
};

export function useSubtitleTimeValidation({ onUpdateCue }: UseSubtitleTimeValidationArgs) {
  const { t } = useTranslation(["subtitles"]);
  const [timeErrorByCue, setTimeErrorByCue] = useState<Record<string, string>>({});

  const clearTimeError = useCallback((cueId: string) => {
    setTimeErrorByCue((old) => {
      if (!old[cueId]) return old;
      return { ...old, [cueId]: "" };
    });
  }, []);

  const commitStart = useCallback((cue: SubtitleCue, startMs: number) => {
    onUpdateCue(cue.id, { startMs, endMs: Math.max(startMs, cue.endMs) });
    clearTimeError(cue.id);
  }, [clearTimeError, onUpdateCue]);

  const commitEnd = useCallback((cue: SubtitleCue, endMs: number) => {
    onUpdateCue(cue.id, { endMs: Math.max(endMs, cue.startMs) });
    clearTimeError(cue.id);
  }, [clearTimeError, onUpdateCue]);

  const rejectStart = useCallback((cueId: string) => {
    setTimeErrorByCue((old) => ({ ...old, [cueId]: t("subtitles:timeValidation.startInvalid") }));
  }, [t]);

  const rejectEnd = useCallback((cueId: string) => {
    setTimeErrorByCue((old) => ({ ...old, [cueId]: t("subtitles:timeValidation.endInvalid") }));
  }, [t]);

  return {
    timeErrorByCue,
    commitStart,
    commitEnd,
    rejectStart,
    rejectEnd,
    clearTimeError,
  };
}
