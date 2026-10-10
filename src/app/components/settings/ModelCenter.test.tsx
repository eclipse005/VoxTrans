import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import { ModelCenter } from "./ModelCenter";
import { SettingsFormContext } from "../../contexts/SettingsFormContext";
import type { SettingsForm } from "../../hooks/useSettingsController";
import type { AppLogHealth } from "../../api/system";
import type {
  AlignModel,
  AsrModel,
  ModelStatusResponse,
} from "../../../features/media/types";

const DISABLED_HEALTH: AppLogHealth = {
  path: "D:\\voxtrans\\output\\logs\\app.log",
  directory: "D:\\voxtrans\\output\\logs",
  enabled: false,
  disabledReason: "disk full",
  fileOpen: false,
};

const HEALTHY: AppLogHealth = {
  path: "D:\\voxtrans\\output\\logs\\app.log",
  directory: "D:\\voxtrans\\output\\logs",
  enabled: true,
  disabledReason: null,
  fileOpen: true,
};

/** Only the fields ModelCenter reads — the rest of the form is irrelevant here. */
function formWithHealth(appLogHealth: AppLogHealth | null): SettingsForm {
  const partial: Partial<SettingsForm> = {
    asrModel: "Qwen3-ASR-0.6B" as AsrModel,
    alignModel: "mms-300m-1130-forced-aligner" as AlignModel,
    appLogHealth,
  };
  return partial as SettingsForm;
}

function renderModelCenter(appLogHealth: AppLogHealth | null) {
  const ctxValue = {
    form: formWithHealth(appLogHealth),
    setForm: vi.fn(),
    asrStatus: null as ModelStatusResponse | null,
    asrStatusByModel: {} as Record<AsrModel, ModelStatusResponse | null>,
    alignStatus: null as ModelStatusResponse | null,
    alignStatusByModel: {} as Record<AlignModel, ModelStatusResponse | null>,
    demucsStatus: null as ModelStatusResponse | null,
    saveSettings: vi.fn(),
    testTranslateConnection: vi.fn(),
    fetchLlmModels: vi.fn(),
    selectLlmProvider: vi.fn(),
    updateActiveLlmProfile: vi.fn(),
    resetActiveLlmProfile: vi.fn(),
    openModelDir: vi.fn(),
    startModelDownload: vi.fn(),
    cancelModelDownload: vi.fn(),
    refreshModelStatus: vi.fn(),
    openAppLogDir: vi.fn(),
  };
  return render(
    <SettingsFormContext.Provider value={ctxValue}>
      <ModelCenter
        modelsDir=""
        storageDefaultLabel="默认存储目录"
        onPickModelsDir={() => {}}
        onResetModelsDir={() => {}}
      />
    </SettingsFormContext.Provider>,
  );
}

describe("ModelCenter app log section health warning", () => {
  it("shows the unwritable warning with the disabled reason", () => {
    renderModelCenter(DISABLED_HEALTH);
    expect(screen.getByText("日志当前不可写：disk full")).toBeTruthy();
    // The normal hint line is still there alongside the warning.
    expect(
      screen.getByText("程序运行出错的记录，反馈问题时发给开发者"),
    ).toBeTruthy();
  });

  it("falls back to the reason-less warning when no reason is given", () => {
    renderModelCenter({ ...DISABLED_HEALTH, disabledReason: null });
    expect(screen.getByText("日志当前不可写")).toBeTruthy();
  });

  it("stays quiet while logging is enabled", () => {
    renderModelCenter(HEALTHY);
    expect(screen.queryByText(/日志当前不可写/)).toBeNull();
  });

  it("stays quiet while health is unknown (never probed / probe failed)", () => {
    renderModelCenter(null);
    expect(screen.queryByText(/日志当前不可写/)).toBeNull();
  });
});
