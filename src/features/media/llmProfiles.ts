/**
 * Multi-profile LLM archives: one free-form slot per vendor preset.
 *
 * Mental model (Egg-style):
 * - 8 independent config slots (deepseek / qwen / … / custom)
 * - Presets only supply default name / baseUrl / model / requiresKey
 * - Users may freely edit URL and model on any slot
 * - "Reset" restores catalog defaults for the **current** slot only (keeps API key)
 */

import type { LlmProfile } from "../../generated/bindings/LlmProfile";
import type { SavedSettings } from "../../generated/bindings/SavedSettings";
import {
  DEFAULT_LLM_PROVIDER_ID,
  LLM_PROVIDER_PRESETS,
  getProviderById,
  type LlmProviderId,
  type LlmProviderPreset,
} from "./llmProviders";

export function createProfileFromPreset(preset: LlmProviderPreset, apiKey = ""): LlmProfile {
  return {
    id: preset.id,
    name: preset.name,
    baseUrl: preset.baseURL,
    model: preset.model,
    apiKey,
    presetId: preset.id,
    requiresKey: preset.requiresKey ?? true,
  };
}

export function createDefaultProfiles(): LlmProfile[] {
  return LLM_PROVIDER_PRESETS.map((p) => createProfileFromPreset(p));
}

export function getActiveProfile(
  profiles: LlmProfile[],
  activeId: string,
): LlmProfile {
  if (!profiles.length) {
    return createProfileFromPreset(getProviderById(DEFAULT_LLM_PROVIDER_ID));
  }
  return profiles.find((p) => p.id === activeId) ?? profiles[0];
}

export function effectiveApiKey(profile: LlmProfile): string {
  const key = profile.apiKey?.trim() ?? "";
  if (!key && profile.requiresKey === false) return "ollama";
  return key;
}

/** "Has usable credentials" for this slot (keyless providers count as ready). */
export function isProfileConfigured(profile: LlmProfile): boolean {
  if (profile.requiresKey === false) return true;
  return (profile.apiKey?.trim().length ?? 0) > 0;
}

export function updateActiveProfile(
  profiles: LlmProfile[],
  activeId: string,
  patch: Partial<Pick<LlmProfile, "name" | "baseUrl" | "apiKey" | "model" | "requiresKey">>,
): LlmProfile[] {
  return profiles.map((p) => (p.id === activeId ? { ...p, ...patch } : p));
}

/**
 * Restore catalog defaults for one slot only. Keeps the API key.
 * Custom slot → empty URL/model; known vendors → preset URL/model/name/requiresKey.
 */
export function resetProfileToPreset(
  profiles: LlmProfile[],
  profileId: string,
): LlmProfile[] {
  const preset = getProviderById(profileId);
  return profiles.map((p) => {
    if (p.id !== profileId) return p;
    return createProfileFromPreset(preset, p.apiKey ?? "");
  });
}

/** True when baseUrl + model already match the vendor catalog defaults. */
export function isProfileAtPresetDefaults(profile: LlmProfile): boolean {
  const preset = getProviderById(profile.id);
  return (
    (profile.baseUrl ?? "").trim() === (preset.baseURL ?? "").trim() &&
    (profile.model ?? "").trim() === (preset.model ?? "").trim()
  );
}

/**
 * Switch active vendor without wiping other slots' keys.
 * Missing slot is created from the preset catalog.
 */
export function selectProvider(
  profiles: LlmProfile[],
  providerId: LlmProviderId,
): { profiles: LlmProfile[]; activeLlmProfileId: string } {
  let next = profiles;
  if (!next.some((p) => p.id === providerId)) {
    next = [...next, createProfileFromPreset(getProviderById(providerId))];
  }
  return { profiles: next, activeLlmProfileId: providerId };
}

/**
 * Ensure every catalog preset has a slot; repair active id; optionally seed
 * legacy single-slot translate_* into the matching profile.
 *
 * Free-form contract: never overwrite a non-empty user model/URL with catalog
 * defaults. Only fill empty model; only migrate obsolete defaults when
 * building slots from an empty archive (one-shot upgrade path).
 */
export function ensureProfiles(
  profiles: LlmProfile[] | null | undefined,
  activeId: string | null | undefined,
  legacy?: { apiKey?: string; baseUrl?: string; model?: string },
): { profiles: LlmProfile[]; activeLlmProfileId: string } {
  const wasEmpty = !profiles?.length;
  let next = wasEmpty ? createDefaultProfiles() : profiles.map((p) => ({ ...p }));
  let seededActiveId: string | null = null;

  if (wasEmpty && legacy) {
    seededActiveId = seedLegacy(next, legacy);
    // One-shot: if legacy model was an old catalog default, bump to current preset.
    if (seededActiveId) {
      const slot = next.find((p) => p.id === seededActiveId);
      if (slot && isObsoleteCatalogModel(slot.id, slot.model)) {
        slot.model = getProviderById(slot.id).model;
      }
    }
  }

  for (const preset of LLM_PROVIDER_PRESETS) {
    const idx = next.findIndex((p) => p.id === preset.id);
    if (idx === -1) {
      next.push(createProfileFromPreset(preset));
    } else {
      const cur = next[idx];
      const model = (cur.model ?? "").trim();
      next[idx] = {
        ...cur,
        id: cur.id.trim() || preset.id,
        name: cur.name?.trim() || preset.name,
        baseUrl: (cur.baseUrl ?? "").trim(),
        apiKey: (cur.apiKey ?? "").trim(),
        // Only fill empty model — free-form slots must keep user choices.
        model: model || preset.model,
        presetId: (cur.presetId ?? "").trim() || preset.id,
        requiresKey:
          preset.id === "custom"
            ? (cur.requiresKey ?? true)
            : (preset.requiresKey ?? true),
      };
    }
  }

  const active = (activeId ?? "").trim();
  let activeLlmProfileId: string;
  if (seededActiveId && next.some((p) => p.id === seededActiveId)) {
    // Legacy migration: land on the slot that received the old key/url.
    activeLlmProfileId = seededActiveId;
  } else if (active && next.some((p) => p.id === active)) {
    activeLlmProfileId = active;
  } else if (next.some((p) => p.id === DEFAULT_LLM_PROVIDER_ID)) {
    activeLlmProfileId = DEFAULT_LLM_PROVIDER_ID;
  } else {
    activeLlmProfileId = next[0]?.id ?? DEFAULT_LLM_PROVIDER_ID;
  }

  return { profiles: next, activeLlmProfileId };
}

/** Models we briefly shipped as catalog defaults before aligning with Egg. */
const OBSOLETE_CATALOG_MODELS: Partial<Record<string, readonly string[]>> = {
  deepseek: ["deepseek-chat"],
  qwen: ["qwen-flash"],
  doubao: ["doubao-seed-1-6-flash-250828"],
  chatgpt: ["gpt-4.1-mini"],
  gemini: ["gemini-2.5-flash"],
  openrouter: ["google/gemini-2.5-flash"],
  ollama: ["qwen2.5:7b"],
};

function isObsoleteCatalogModel(presetId: string, model: string): boolean {
  return (OBSOLETE_CATALOG_MODELS[presetId] ?? []).includes(model.trim());
}

/** Normalize endpoint for matching: trim, strip trailing slashes, lower-case host. */
export function normalizeEndpointUrl(url: string): string {
  const trimmed = url.trim().replace(/\/+$/, "");
  if (!trimmed) return "";
  try {
    const u = new URL(trimmed);
    u.hostname = u.hostname.toLowerCase();
    // Drop default ports noise; keep path as-is (minus trailing slash already).
    return `${u.protocol}//${u.host}${u.pathname === "/" ? "" : u.pathname}`.replace(/\/+$/, "");
  } catch {
    return trimmed.toLowerCase();
  }
}

/**
 * Seed legacy single-slot translate_* into the best matching profile.
 * Returns the target profile id when anything was written, else null.
 */
function seedLegacy(
  profiles: LlmProfile[],
  legacy: { apiKey?: string; baseUrl?: string; model?: string },
): string | null {
  const key = legacy.apiKey?.trim() ?? "";
  const base = legacy.baseUrl?.trim() ?? "";
  const model = legacy.model?.trim() ?? "";
  if (!key && !base && !model) return null;

  const baseNorm = normalizeEndpointUrl(base);
  const targetId =
    profiles.find((p) => baseNorm && normalizeEndpointUrl(p.baseUrl) === baseNorm)?.id ??
    (baseNorm.includes("deepseek")
      ? "deepseek"
      : base
        ? "custom"
        : DEFAULT_LLM_PROVIDER_ID);

  const p = profiles.find((x) => x.id === targetId);
  if (!p) return null;
  if (key) p.apiKey = key;
  if (base) p.baseUrl = base.replace(/\/+$/, "");
  if (model) p.model = model;
  return targetId;
}

/** Flatten active profile into denormalized translate_* fields for the pipeline. */
export function flattenActiveToTranslateFields(
  profiles: LlmProfile[],
  activeId: string,
): Pick<SavedSettings, "translateApiKey" | "translateBaseUrl" | "translateModel"> {
  const active = getActiveProfile(profiles, activeId);
  return {
    translateApiKey: effectiveApiKey(active),
    translateBaseUrl: active.baseUrl.trim(),
    translateModel: active.model.trim(),
  };
}

export function applyProfilesToSettings(
  settings: SavedSettings,
  profiles: LlmProfile[],
  activeLlmProfileId: string,
): SavedSettings {
  const ensured = ensureProfiles(profiles, activeLlmProfileId);
  const flat = flattenActiveToTranslateFields(ensured.profiles, ensured.activeLlmProfileId);
  return {
    ...settings,
    llmProfiles: ensured.profiles,
    activeLlmProfileId: ensured.activeLlmProfileId,
    ...flat,
  };
}
