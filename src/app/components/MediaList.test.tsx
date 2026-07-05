import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactNode } from "react";
import MediaList from "./MediaList";
import type { QueueItem } from "../../features/media/types";
import type { SourceLanguageOption } from "../../generated/bindings/SourceLanguageOption";

const mockUseSourceLanguages = vi.hoisted(() => vi.fn());

vi.mock("../hooks/useSourceLanguages", () => ({
  useSourceLanguages: (...args: unknown[]) => mockUseSourceLanguages(...args),
}));

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

function testQueueItem(overrides?: Partial<QueueItem>): QueueItem {
  return {
    id: "task-1",
    path: "D:\\media\\demo.mp4",
    name: "demo.mp4",
    mediaKind: "video",
    sizeBytes: 1,
    sourceLang: "en",
    targetLang: "zh-CN",
    transcribeStatus: "pending",
    taskProgress: {
      stage: {
        code: "",
        label: "",
        order: 0,
        detail: "",
        current: 0,
        total: 0,
      },
    },
    transcribeError: "",
    resultText: "",
    resultSrt: "",
    subtitleSegmentsJson: "[]",
    ...overrides,
  };
}

describe("MediaList model-change auto-correct", () => {
  beforeEach(() => {
    mockUseSourceLanguages.mockReturnValue({
      data: [],
      isLoading: false,
      error: null,
    });
  });

  it("falls back an unsupported source language and notifies via toast", async () => {
    const options: SourceLanguageOption[] = [
      { tag: "en", label: "English", short: "EN" },
      { tag: "zh", label: "Chinese", short: "ZH" },
    ];
    mockUseSourceLanguages.mockReturnValue({
      data: options,
      isLoading: false,
      error: null,
    });

    const item = testQueueItem({ sourceLang: "ja", targetLang: "zh-CN" });
    const onUpdateTaskLanguages = vi.fn();
    const pushToast = vi.fn();

    render(
      <MediaList
        queue={[item]}
        queueCount={1}
        workspaceHydrated
        activeId=""
        isProcessing={false}
        asrModel="Qwen3-ASR-0.6B"
        alignModel="Qwen3-ForcedAligner-0.6B"
        pushToast={pushToast}
        onSetActiveId={vi.fn()}
        onProcessQueue={vi.fn()}
        onClearQueue={vi.fn()}
        onProcessSingle={vi.fn()}
        onProcessSingleTranscribeTranslate={vi.fn()}
        onUpdateTaskLanguages={onUpdateTaskLanguages}
        onUpdateAllTaskLanguages={vi.fn()}
        onUpdateTaskTerminology={vi.fn()}
        terminologyGroups={[]}
        onRemoveItem={vi.fn()}
      />,
      { wrapper: createWrapper() },
    );

    await waitFor(() =>
      expect(onUpdateTaskLanguages).toHaveBeenCalledWith(
        item,
        "en",
        "zh-CN",
      ),
    );
    expect(pushToast).toHaveBeenCalledWith(
      expect.stringContaining("当前源语言不再被新模型组合支持"),
      "info",
    );
  });

  it("does not change a source language that is still supported", async () => {
    const options: SourceLanguageOption[] = [
      { tag: "en", label: "English", short: "EN" },
      { tag: "zh", label: "Chinese", short: "ZH" },
    ];
    mockUseSourceLanguages.mockReturnValue({
      data: options,
      isLoading: false,
      error: null,
    });

    const item = testQueueItem({ sourceLang: "zh", targetLang: "en" });
    const onUpdateTaskLanguages = vi.fn();
    const pushToast = vi.fn();

    render(
      <MediaList
        queue={[item]}
        queueCount={1}
        workspaceHydrated
        activeId=""
        isProcessing={false}
        asrModel="Qwen3-ASR-0.6B"
        alignModel="Qwen3-ForcedAligner-0.6B"
        pushToast={pushToast}
        onSetActiveId={vi.fn()}
        onProcessQueue={vi.fn()}
        onClearQueue={vi.fn()}
        onProcessSingle={vi.fn()}
        onProcessSingleTranscribeTranslate={vi.fn()}
        onUpdateTaskLanguages={onUpdateTaskLanguages}
        onUpdateAllTaskLanguages={vi.fn()}
        onUpdateTaskTerminology={vi.fn()}
        terminologyGroups={[]}
        onRemoveItem={vi.fn()}
      />,
      { wrapper: createWrapper() },
    );

    await waitFor(() =>
      expect(mockUseSourceLanguages).toHaveBeenCalled(),
    );
    expect(onUpdateTaskLanguages).not.toHaveBeenCalled();
    expect(pushToast).not.toHaveBeenCalled();
  });
});
