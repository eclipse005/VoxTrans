import type { LanguageTag, TargetLanguage } from "./types";

type LanguageOption<T extends string> = {
  id: T;
  short: string;
  label: string;
  promptLabel: string;
};

export const DEFAULT_SOURCE_LANGUAGE: LanguageTag = "en";
export const DEFAULT_TARGET_LANGUAGE: TargetLanguage = "zh-CN";

/**
 * @deprecated Use `useSourceLanguages(asrModel, alignModel)` for model-aware
 * source language options. This static list remains only as a development
 * fallback and for non-model-aware contexts.
 */
export const SOURCE_LANGUAGE_OPTIONS: LanguageOption<LanguageTag>[] = [
  { id: "en", short: "EN", label: "English", promptLabel: "English" },
  { id: "zh", short: "ZH", label: "中文普通话", promptLabel: "Mandarin Chinese" },
  { id: "yue", short: "粤", label: "粤语", promptLabel: "Cantonese" },
  { id: "ja", short: "JA", label: "日本語", promptLabel: "Japanese" },
  { id: "ko", short: "KO", label: "한국어", promptLabel: "Korean" },
  { id: "fr", short: "FR", label: "Français", promptLabel: "French" },
  { id: "de", short: "DE", label: "Deutsch", promptLabel: "German" },
  { id: "it", short: "IT", label: "Italiano", promptLabel: "Italian" },
  { id: "es", short: "ES", label: "Español", promptLabel: "Spanish" },
  { id: "pt", short: "PT", label: "Português", promptLabel: "Portuguese" },
  { id: "ru", short: "RU", label: "Русский", promptLabel: "Russian" },
];

export const TARGET_LANGUAGE_OPTIONS: LanguageOption<TargetLanguage>[] = [
  { id: "zh-CN", short: "ZH", label: "简体中文", promptLabel: "Simplified Chinese" },
  { id: "zh-TW", short: "繁", label: "繁體中文", promptLabel: "Traditional Chinese" },
  { id: "en", short: "EN", label: "English", promptLabel: "English" },
  { id: "ja", short: "JA", label: "日本語", promptLabel: "Japanese" },
  { id: "ko", short: "KO", label: "한국어", promptLabel: "Korean" },
  { id: "fr", short: "FR", label: "Français", promptLabel: "French" },
  { id: "de", short: "DE", label: "Deutsch", promptLabel: "German" },
  { id: "es", short: "ES", label: "Español", promptLabel: "Spanish" },
  { id: "it", short: "IT", label: "Italiano", promptLabel: "Italian" },
  { id: "pt", short: "PT", label: "Português", promptLabel: "Portuguese" },
  { id: "ru", short: "RU", label: "Русский", promptLabel: "Russian" },
  { id: "ar", short: "AR", label: "العربية", promptLabel: "Arabic" },
  { id: "vi", short: "VI", label: "Tiếng Việt", promptLabel: "Vietnamese" },
  { id: "th", short: "TH", label: "ไทย", promptLabel: "Thai" },
  { id: "id", short: "ID", label: "Bahasa Indonesia", promptLabel: "Indonesian" },
  { id: "tr", short: "TR", label: "Türkçe", promptLabel: "Turkish" },
  { id: "nl", short: "NL", label: "Nederlands", promptLabel: "Dutch" },
  { id: "pl", short: "PL", label: "Polski", promptLabel: "Polish" },
];

const SOURCE_LANGUAGE_SET = new Set<string>(SOURCE_LANGUAGE_OPTIONS.map((option) => option.id));
const TARGET_LANGUAGE_SET = new Set<string>(TARGET_LANGUAGE_OPTIONS.map((option) => option.id));

export function normalizeSourceLanguage(value: unknown): LanguageTag {
  if (typeof value !== "string") return DEFAULT_SOURCE_LANGUAGE;
  const normalized = value.trim();
  if (normalized === "") {
    return DEFAULT_SOURCE_LANGUAGE;
  }
  if (SOURCE_LANGUAGE_SET.has(normalized)) {
    return normalized as LanguageTag;
  }
  // Preserve unrecognised values so that persisted invalid languages surface
  // as execution errors instead of being silently migrated to a fallback.
  return normalized as LanguageTag;
}

export function normalizeTargetLanguage(value: unknown): TargetLanguage {
  if (typeof value !== "string") return DEFAULT_TARGET_LANGUAGE;
  const normalized = value.trim();
  if (TARGET_LANGUAGE_SET.has(normalized)) {
    return normalized as TargetLanguage;
  }
  const lower = normalized.toLowerCase();
  if (lower === "zh" || lower === "zh-cn" || lower === "zh-hans" || lower === "simplified chinese") {
    return "zh-CN";
  }
  if (lower === "zh-tw" || lower === "zh-hant" || lower === "traditional chinese") {
    return "zh-TW";
  }
  return DEFAULT_TARGET_LANGUAGE;
}

export function sourceLanguageOption(value: unknown): LanguageOption<LanguageTag> {
  const normalized = normalizeSourceLanguage(value);
  return SOURCE_LANGUAGE_OPTIONS.find((option) => option.id === normalized) ?? SOURCE_LANGUAGE_OPTIONS[0];
}

export function targetLanguageOption(value: unknown): LanguageOption<TargetLanguage> {
  const normalized = normalizeTargetLanguage(value);
  return TARGET_LANGUAGE_OPTIONS.find((option) => option.id === normalized) ?? TARGET_LANGUAGE_OPTIONS[0];
}
