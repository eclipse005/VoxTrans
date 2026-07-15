import { createContext, useContext } from "react";
import type {
  ModelStatusResponse,
  AsrModel,
  LlmProfile,
} from "../../features/media/types";
import type { SettingsForm } from "../hooks/useSettingsController";
import type { LlmProviderId } from "../../features/media/llmProviders";
import type { FetchLlmModelsResult } from "../api/settings";

type SettingsFormContextValue = {
  form: SettingsForm;
  setForm: React.Dispatch<React.SetStateAction<SettingsForm>>;
  asrStatus: ModelStatusResponse | null;
  asrStatusByModel: Record<AsrModel, ModelStatusResponse | null>;
  alignStatus: ModelStatusResponse | null;
  demucsStatus: ModelStatusResponse | null;
  saveSettings: () => void | Promise<void>;
  testTranslateConnection: () => void | Promise<void>;
  fetchLlmModels: () => Promise<FetchLlmModelsResult>;
  selectLlmProvider: (id: LlmProviderId) => void;
  updateActiveLlmProfile: (
    patch: Partial<Pick<LlmProfile, "baseUrl" | "apiKey" | "model" | "name" | "requiresKey">>,
  ) => void;
  /** Restore active provider's catalog baseUrl/model (keeps key). Persist on Save. */
  resetActiveLlmProfile: () => void;
  openModelDir: (target: "asr" | "align" | "demucs", model?: string) => void | Promise<void>;
  startModelDownload: (target: "asr" | "align" | "demucs", model?: string) => void | Promise<void>;
  cancelModelDownload: (target: "asr" | "align" | "demucs", model?: string) => void | Promise<void>;
  refreshModelStatus: () => Promise<void>;
};

export const SettingsFormContext = createContext<SettingsFormContextValue | null>(null);

export function useSettingsFormContext(): SettingsFormContextValue {
  const ctx = useContext(SettingsFormContext);
  if (!ctx) {
    throw new Error("useSettingsFormContext must be used within SettingsFormProvider");
  }
  return ctx;
}
