import { describe, expect, it } from "vitest";
import { toUserErrorMessage } from "./errors";

describe("toUserErrorMessage", () => {
  it("returns string errors directly", () => {
    expect(toUserErrorMessage("Something went wrong")).toBe("Something went wrong");
  });

  it("returns fallback for empty string", () => {
    expect(toUserErrorMessage("")).toBe("操作失败，请稍后重试");
    expect(toUserErrorMessage("   ")).toBe("操作失败，请稍后重试");
  });

  it("extracts message from Error objects", () => {
    expect(toUserErrorMessage(new Error("Network error"))).toBe("Network error");
  });

  it("returns fallback for Error with empty message", () => {
    expect(toUserErrorMessage(new Error(""))).toBe("操作失败，请稍后重试");
  });

  it("extracts message from object with message property", () => {
    expect(toUserErrorMessage({ message: "Invalid input" })).toBe("Invalid input");
  });

  it("returns fallback for object without message", () => {
    expect(toUserErrorMessage({ code: 500 })).toBe("操作失败，请稍后重试");
  });

  it("returns fallback for null and undefined", () => {
    expect(toUserErrorMessage(null)).toBe("操作失败，请稍后重试");
    expect(toUserErrorMessage(undefined)).toBe("操作失败，请稍后重试");
  });

  it("uses custom fallback when provided", () => {
    expect(toUserErrorMessage("", "Custom fallback")).toBe("Custom fallback");
    expect(toUserErrorMessage(null, "Try again")).toBe("Try again");
  });

  it("trims whitespace from string errors", () => {
    expect(toUserErrorMessage("  error  ")).toBe("error");
  });

  it("classifies known backend errors", () => {
    expect(toUserErrorMessage("task not found: abc123")).toBe("任务不存在，请刷新任务列表");
    expect(toUserErrorMessage("task is processing or queued")).toBe("任务正在处理中，请稍后再试");
    expect(toUserErrorMessage("workspace store lock poisoned")).toBe("内部状态错误，请重启应用");
    expect(toUserErrorMessage("invalid request: missing field")).toBe("请求参数无效");
    expect(toUserErrorMessage("IO error: permission denied")).toBe("文件读写失败，请检查磁盘空间");
    expect(toUserErrorMessage("serialization error: invalid JSON")).toBe("数据解析失败");
  });

  it("classifies LLM and network errors", () => {
    expect(toUserErrorMessage("llm call failed after 3 attempts")).toBe("AI 调用失败，请检查网络连接和 API 配置");
    expect(toUserErrorMessage("failed to create http client: timeout")).toBe("网络配置错误，请检查 API 设置");
    expect(toUserErrorMessage("Request timed out")).toBe("请求超时，请检查网络连接");
  });

  it("classifies configuration errors", () => {
    expect(toUserErrorMessage("translateApiKey is required")).toBe("翻译 API Key 未设置，请在设置中配置");
    expect(toUserErrorMessage("translateBaseUrl is required")).toBe("翻译 API 地址未设置，请在设置中配置");
    expect(toUserErrorMessage("translateModel is required")).toBe("翻译模型未设置，请在设置中配置");
  });

  it("classifies cancellation", () => {
    expect(toUserErrorMessage("operation cancelled")).toBe("操作已取消");
    expect(toUserErrorMessage("下载已取消")).toBe("操作已取消");
  });

  it("preserves unknown errors as-is", () => {
    expect(toUserErrorMessage("some random error")).toBe("some random error");
  });
});
