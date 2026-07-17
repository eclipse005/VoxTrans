import { describe, expect, it } from "vitest";
import { isSubtitleEditMode, resolveSubtitleEditorMode } from "./subtitleEditorMode";

describe("subtitleEditorMode", () => {
  it("uses edit mode only for parked human-review / done states", () => {
    expect(resolveSubtitleEditorMode("done")).toBe("edit");
    expect(resolveSubtitleEditorMode("review_source")).toBe("edit");
    expect(resolveSubtitleEditorMode("review_target")).toBe("edit");
    expect(isSubtitleEditMode("done")).toBe(true);
  });

  it("uses preview mode for machine-running and other statuses", () => {
    expect(resolveSubtitleEditorMode("processing")).toBe("preview");
    expect(resolveSubtitleEditorMode("queued")).toBe("preview");
    expect(resolveSubtitleEditorMode("pending")).toBe("preview");
    expect(resolveSubtitleEditorMode("error")).toBe("preview");
    expect(isSubtitleEditMode("processing")).toBe(false);
  });
});
