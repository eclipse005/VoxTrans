/**
 * Translation LLM vendor presets (quick-fill) — Anthropic Messages protocol.
 *
 * Adding a provider:
 * 1. Append one entry here
 * 2. Drop `/public/icons/providers/{id}.svg` if it has a brand icon
 * 3. Optionally add hint i18n under settings:translate.providerHints.*
 *
 * Do NOT touch SettingsModal field lists, save/load, or the Rust pipeline.
 * Multi-profile ensure/fill is handled by `llmProfiles.ts` + backend normalize
 * (keep the Rust `default_llm_profiles()` catalog in lockstep).
 */

export type LlmProviderId =
  | "custom"
  | "deepseek"
  | "moonshot"
  | "sensenova"
  | "minimax"
  | "doubao"
  | "hunyuan"
  | "openrouter";

export type LlmProviderPreset = {
  id: LlmProviderId;
  name: string;
  shortName: string;
  baseURL: string;
  model: string;
  badge?: string;
  badgeTone?: "free" | "recommend";
  /** Platform URL for obtaining an API key */
  keyUrl?: string;
  /** When false (local runtimes), empty key is allowed */
  requiresKey?: boolean;
  /** Short secondary line under the selected card */
  hint?: string;
  /** Path under public/, e.g. /icons/providers/deepseek.svg */
  iconSrc?: string;
  /** Monochrome/currentColor icons need invert on dark panels */
  iconMono?: boolean;
};

/**
 * All presets speak the Anthropic Messages protocol (`{base}/v1/messages`).
 * Model ids drift with vendors; users can override or "Fetch models".
 */
export const LLM_PROVIDER_PRESETS: LlmProviderPreset[] = [
  {
    id: "custom",
    name: "自定义",
    shortName: "自定义",
    baseURL: "",
    model: "",
    hint: "手填任意 Anthropic 兼容接口",
  },
  {
    id: "deepseek",
    name: "DeepSeek",
    shortName: "DeepSeek",
    baseURL: "https://api.deepseek.com/anthropic",
    model: "deepseek-v4-flash",
    badge: "推荐",
    badgeTone: "recommend",
    keyUrl: "https://platform.deepseek.com/",
    hint: "V4 Flash · 翻译够用",
    iconSrc: "/icons/providers/deepseek.svg",
  },
  {
    id: "moonshot",
    name: "Moonshot/Kimi",
    shortName: "Kimi",
    baseURL: "https://api.moonshot.cn/anthropic",
    model: "kimi-k3",
    keyUrl: "https://platform.moonshot.cn/",
    hint: "K3 · 官方 Anthropic 端点",
    iconSrc: "/icons/providers/kimi.svg",
    iconMono: true,
  },
  {
    id: "sensenova",
    name: "日日新 SenseNova",
    shortName: "日日新",
    baseURL: "https://token.sensenova.cn/v1",
    // Token 计划在售模型全部免费；6.8 Flash-Lite 支持文本+图片输入。
    model: "sensenova-6.8-flash-lite",
    badge: "免费",
    badgeTone: "free",
    keyUrl: "https://www.sensenova.cn/token-plan",
    hint: "6.8 Flash-Lite · Token 计划免费额度",
    iconSrc: "/icons/providers/sensenova.png",
  },
  {
    id: "minimax",
    name: "MiniMax",
    shortName: "MiniMax",
    baseURL: "https://api.minimaxi.com/anthropic",
    model: "MiniMax-M3",
    keyUrl: "https://platform.minimaxi.com/",
    hint: "M3 · 官方 Anthropic 端点",
    iconSrc: "/icons/providers/minimax.svg",
  },
  {
    id: "doubao",
    name: "豆包",
    shortName: "豆包",
    baseURL: "https://ark.cn-beijing.volces.com/api/compatible",
    // Seed 2.1 Turbo：当前代高频/低成本档（非 Pro）；方舟也可填接入点 ID
    model: "doubao-seed-2-1-turbo-260628",
    keyUrl: "https://console.volcengine.com/ark",
    hint: "2.1 Turbo · 可改成接入点 ID",
    iconSrc: "/icons/providers/doubao.svg",
  },
  {
    id: "hunyuan",
    name: "混元 Hy3",
    shortName: "混元",
    // Hy3 由腾讯云 TokenHub 统一承载（老混元平台已停止新增模型）。
    baseURL: "https://tokenhub.tencentmaas.com/v1",
    model: "hy3",
    keyUrl: "https://console.cloud.tencent.com/tokenhub",
    hint: "Hy3 · 需在 TokenHub 开通模型",
    iconSrc: "/icons/providers/hunyuan.svg",
  },
  {
    id: "openrouter",
    name: "OpenRouter",
    shortName: "OpenRouter",
    baseURL: "https://openrouter.ai/api/v1",
    model: "google/gemini-3.5-flash",
    keyUrl: "https://openrouter.ai/keys",
    hint: "聚合 · 原生 /api/v1/messages",
    iconSrc: "/icons/providers/openrouter.svg",
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
