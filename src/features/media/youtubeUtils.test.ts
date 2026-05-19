import { describe, expect, it } from "vitest";
import {
  clampPercent,
  createYoutubePlaceholderTask,
  decodeYoutubeUrlFromPath,
  encodeYoutubePlaceholderPath,
  isCancelledMessage,
  isYoutubePlaceholderPath,
  normalizeTitle,
  parseSizeToBytes,
} from "./youtubeUtils";

describe("encodeYoutubePlaceholderPath", () => {
  it("encodes taskId and url", () => {
    const path = encodeYoutubePlaceholderPath("abc", "https://youtube.com/watch?v=123");
    expect(path).toBe("youtube://pending/abc?url=https%3A%2F%2Fyoutube.com%2Fwatch%3Fv%3D123");
  });
});

describe("decodeYoutubeUrlFromPath", () => {
  it("decodes url from placeholder path", () => {
    const path = "youtube://pending/abc?url=https%3A%2F%2Fyoutube.com%2Fwatch%3Fv%3D123";
    expect(decodeYoutubeUrlFromPath(path)).toBe("https://youtube.com/watch?v=123");
  });

  it("returns empty for non-placeholder path", () => {
    expect(decodeYoutubeUrlFromPath("/some/path")).toBe("");
  });

  it("returns empty for missing query", () => {
    expect(decodeYoutubeUrlFromPath("youtube://pending/abc")).toBe("");
  });
});

describe("isYoutubePlaceholderPath", () => {
  it("matches placeholder path", () => {
    expect(isYoutubePlaceholderPath("youtube://pending/abc?url=x")).toBe(true);
  });

  it("rejects regular path", () => {
    expect(isYoutubePlaceholderPath("/downloads/video.mp4")).toBe(false);
  });
});

describe("normalizeTitle", () => {
  it("extracts basename from path", () => {
    expect(normalizeTitle("/path/to/video.mp4")).toBe("video");
  });

  it("removes yt-dlp format suffix", () => {
    expect(normalizeTitle("video.f140")).toBe("video");
  });

  it("handles empty input", () => {
    expect(normalizeTitle("")).toBe("");
    expect(normalizeTitle("   ")).toBe("");
  });

  it("handles forward slashes", () => {
    expect(normalizeTitle("folder\\video.mkv")).toBe("video");
  });
});

describe("isCancelledMessage", () => {
  it("detects cancel keywords", () => {
    expect(isCancelledMessage("操作已取消")).toBe(true);
    expect(isCancelledMessage("operation cancelled")).toBe(true);
    expect(isCancelledMessage("CANCELLED")).toBe(true);
  });

  it("returns false for unrelated messages", () => {
    expect(isCancelledMessage("success")).toBe(false);
  });
});

describe("clampPercent", () => {
  it("clamps to 0-100 range", () => {
    expect(clampPercent(-10)).toBe(0);
    expect(clampPercent(0)).toBe(0);
    expect(clampPercent(50)).toBe(50);
    expect(clampPercent(100)).toBe(100);
    expect(clampPercent(150)).toBe(100);
  });

  it("returns 0 for non-finite", () => {
    expect(clampPercent(NaN)).toBe(0);
    expect(clampPercent(Infinity)).toBe(0);
  });

  it("rounds to integer", () => {
    expect(clampPercent(33.7)).toBe(34);
  });
});

describe("parseSizeToBytes", () => {
  it("parses decimal units", () => {
    expect(parseSizeToBytes("1 B")).toBe(1);
    expect(parseSizeToBytes("2 KB")).toBe(2000);
    expect(parseSizeToBytes("3 MB")).toBe(3_000_000);
    expect(parseSizeToBytes("1.5 GB")).toBe(1_500_000_000);
  });

  it("parses binary units", () => {
    expect(parseSizeToBytes("1 KiB")).toBe(1024);
    expect(parseSizeToBytes("2 MiB")).toBe(2_097_152);
  });

  it("returns 0 for invalid input", () => {
    expect(parseSizeToBytes("")).toBe(0);
    expect(parseSizeToBytes("abc")).toBe(0);
    expect(parseSizeToBytes("10 XB")).toBe(0);
  });
});

describe("createYoutubePlaceholderTask", () => {
  it("creates placeholder with correct fields", () => {
    const task = createYoutubePlaceholderTask("t1", "youtube://pending/t1?url=x", "My Video", 1024, 50);
    expect(task.id).toBe("t1");
    expect(task.name).toBe("My Video");
    expect(task.mediaKind).toBe("video");
    expect(task.sizeBytes).toBe(1024);
    expect(task.transcribeStatus).toBe("processing");
    expect(task.taskProgress.stage.code).toBe("downloading");
    expect(task.taskProgress.stage.detail).toBe("50%");
    expect(task.taskProgress.stage.current).toBe(50);
    expect(task.taskProgress.stage.total).toBe(100);
  });
});
