import { describe, expect, it, vi } from "vitest";
import {
  createTerminologyGroup,
  createTerminologyTerm,
  normalizeTerminologyGroups,
  parseBatchTerminologyInput,
  DEFAULT_TERMINOLOGY_GROUP_NAME,
} from "./terminology";

describe("parseBatchTerminologyInput", () => {
  it("parses simple origin:target pairs", () => {
    const result = parseBatchTerminologyInput("Hello:你好\nWorld:世界");
    expect(result.terms).toHaveLength(2);
    expect(result.terms[0].origin).toBe("Hello");
    expect(result.terms[0].target).toBe("你好");
    expect(result.terms[1].origin).toBe("World");
    expect(result.terms[1].target).toBe("世界");
    expect(result.skipped).toBe(0);
  });

  it("parses origin:target:note triples", () => {
    const result = parseBatchTerminologyInput("NATO:北约:军事组织");
    expect(result.terms).toHaveLength(1);
    expect(result.terms[0].origin).toBe("NATO");
    expect(result.terms[0].target).toBe("北约");
    expect(result.terms[0].note).toBe("军事组织");
  });

  it("handles note with extra colons", () => {
    const result = parseBatchTerminologyInput("A:B:C:D");
    expect(result.terms).toHaveLength(1);
    expect(result.terms[0].origin).toBe("A");
    expect(result.terms[0].target).toBe("B");
    expect(result.terms[0].note).toBe("C:D");
  });

  it("skips lines without colon separator", () => {
    const result = parseBatchTerminologyInput("Hello:你好\ninvalid\nWorld:世界");
    expect(result.terms).toHaveLength(2);
    expect(result.skipped).toBe(1);
  });

  it("skips empty origin or target", () => {
    const result = parseBatchTerminologyInput(":你好\nHello:\nWorld:世界");
    expect(result.terms).toHaveLength(1);
    expect(result.terms[0].origin).toBe("World");
    expect(result.skipped).toBe(2);
  });

  it("trims whitespace around parts", () => {
    const result = parseBatchTerminologyInput("  Hello  :  你好  :  note  ");
    expect(result.terms[0].origin).toBe("Hello");
    expect(result.terms[0].target).toBe("你好");
    expect(result.terms[0].note).toBe("note");
  });

  it("handles comma-separated input", () => {
    const result = parseBatchTerminologyInput("Hello:你好,World:世界");
    expect(result.terms).toHaveLength(2);
  });

  it("ignores empty lines", () => {
    const result = parseBatchTerminologyInput("Hello:你好\n\n\nWorld:世界");
    expect(result.terms).toHaveLength(2);
    expect(result.skipped).toBe(0);
  });
});

describe("normalizeTerminologyGroups", () => {
  it("returns provided groups when non-empty", () => {
    const groups = [createTerminologyGroup("Custom")];
    const result = normalizeTerminologyGroups(groups);
    expect(result).toHaveLength(1);
    expect(result[0].name).toBe("Custom");
  });

  it("creates default group when empty", () => {
    const result = normalizeTerminologyGroups([]);
    expect(result).toHaveLength(1);
    expect(result[0].name).toBe(DEFAULT_TERMINOLOGY_GROUP_NAME);
  });
});

describe("createTerminologyGroup", () => {
  it("uses provided name", () => {
    vi.spyOn(Date, "now").mockReturnValue(123456789);
    vi.spyOn(Math, "random").mockReturnValue(0.123456);
    const group = createTerminologyGroup("My Group");
    expect(group.name).toBe("My Group");
    expect(group.terms).toEqual([]);
    expect(group.id).toMatch(/^group-\d+-[a-z0-9]+$/);
    vi.restoreAllMocks();
  });

  it("falls back to default name", () => {
    vi.spyOn(Date, "now").mockReturnValue(123456789);
    vi.spyOn(Math, "random").mockReturnValue(0.123456);
    const group = createTerminologyGroup();
    expect(group.name).toBe(DEFAULT_TERMINOLOGY_GROUP_NAME);
    vi.restoreAllMocks();
  });

  it("trims whitespace from name", () => {
    vi.spyOn(Date, "now").mockReturnValue(123456789);
    vi.spyOn(Math, "random").mockReturnValue(0.123456);
    const group = createTerminologyGroup("  My Group  ");
    expect(group.name).toBe("My Group");
    vi.restoreAllMocks();
  });

  it("falls back to default for empty name", () => {
    vi.spyOn(Date, "now").mockReturnValue(123456789);
    vi.spyOn(Math, "random").mockReturnValue(0.123456);
    const group = createTerminologyGroup("   ");
    expect(group.name).toBe(DEFAULT_TERMINOLOGY_GROUP_NAME);
    vi.restoreAllMocks();
  });
});

describe("createTerminologyTerm", () => {
  it("creates term with origin and target", () => {
    vi.spyOn(Date, "now").mockReturnValue(123456789);
    vi.spyOn(Math, "random").mockReturnValue(0.123456);
    const term = createTerminologyTerm("Hello", "你好");
    expect(term.origin).toBe("Hello");
    expect(term.target).toBe("你好");
    expect(term.note).toBe("");
    vi.restoreAllMocks();
  });

  it("includes note when provided", () => {
    vi.spyOn(Date, "now").mockReturnValue(123456789);
    vi.spyOn(Math, "random").mockReturnValue(0.123456);
    const term = createTerminologyTerm("NATO", "北约", "军事联盟");
    expect(term.note).toBe("军事联盟");
    vi.restoreAllMocks();
  });

  it("trims whitespace", () => {
    vi.spyOn(Date, "now").mockReturnValue(123456789);
    vi.spyOn(Math, "random").mockReturnValue(0.123456);
    const term = createTerminologyTerm("  Hello  ", "  你好  ", "  note  ");
    expect(term.origin).toBe("Hello");
    expect(term.target).toBe("你好");
    expect(term.note).toBe("note");
    vi.restoreAllMocks();
  });
});
