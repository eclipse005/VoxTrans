import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  listLlmModels,
  saveAppSettings as saveAppSettingsApi,
  testTranslateLlmConnection,
  type FetchLlmModelsResult,
} from "../api/settings";
import {
  getAppLogHealth as getAppLogHealthApi,
  openAppLogDir as openAppLogDirApi,
  type AppLogHealth,
} from "../api/system";
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
  defaultReviewSource: boolean;
  defaultReviewTarget: boolean;
  subtitleBurnMode: SubtitleBurnMode;
  subtitleRenderStyle: SubtitleRenderStyle;
  flatSrtOutput: boolean;
  flatSrtItems: SubtitleBurnMode[];
  locale: SavedSettings["locale"];
  modelsDir: string;
  /**
   * Latest `get_app_log_health` snapshot — diagnostics only, never saved.
   * It rides on the form because the settings dialog reads everything through
   * SettingsFormContext; the form is the channel that already reaches it.
   * Refreshed by `openSettings` on every open.
   */
  appLogHealth: AppLogHealth | null;
};

function settingsToForm(settings: SavedSettings, appLogHealth: AppLogHealth | null): SettingsForm {
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
    defaultReviewSource: settings.defaultReviewSource,
    defaultReviewTarget: settings.defaultReviewTarget,
    subtitleBurnMode: settings.subtitleBurnMode,
    subtitleRenderStyle: settings.subtitleRenderStyle,
    flatSrtOutput: settings.flatSrtOutput,
    flatSrtItems: settings.flatSrtItems,
    locale: settings.locale,
    modelsDir: settings.modelsDir ?? "",
    appLogHealth,
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
  const [form, setForm] = useState<SettingsForm>(() => settingsToForm(settings, null));
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

  /**
   * Probe the app-error-log writer health (diagnostics only). Never toasts:
   * a broken logger must not scare the user, so a failed probe just degrades
   * to "unknown" (null) and the log section stays silent.
   */
  const refreshAppLogHealth = useCallback(async () => {
    try {
      const health = await getAppLogHealthApi();
      setForm((prev) => ({ ...prev, appLogHealth: health }));
    } catch (error) {
      console.warn("failed to read app log health:", error);
      setForm((prev) => ({ ...prev, appLogHealth: null }));
    }
  }, []);

  const openSettings = useCallback(() => {
    void refreshModelStatus();
    void refreshAppLogHealth();
    // Keep the previous snapshot (if any) visible while the probe is in
    // flight — avoids the warning line flashing away on every open.
    setForm((prev) => ({ ...settingsToForm(settings, prev.appLogHealth) }));
    dispatch({ type: "set_ui", payload: { showSettings: true } });
  }, [dispatch, refreshModelStatus, refreshAppLogHealth, settings]);

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
      defaultReviewSource: form.defaultReviewSource,
      defaultReviewTarget: form.defaultReviewTarget,
      subtitleBurnMode: form.subtitleBurnMode,
      subtitleRenderStyle: form.subtitleRenderStyle,
      flatSrtOutput: form.flatSrtOutput,
      flatSrtItems: form.flatSrtItems,
      locale: form.locale,
      modelsDir: form.modelsDir.trim() || null,
    };

    const nextSettings = normalizeSettings(draft, settings);

    dispatch({ type: "set_settings", settings: nextSettings });
    setForm((prev) => ({ ...settingsToForm(nextSettings, prev.appLogHealth) }));

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
  }, [form.llmProfiles, form.activeLlmProfileId, pushToast, t]);

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

  const openAppLogDir = useCallback(async () => {
    try {
      await openAppLogDirApi();
    } catch (error) {
      const message = error instanceof Error ? error.message : t("toasts:settings.openLogDirFailed");
      pushToast(message, "error");
    }
  }, [pushToast, t]);

  return {
    openSettings,
    saveSettings,
    saveTerminologyGroups,
    testTranslateConnection,
    openAppLogDir,
    fetchLlmModels,
    selectLlmProvider,
    updateActiveLlmProfile,
    resetActiveLlmProfile,
    prepareTerminologyForm,
    /** Latest app-log health snapshot (`null` = unknown / not probed yet). */
    appLogHealth: form.appLogHealth,
    form,
    setForm,
  };
}
