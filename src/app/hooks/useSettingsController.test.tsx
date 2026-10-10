import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { useSettingsController } from "./useSettingsController";
import type { AppLogHealth } from "../api/system";
import type { SavedSettings } from "../../features/media/types";
import { createDefaultProfiles } from "../../features/media/llmProfiles";

const mockGetAppLogHealth = vi.hoisted(() => vi.fn());

vi.mock("../api/system", () => ({
  getAppLogHealth: mockGetAppLogHealth,
  openAppLogDir: vi.fn(),
}));

const DISABLED_HEALTH: AppLogHealth = {
  path: "D:\\voxtrans\\output\\logs\\app.log",
  directory: "D:\\voxtrans\\output\\logs",
  enabled: false,
  disabledReason: "disk full",
  fileOpen: false,
};

function baseSettings(): SavedSettings {
  return {
    provider: "cpu",
    chunkTargetSeconds: 60,
    subtitleLengthPreset: "standard",
    asrModel: "Qwen3-ASR-0.6B",
    alignModel: "mms-300m-1130-forced-aligner",
    demucsModel: "htdemucs_ft",
    enableVocalSeparation: false,
    translateApiKey: "",
    translateBaseUrl: "https://api.deepseek.com/v1",
    translateModel: "deepseek-v4-flash",
    llmProfiles: createDefaultProfiles(),
    activeLlmProfileId: "deepseek",
    llmConcurrency: 4,
    terminologyGroups: [{ id: "group-test", name: "默认", terms: [] }],
    activeTerminologyGroupId: "",
    enableSubtitleBeautify: true,
    enableClickSound: true,
    autoBurnHardSubtitle: false,
    defaultReviewSource: false,
    defaultReviewTarget: false,
    subtitleBurnMode: "bilingualSourceFirst",
    subtitleRenderStyle: {
      source: {
        fontFamily: "Arial",
        fontSize: 44,
        primaryColor: "#FFFFFF",
        outlineColor: "#101010",
        backColor: "#000000",
        outline: 2.5,
        shadow: 1,
        borderStyle: "outline",
        borderOpacity: 88,
      },
      target: {
        fontFamily: "Microsoft YaHei",
        fontSize: 40,
        primaryColor: "#EAF6FF",
        outlineColor: "#101010",
        backColor: "#000000",
        outline: 2.5,
        shadow: 1,
        borderStyle: "outline",
        borderOpacity: 88,
      },
      layout: {
        marginV: 40,
        alignment: 2,
        bilingualLineGap: 10,
      },
    },
    flatSrtOutput: false,
    flatSrtItems: ["source", "target"],
    locale: "zh-CN",
    modelsDir: null,
  };
}

function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return function Wrapper({ children }: { children: ReactNode }) {
    return (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
  };
}

function setup() {
  const dispatch = vi.fn();
  const pushToast = vi.fn();
  const refreshModelStatus = vi.fn().mockResolvedValue(undefined);
  const view = renderHook(
    () =>
      useSettingsController({
        settings: baseSettings(),
        dispatch,
        pushToast,
        refreshModelStatus,
      }),
    { wrapper: createWrapper() },
  );
  return { ...view, dispatch, pushToast, refreshModelStatus };
}

beforeEach(() => {
  mockGetAppLogHealth.mockReset();
});

describe("useSettingsController app log health", () => {
  it("probes health once per settings open and exposes it", async () => {
    mockGetAppLogHealth.mockResolvedValue(DISABLED_HEALTH);
    const { result, pushToast } = setup();

    // Unknown until the dialog is opened for the first time.
    expect(result.current.appLogHealth).toBeNull();

    act(() => {
      result.current.openSettings();
    });

    await waitFor(() =>
      expect(result.current.appLogHealth).toEqual(DISABLED_HEALTH),
    );
    expect(mockGetAppLogHealth).toHaveBeenCalledTimes(1);
    // Diagnostics only — never a toast.
    expect(pushToast).not.toHaveBeenCalled();
  });

  it("degrades to unknown silently when the probe fails", async () => {
    const warnSpy = vi.spyOn(console, "warn").mockImplementation(() => {});
    mockGetAppLogHealth.mockRejectedValue(new Error("command not found"));
    const { result, pushToast } = setup();

    act(() => {
      result.current.openSettings();
    });

    await waitFor(() => expect(warnSpy).toHaveBeenCalled());
    expect(result.current.appLogHealth).toBeNull();
    expect(pushToast).not.toHaveBeenCalled();
    warnSpy.mockRestore();
  });
});
