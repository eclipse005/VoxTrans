import { describe, expect, it, vi } from "vitest";
import { act, renderHook } from "@testing-library/react";
import type { MouseEvent } from "react";
import { useSubtitleSelection } from "./useSubtitleSelection";

function click(partial: Partial<MouseEvent<HTMLElement>> = {}): MouseEvent<HTMLElement> {
  return {
    shiftKey: false,
    ctrlKey: false,
    metaKey: false,
    ...partial,
  } as MouseEvent<HTMLElement>;
}

describe("useSubtitleSelection", () => {
  it("keeps a multi-selection when focusing an already-selected cue", () => {
    const onSelectedCueChanged = vi.fn();
    const { result } = renderHook(() =>
      useSubtitleSelection({
        cueIds: ["a", "b", "c"],
        onSelectedCueChanged,
      }),
    );

    act(() => {
      result.current.handleCueClick("a", click());
      result.current.handleCueClick("c", click({ shiftKey: true }));
    });
    expect(result.current.validSelectedCueIds).toEqual(["a", "b", "c"]);

    act(() => {
      result.current.ensureSelected("b");
    });
    expect(result.current.validSelectedCueIds).toEqual(["a", "b", "c"]);
    expect(onSelectedCueChanged).toHaveBeenLastCalledWith("b");
  });

  it("becomes exclusive when focusing a cue that is not selected", () => {
    const { result } = renderHook(() =>
      useSubtitleSelection({ cueIds: ["a", "b", "c"] }),
    );

    act(() => {
      result.current.handleCueClick("a", click());
      result.current.handleCueClick("b", click({ ctrlKey: true }));
    });
    expect(result.current.validSelectedCueIds).toEqual(["a", "b"]);

    act(() => {
      result.current.ensureSelected("c");
    });
    expect(result.current.validSelectedCueIds).toEqual(["c"]);
  });

  it("toggles a cue with ctrl-click", () => {
    const { result } = renderHook(() =>
      useSubtitleSelection({ cueIds: ["a", "b"] }),
    );

    act(() => {
      result.current.handleCueClick("a", click());
      result.current.handleCueClick("b", click({ ctrlKey: true }));
    });
    expect(result.current.validSelectedCueIds).toEqual(["a", "b"]);

    act(() => {
      result.current.handleCueClick("a", click({ ctrlKey: true }));
    });
    expect(result.current.validSelectedCueIds).toEqual(["b"]);
  });
});
