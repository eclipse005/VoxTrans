import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  listLlmModels,
  saveAppSettings as saveAppSettingsApi,
  testTranslateLlmConnection,
  type FetchLlmModelsResult,
} from "../api/settings";
import type {
  AlignModel,
  AsrModel,
  DemucsModel,
  LlmProfile,
  Provider,
  SavedSettings,
  SubtitleBurnMode,
  SubtitleLengthPreset,
  SubtitleRenderStyle,
} from "../../features/media/types";
import type { AppAction } from "../state/appReducer";
import type { ToastTone } from "../types";
import { normalizeTerminologyGroups } from "../utils/terminology";
import { normalizeSettings } from "../utils/normalizeSettings";
import { useInvalidateSourceLanguages } from "./useSourceLanguages";
import { changeAppLanguage } from "../../i18n";
import {
  effectiveApiKey,
  ensureProfiles,
  flattenActiveToTranslateFields,
  getActiveProfile,
  resetProfileToPreset,
  selectProvider,
  updateActiveProfile,
} from "../../features/media/llmProfiles";
import type { LlmProviderId } from "../../features/media/llmProviders";

type DispatchState = (action: AppAction) => void;
type PushToast = (
  message: string,
  tone?: ToastTone,
  options?: { id?: number; sticky?: boolean; durationMs?: number },
) => number;

export type SettingsForm = {
  provider: Provider;
  chunkInput: string;
  subtitleLengthPreset: SubtitleLengthPreset;
  asrModel: AsrModel;
  alignModel: AlignModel;
  demucsModel: DemucsModel;
  enableVocalSeparation: boolean;
  /** Multi-vendor LLM archives (source of truth for keys). */
  llmProfiles: LlmProfile[];
  activeLlmProfileId: string;
  llmConcurrencyInput: string;
  terminologyGroups: SavedSettings["terminologyGroups"];
  activeTerminologyGroupId: string;
  enableSubtitleBeautify: boolean;
  enableClickSound: boolean;
  autoBurnHardSubtitle: boolean;
  subtitleBurnMode: SubtitleBurnMode;
  subtitleRenderStyle: SubtitleRenderStyle;
  flatSrtOutput: boolean;
  flatSrtItems: SubtitleBurnMode[];
  enableVisionAssist: boolean;
  locale: SavedSettings["locale"];
  modelsDir: string;
};

function settingsToForm(settings: SavedSettings): SettingsForm {
  const ensured = ensureProfiles(settings.llmProfiles, settings.activeLlmProfileId, {
    apiKey: settings.translateApiKey,
    baseUrl: settings.translateBaseUrl,
    model: settings.translateModel,
  });
  return {
    provider: settings.provider,
    chunkInput: String(settings.chunkTargetSeconds),
    subtitleLengthPreset: settings.subtitleLengthPreset,
    asrModel: settings.asrModel,
    alignModel: settings.alignModel,
    demucsModel: settings.demucsModel,
    enableVocalSeparation: settings.enableVocalSeparation,
    llmProfiles: ensured.profiles,
    activeLlmProfileId: ensured.activeLlmProfileId,
    llmConcurrencyInput: String(settings.llmConcurrency),
    terminologyGroups: settings.terminologyGroups,
    activeTerminologyGroupId: settings.activeTerminologyGroupId,
    enableSubtitleBeautify: settings.enableSubtitleBeautify,
    enableClickSound: settings.enableClickSound,
    autoBurnHardSubtitle: settings.autoBurnHardSubtitle,
    subtitleBurnMode: settings.subtitleBurnMode,
    subtitleRenderStyle: settings.subtitleRenderStyle,
    flatSrtOutput: settings.flatSrtOutput,
    flatSrtItems: settings.flatSrtItems,
    enableVisionAssist: settings.enableVisionAssist,
    locale: settings.locale,
    modelsDir: settings.modelsDir ?? "",
  };
}

type UseSettingsControllerArgs = {
  settings: SavedSettings;
  dispatch: DispatchState;
  pushToast: PushToast;
  refreshModelStatus: () => Promise<void>;
};

export function useSettingsController({
  settings,
  dispatch,
  pushToast,
  refreshModelStatus,
}: UseSettingsControllerArgs) {
  const [form, setForm] = useState<SettingsForm>(() => settingsToForm(settings));
  const { t } = useTranslation(["toasts", "tasks", "settings"]);
  const invalidateSourceLanguages = useInvalidateSourceLanguages();

  // Keep terminology form fields in sync with the authoritative settings
  // snapshot. Call this before opening the terminology modal so it always
  // reflects the latest DB state, even when opened from outside the settings
  // dialog.
  const prepareTerminologyForm = useCallback(() => {
    setForm((prev) => ({
      ...prev,
      terminologyGroups: settings.terminologyGroups,
      activeTerminologyGroupId: settings.activeTerminologyGroupId,
    }));
  }, [settings.terminologyGroups, settings.activeTerminologyGroupId]);

  const openSettings = useCallback(() => {
    void refreshModelStatus();
    setForm(settingsToForm(settings));
    dispatch({ type: "set_ui", payload: { showSettings: true } });
  }, [dispatch, refreshModelStatus, settings]);

  const selectLlmProvider = useCallback((id: LlmProviderId) => {
    setForm((prev) => {
      const next = selectProvider(prev.llmProfiles, id);
      return {
        ...prev,
        llmProfiles: next.profiles,
        activeLlmProfileId: next.activeLlmProfileId,
      };
    });
  }, []);

  const updateActiveLlmProfile = useCallback(
    (patch: Partial<Pick<LlmProfile, "baseUrl" | "apiKey" | "model" | "name" | "requiresKey">>) => {
      setForm((prev) => ({
        ...prev,
        llmProfiles: updateActiveProfile(prev.llmProfiles, prev.activeLlmProfileId, patch),
      }));
    },
    [],
  );

  /** Restore catalog URL/model for the active slot only; keeps API key. Needs Save. */
  const resetActiveLlmProfile = useCallback(() => {
    setForm((prev) => ({
      ...prev,
      llmProfiles: resetProfileToPreset(prev.llmProfiles, prev.activeLlmProfileId),
    }));
  }, []);

  const saveSettings = useCallback(async () => {
    const parsed = Number.parseInt(form.chunkInput.trim(), 10);
    if (!Number.isFinite(parsed)) {
      pushToast(t("toasts:settings.chunkMustBeNumber"), "error");
      return;
    }
    const parsedConcurrency = Number.parseInt(form.llmConcurrencyInput.trim(), 10);
    if (!Number.isFinite(parsedConcurrency)) {
      pushToast(t("toasts:settings.concurrencyMustBeNumber"), "error");
      return;
    }

    const ensured = ensureProfiles(form.llmProfiles, form.activeLlmProfileId);
    const active = getActiveProfile(ensured.profiles, ensured.activeLlmProfileId);
    // Block cross-vendor mix: denormalized translate_* must equal the active slot.
    if (active.requiresKey !== false && !active.apiKey.trim()) {
      pushToast(t("toasts:settings.apiKeyRequiredFirst"), "error");
      return;
    }
    if (!active.baseUrl.trim()) {
      pushToast(t("toasts:settings.baseUrlRequiredFirst"), "error");
      return;
    }
    if (!active.model.trim()) {
      pushToast(t("toasts:settings.modelRequiredFirst"), "error");
      return;
    }
    const flat = flattenActiveToTranslateFields(ensured.profiles, ensured.activeLlmProfileId);

    const draft: SavedSettings = {
      ...settings,
      provider: form.provider,
      chunkTargetSeconds: parsed,
      subtitleLengthPreset: form.subtitleLengthPreset,
      asrModel: form.asrModel,
      alignModel: form.alignModel,
      demucsModel: form.demucsModel,
      enableVocalSeparation: form.enableVocalSeparation,
      llmProfiles: ensured.profiles,
      activeLlmProfileId: ensured.activeLlmProfileId,
      translateApiKey: flat.translateApiKey,
      translateBaseUrl: flat.translateBaseUrl,
      translateModel: flat.translateModel,
      llmConcurrency: parsedConcurrency,
      terminologyGroups: normalizeTerminologyGroups(form.terminologyGroups),
      activeTerminologyGroupId: form.activeTerminologyGroupId,
      enableSubtitleBeautify: form.enableSubtitleBeautify,
      enableClickSound: form.enableClickSound,
      autoBurnHardSubtitle: form.autoBurnHardSubtitle,
      subtitleBurnMode: form.subtitleBurnMode,
      subtitleRenderStyle: form.subtitleRenderStyle,
      flatSrtOutput: form.flatSrtOutput,
      flatSrtItems: form.flatSrtItems,
      enableVisionAssist: form.enableVisionAssist,
      locale: form.locale,
      modelsDir: form.modelsDir.trim() || null,
    };

    const nextSettings = normalizeSettings(draft, settings);

    dispatch({ type: "set_settings", settings: nextSettings });
    setForm(settingsToForm(nextSettings));

    try {
      await saveAppSettingsApi(nextSettings);
      pushToast(t("toasts:settings.saved"), "success");
      // The saved locale may differ from the active one — apply it so the UI
      // switches language immediately on save (no restart needed).
      if (nextSettings.locale !== settings.locale) {
        void changeAppLanguage(nextSettings.locale);
      }
      if (
        settings.asrModel !== nextSettings.asrModel ||
        settings.alignModel !== nextSettings.alignModel
      ) {
        invalidateSourceLanguages();
      }
      // If the model storage directory changed, refresh model status cards.
      if (settings.modelsDir !== nextSettings.modelsDir) {
        void refreshModelStatus();
      }
    } catch (error) {
      const message = error instanceof Error ? error.message : t("toasts:settings.saveFailed");
      pushToast(message, "error");
    }
  }, [form, settings, dispatch, pushToast, invalidateSourceLanguages, refreshModelStatus, t]);

  const saveTerminologyGroups = useCallback(async (groups: SavedSettings["terminologyGroups"]) => {
    const normalizedGroups = normalizeTerminologyGroups(groups);
    const nextSettings: SavedSettings = {
      ...settings,
      terminologyGroups: normalizedGroups,
      activeTerminologyGroupId: form.activeTerminologyGroupId,
    };
    dispatch({ type: "set_settings", settings: nextSettings });
    setForm((prev) => ({ ...prev, terminologyGroups: normalizedGroups }));
    try {
      await saveAppSettingsApi(nextSettings);
      pushToast(t("toasts:settings.terminologySaved"), "success");
    } catch (error) {
      const message = error instanceof Error ? error.message : t("toasts:settings.terminologySaveFailed");
      pushToast(message, "error");
    }
  }, [settings, form.activeTerminologyGroupId, dispatch, pushToast, t]);

  const testTranslateConnection = useCallback(async () => {
    // Only the current form slot — never fall back to last-saved translate_*.
    const active = getActiveProfile(form.llmProfiles, form.activeLlmProfileId);
    const apiKey = effectiveApiKey(active);
    const baseUrl = active.baseUrl.trim();
    const configuredModel = active.model.trim();
    if (active.requiresKey !== false && !active.apiKey.trim()) {
      pushToast(t("toasts:settings.apiKeyRequiredFirst"), "error");
      return;
    }
    if (!baseUrl) {
      pushToast(t("toasts:settings.baseUrlRequiredFirst"), "error");
      return;
    }
    if (!configuredModel) {
      pushToast(t("toasts:settings.modelRequiredFirst"), "error");
      return;
    }
    const toastId = pushToast(t("toasts:settings.testingLlm"), "info", { sticky: true });
    try {
      const response = await testTranslateLlmConnection({
        apiKey,
        baseUrl,
        model: configuredModel,
        enableVisionAssist: form.enableVisionAssist,
      });
      if (response.ok) {
        const modelName = response.model?.trim() || configuredModel;
        pushToast(t("toasts:settings.testSuccess", { model: modelName }), "success", { id: toastId, durationMs: 2600 });
        return;
      }
      pushToast(t("toasts:settings.testFailed", { message: response.message || t("errors:fallback") }), "error", { id: toastId, durationMs: 3000 });
    } catch (error) {
      const message = error instanceof Error ? error.message : t("toasts:settings.testConnectFailed");
      pushToast(message, "error", { id: toastId, durationMs: 3000 });
    }
  }, [form.llmProfiles, form.activeLlmProfileId, form.enableVisionAssist, pushToast, t]);

  /**
   * Fetch OpenAI-compatible model list for the active profile.
   * Returns a tagged result: discarded mid-flight requests never surface as
   * an empty list (which would wipe the UI for the *current* provider).
   */
  const fetchLlmModels = useCallback(async (): Promise<FetchLlmModelsResult> => {
    const profileId = form.activeLlmProfileId;
    const active = getActiveProfile(form.llmProfiles, profileId);
    const baseUrl = active.baseUrl.trim();
    if (!baseUrl) {
      pushToast(t("toasts:settings.baseUrlRequiredFirst"), "error");
      return { ok: false, reason: "validation" };
    }
    if (active.requiresKey !== false && !active.apiKey.trim()) {
      pushToast(t("toasts:settings.apiKeyRequiredFirst"), "error");
      return { ok: false, reason: "validation" };
    }
    try {
      const { chatModels, excludedModels } = await listLlmModels({
        apiKey: effectiveApiKey(active),
        baseUrl,
      });

      // Drop stale results if the user switched provider while awaiting.
      let discarded = false;
      setForm((prev) => {
        if (prev.activeLlmProfileId !== profileId) {
          discarded = true;
          return prev;
        }
        // Autofill only if this slot is *still* empty at apply time (user may
        // have typed a model while the request was in flight).
        const slot = getActiveProfile(prev.llmProfiles, profileId);
        if (!(slot.model ?? "").trim() && chatModels.length > 0) {
          return {
            ...prev,
            llmProfiles: updateActiveProfile(prev.llmProfiles, profileId, {
              model: chatModels[0].id,
            }),
          };
        }
        return prev;
      });
      if (discarded) return { ok: false, reason: "discarded" };

      if (chatModels.length === 0) {
        pushToast(
          excludedModels.length > 0
            ? t("toasts:settings.fetchModelsEmptyExcluded", { excluded: excludedModels.length })
            : t("toasts:settings.fetchModelsEmpty"),
          "error",
        );
        return { ok: false, reason: "empty" };
      }
      pushToast(
        excludedModels.length > 0
          ? t("toasts:settings.fetchModelsFiltered", {
              count: chatModels.length,
              excluded: excludedModels.length,
            })
          : t("toasts:settings.fetchModelsSuccess", { count: chatModels.length }),
        "success",
      );
      return { ok: true, profileId, models: chatModels };
    } catch (error) {
      const message = error instanceof Error ? error.message : t("errors:fallback");
      pushToast(t("toasts:settings.fetchModelsFailed", { message }), "error");
      return { ok: false, reason: "error" };
    }
  }, [form.llmProfiles, form.activeLlmProfileId, pushToast, t]);

  return {
    openSettings,
    saveSettings,
    saveTerminologyGroups,
    testTranslateConnection,
    fetchLlmModels,
    selectLlmProvider,
    updateActiveLlmProfile,
    resetActiveLlmProfile,
    prepareTerminologyForm,
    form,
    setForm,
  };
}
