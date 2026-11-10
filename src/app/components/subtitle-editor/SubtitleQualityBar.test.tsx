import { describe, expect, it, vi } from "vitest";
import { fireEvent, render } from "@testing-library/react";
import SubtitleQualityBar from "./SubtitleQualityBar";
import {
  diagnoseCues,
  summarizeDiagnostics,
  type Diagnostic,
} from "../../../features/media/subtitleDiagnostics";
import type { SubtitleCue } from "../../../features/media/types";

function cue(
  id: string,
  startMs: number,
  endMs: number,
  text = "text",
  translatedText = "译文",
): SubtitleCue {
  return { id, startMs, endMs, text, translatedText };
}

function renderBar(options: {
  cues: SubtitleCue[];
  canEdit?: boolean;
  applying?: boolean;
  activeKind?: Diagnostic["kind"] | null;
  onActiveKindChange?: (kind: Diagnostic["kind"] | null) => void;
  onLocate?: (cueId: string) => void;
  onApply?: () => void;
}) {
  const diagnostics = diagnoseCues(options.cues, "zh-CN");
  return render(
    <SubtitleQualityBar
      diagnostics={diagnostics}
      summary={summarizeDiagnostics(diagnostics)}
      canEdit={options.canEdit ?? true}
      applying={options.applying ?? false}
      activeKind={options.activeKind ?? null}
      onActiveKindChange={options.onActiveKindChange ?? vi.fn()}
      onLocate={options.onLocate ?? vi.fn()}
      onApply={options.onApply ?? vi.fn()}
    />,
  );
}

describe("SubtitleQualityBar", () => {
  it("summarises the severity mix and offers the auto-fix count", () => {
    const { container } = renderBar({
      cues: [cue("a", 1000, 2600), cue("b", 2000, 3000)],
    });
    const strip = container.querySelector(".subtitle-quality-strip");
    expect(strip?.className).toContain("error");
    expect(container.querySelector(".subtitle-quality-label")?.textContent).toContain("1");
    expect(container.querySelector(".subtitle-quality-apply")?.textContent).toContain("美化");
  });

  it("uses the warning tone when nothing is blocking", () => {
    // gap 600ms < 800 → warning only, no overlay.
    const { container } = renderBar({ cues: [cue("a", 0, 1000), cue("b", 1600, 2600)] });
    expect(container.querySelector(".subtitle-quality-strip")?.className).toContain("warning");
  });

  it("renders one chip per issue kind with its count", () => {
    const { container } = renderBar({
      cues: [cue("a", 1000, 2600), cue("b", 2000, 3000), cue("c", 2100, 4000)],
    });
    const chips = Array.from(container.querySelectorAll(".subtitle-quality-chip"));
    expect(chips.length).toBeGreaterThan(0);
    expect(chips.some((chip) => chip.textContent?.includes("时间重叠"))).toBe(true);
  });

  it("forwards a chip click as a filter toggle", () => {
    const onActiveKindChange = vi.fn();
    const { container } = renderBar({
      cues: [cue("a", 1000, 2600), cue("b", 2000, 3000)],
      onActiveKindChange,
    });
    const chip = Array.from(container.querySelectorAll<HTMLButtonElement>(".subtitle-quality-chip"))
      .find((node) => node.textContent?.includes("时间重叠"));
    expect(chip).toBeDefined();
    fireEvent.click(chip as HTMLButtonElement);
    expect(onActiveKindChange).toHaveBeenCalledWith("overlap");
  });

  it("marks the active chip for assistive tech", () => {
    const { container } = renderBar({
      cues: [cue("a", 1000, 2600), cue("b", 2000, 3000)],
      activeKind: "overlap",
    });
    const active = Array.from(container.querySelectorAll(".subtitle-quality-chip")).find(
      (node) => node.getAttribute("aria-pressed") === "true",
    );
    expect(active?.textContent).toContain("时间重叠");
  });

  it("opens the issue list and forwards locate", () => {
    const onLocate = vi.fn();
    const { container } = renderBar({
      cues: [cue("a", 1000, 2600), cue("b", 2000, 3000)],
      onLocate,
    });
    const panel = container.querySelector(".subtitle-quality-panel");
    expect(panel?.getAttribute("data-open")).toBe("false");
    expect(panel?.getAttribute("aria-hidden")).toBe("true");

    fireEvent.click(container.querySelector(".subtitle-quality-summary") as HTMLButtonElement);
    expect(container.querySelector(".subtitle-quality-panel")?.getAttribute("data-open")).toBe("true");

    const item = container.querySelector<HTMLButtonElement>(".subtitle-quality-item");
    expect(item).not.toBeNull();
    fireEvent.click(item as HTMLButtonElement);
    expect(onLocate).toHaveBeenCalledTimes(1);
    expect(container.querySelector(".subtitle-quality-panel")?.getAttribute("data-open")).toBe("false");
  });

  it("disables the action in read-only mode and while applying", () => {
    const cues = [cue("a", 1000, 2600), cue("b", 2000, 3000)];
    const readOnly = renderBar({ cues, canEdit: false });
    expect(
      readOnly.container.querySelector<HTMLButtonElement>(".subtitle-quality-apply")?.disabled,
    ).toBe(true);

    const busy = renderBar({ cues, applying: true });
    const button = busy.container.querySelector<HTMLButtonElement>(".subtitle-quality-apply");
    expect(button?.disabled).toBe(true);
    expect(button?.textContent).toContain("处理中");
  });

  it("keeps the strip honest when only manual issues remain", () => {
    // Empty source text cannot be auto-fixed, so the CTA must stay disabled.
    const { container } = renderBar({ cues: [cue("a", 0, 3000, "   ", "译文")] });
    expect(
      container.querySelector<HTMLButtonElement>(".subtitle-quality-apply")?.disabled,
    ).toBe(true);
    expect(container.querySelector(".subtitle-quality-manual")?.textContent).toContain("1");
  });
});
