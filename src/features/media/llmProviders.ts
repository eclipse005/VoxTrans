/**
 * Translation LLM vendor presets (quick-fill).
 *
 * Adding a provider:
 * 1. Append one entry here
 * 2. Drop `/public/icons/providers/{id}.svg` if it has a brand icon
 * 3. Optionally add hint i18n under settings:translate.providerHints.*
 *
 * Do NOT touch SettingsModal field lists, save/load, or the Rust pipeline.
 * Multi-profile ensure/fill is handled by `llmProfiles.ts` + backend normalize.
 */

export type LlmProviderId =
  | "custom"
  | "deepseek"
  | "qwen"
  | "doubao"
  | "chatgpt"
  | "gemini"
  | "openrouter"
  | "ollama";

export type LlmProviderBadgeTone = "free" | "recommend";

export type LlmProviderPreset = {
  id: LlmProviderId;
  name: string;
  shortName: string;
  baseURL: string;
  model: string;
  badge?: string;
  badgeTone?: LlmProviderBadgeTone;
  /** Platform URL for obtaining an API key */
  keyUrl?: string;
  /** When false (Ollama), empty key is allowed */
  requiresKey?: boolean;
  /** Short secondary line under the selected card */
  hint?: string;
  /** Path under public/, e.g. /icons/providers/deepseek.svg */
  iconSrc?: string;
  /** Monochrome/currentColor icons need invert on dark panels */
  iconMono?: boolean;
};

/**
 * Subtitle translation uses Flash/Mini tiers — not Pro/Max.
 * Model ids drift with vendors; users can override or "Fetch models".
 */
export const LLM_PROVIDER_PRESETS: LlmProviderPreset[] = [
  {
    id: "custom",
    name: "自定义",
    shortName: "自定义",
    baseURL: "",
    model: "",
    hint: "手填任意 OpenAI 兼容接口",
  },
  {
    id: "deepseek",
    name: "DeepSeek",
    shortName: "DeepSeek",
    baseURL: "https://api.deepseek.com/v1",
    model: "deepseek-v4-flash",
    badge: "推荐",
    badgeTone: "recommend",
    keyUrl: "https://platform.deepseek.com/",
    hint: "V4 Flash · 翻译够用",
    iconSrc: "/icons/providers/deepseek.svg",
  },
  {
    id: "qwen",
    name: "通义千问",
    shortName: "通义",
    baseURL: "https://dashscope.aliyuncs.com/compatible-mode/v1",
    model: "qwen3.6-flash",
    keyUrl: "https://dashscope.console.aliyun.com/",
    hint: "3.6 Flash · 低成本",
    iconSrc: "/icons/providers/qwen.svg",
  },
  {
    id: "doubao",
    name: "豆包",
    shortName: "豆包",
    baseURL: "https://ark.cn-beijing.volces.com/api/v3",
    // Seed 2.1 Turbo：当前代高频/低成本档（非 Pro）；方舟也可填接入点 ID
    model: "doubao-seed-2-1-turbo-260628",
    keyUrl: "https://console.volcengine.com/ark",
    hint: "2.1 Turbo · 可改成接入点 ID",
    iconSrc: "/icons/providers/doubao.svg",
  },
  {
    id: "chatgpt",
    name: "OpenAI",
    shortName: "OpenAI",
    baseURL: "https://api.openai.com/v1",
    model: "gpt-5-mini",
    keyUrl: "https://platform.openai.com/api-keys",
    hint: "GPT-5 mini · 高性价比",
    iconSrc: "/icons/providers/chatgpt.svg",
    iconMono: true,
  },
  {
    id: "gemini",
    name: "Google Gemini",
    shortName: "Gemini",
    baseURL: "https://generativelanguage.googleapis.com/v1beta/openai",
    model: "gemini-3.5-flash",
    keyUrl: "https://aistudio.google.com/apikey",
    hint: "3.5 Flash · OpenAI 兼容",
    iconSrc: "/icons/providers/gemini.svg",
  },
  {
    id: "openrouter",
    name: "OpenRouter",
    shortName: "OpenRouter",
    baseURL: "https://openrouter.ai/api/v1",
    model: "google/gemini-3.5-flash",
    keyUrl: "https://openrouter.ai/keys",
    hint: "聚合 · 默认 Gemini Flash",
    iconSrc: "/icons/providers/openrouter.svg",
    iconMono: true,
  },
  {
    id: "ollama",
    name: "Ollama",
    shortName: "Ollama",
    baseURL: "http://localhost:11434/v1",
    model: "qwen3:8b",
    keyUrl: "https://ollama.com/",
    requiresKey: false,
    hint: "本地 · 需先 ollama pull · Key 可填 ollama",
    iconSrc: "/icons/providers/ollama.svg",
    iconMono: true,
  },
];

export const DEFAULT_LLM_PROVIDER_ID: LlmProviderId = "deepseek";

export function getProviderById(id: string): LlmProviderPreset {
  return (
    LLM_PROVIDER_PRESETS.find((p) => p.id === id) ??
    LLM_PROVIDER_PRESETS.find((p) => p.id === "custom")!
  );
}

export function isLlmProviderId(id: string): id is LlmProviderId {
  return LLM_PROVIDER_PRESETS.some((p) => p.id === id);
}
