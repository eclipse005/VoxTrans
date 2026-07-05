import { describe, expect, it, vi, beforeEach } from "vitest";
import { renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { listSourceLanguages } from "../api/language";
import {
  useSourceLanguages,
  useInvalidateSourceLanguages,
} from "./useSourceLanguages";
import type { AsrModel, AlignModel, SourceLanguageOption } from "../../generated/bindings";

const mockInvoke = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/api/core", () => ({
  invoke: mockInvoke,
}));

beforeEach(() => {
  mockInvoke.mockReset().mockResolvedValue([]);
});

function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
      },
    },
  });

  return function Wrapper({ children }: { children: ReactNode }) {
    return (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
  };
}

describe("listSourceLanguages API", () => {
  it("invokes the backend command with the provided models", async () => {
    mockInvoke.mockResolvedValueOnce([]);

    await listSourceLanguages(
      "Qwen3-ASR-0.6B" as AsrModel,
      "Qwen3-ForcedAligner-0.6B" as AlignModel,
    );

    expect(mockInvoke).toHaveBeenCalledWith("list_source_languages", {
      asrModel: "Qwen3-ASR-0.6B",
      alignModel: "Qwen3-ForcedAligner-0.6B",
    });
  });
});

describe("useSourceLanguages", () => {
  it("returns fetched source language options", async () => {
    const options: SourceLanguageOption[] = [
      { tag: "en", label: "English", short: "EN" },
      { tag: "zh", label: "Chinese", short: "ZH" },
    ];
    mockInvoke.mockResolvedValueOnce(options);

    const { result } = renderHook(
      () =>
        useSourceLanguages(
          "Qwen3-ASR-0.6B" as AsrModel,
          "Qwen3-ForcedAligner-0.6B" as AlignModel,
        ),
      { wrapper: createWrapper() },
    );

    await waitFor(() => expect(result.current.isLoading).toBe(false));

    expect(result.current.data).toEqual(options);
    expect(result.current.error).toBeNull();
  });

  it("reflects loading state while fetching", async () => {
    mockInvoke.mockImplementation(
      () => new Promise<SourceLanguageOption[]>((resolve) => setTimeout(resolve, 50)),
    );

    const { result } = renderHook(
      () =>
        useSourceLanguages(
          "Qwen3-ASR-0.6B" as AsrModel,
          "Qwen3-ForcedAligner-0.6B" as AlignModel,
        ),
      { wrapper: createWrapper() },
    );

    expect(result.current.isLoading).toBe(true);
  });
});

describe("useInvalidateSourceLanguages", () => {
  it("returns a callable invalidator function", () => {
    const { result } = renderHook(() => useInvalidateSourceLanguages(), {
      wrapper: createWrapper(),
    });

    expect(typeof result.current).toBe("function");
    expect(() => result.current()).not.toThrow();
  });
});
