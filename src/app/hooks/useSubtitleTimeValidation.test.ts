import { describe, expect, it, vi } from "vitest";
import { act, renderHook } from "@testing-library/react";
import i18n from "i18next";
import type { SubtitleCue } from "../../features/media/types";
import { useSubtitleTimeValidation } from "./useSubtitleTimeValidation";

function cue(overrides: Partial<SubtitleCue> = {}): SubtitleCue {
  return {
    id: "c1",
    startMs: 0,
    endMs: 1000,
    text: "hi",
    translatedText: "",
    ...overrides,
  };
}

describe("useSubtitleTimeValidation", () => {
  it("commits start in milliseconds and clamps end forward", () => {
    const onUpdateCue = vi.fn();
    const { result } = renderHook(() => useSubtitleTimeValidation({ onUpdateCue }));
    act(() => {
      result.current.commitStart(cue(), 2500);
    });
    expect(onUpdateCue).toHaveBeenCalledWith("c1", { startMs: 2500, endMs: 2500 });
  });

  it("commits end in milliseconds and clamps it to start", () => {
    const onUpdateCue = vi.fn();
    const { result } = renderHook(() => useSubtitleTimeValidation({ onUpdateCue }));
    act(() => {
      result.current.commitEnd(cue({ startMs: 800 }), 100);
    });
    expect(onUpdateCue).toHaveBeenCalledWith("c1", { endMs: 800 });
  });

  it("stores per-edge parse errors and clears them on commit or cancel", () => {
    const onUpdateCue = vi.fn();
    const { result } = renderHook(() => useSubtitleTimeValidation({ onUpdateCue }));
    act(() => {
      result.current.rejectStart("c1");
    });
    expect(result.current.timeErrorByCue.c1).toBe(
      i18n.t("subtitles:timeValidation.startInvalid"),
    );

    act(() => {
      result.current.commitStart(cue(), 500);
    });
    expect(result.current.timeErrorByCue.c1).toBe("");
    expect(onUpdateCue).toHaveBeenCalledWith("c1", { startMs: 500, endMs: 1000 });

    act(() => {
      result.current.rejectEnd("c1");
    });
    expect(result.current.timeErrorByCue.c1).toBe(
      i18n.t("subtitles:timeValidation.endInvalid"),
    );

    act(() => {
      result.current.clearTimeError("c1");
    });
    expect(result.current.timeErrorByCue.c1).toBe("");
  });
});
