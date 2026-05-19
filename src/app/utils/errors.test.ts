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
});
