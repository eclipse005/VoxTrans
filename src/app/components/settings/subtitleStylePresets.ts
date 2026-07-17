// Built-in subtitle style presets. Each preset is a complete
// `SubtitleRenderStyle` snapshot: selecting one replaces the whole style
// object in the settings form, after which the user can freely fine-tune
// any field — the preset only seeds the values, it does not lock them.
//
// All values are kept inside the backend clamp ranges (fontSize 16–96,
// outline 0.1–8 (libass renders nothing at exactly 0), shadow 0–8,
// borderOpacity 0–100, marginV 0–200, bilingualLineGap 0–140,
// alignment ∈ {1,2,3}) so nothing gets truncated.

import type { SubtitleRenderStyle } from "../../../features/media/types";

export type SubtitleStylePreset = {
  /** Stable id used as the <option value>. */
  id: string;
  /** i18n key for the display name in the preset dropdown. */
  labelKey: string;
  /** Full style snapshot applied on selection. */
  style: SubtitleRenderStyle;
};

export const SUBTITLE_STYLE_PRESETS: SubtitleStylePreset[] = [
  {
    id: "classic-cinema",
    labelKey: "settings:subtitle.stylePresetClassicCinema",
    style: {
      source: {
        fontFamily: "Arial",
        fontSize: 44,
        primaryColor: "#FFFFFF",
        outlineColor: "#0F0F0F",
        backColor: "#000000",
        outline: 2.2,
        shadow: 1.2,
        borderStyle: "outline",
        borderOpacity: 90,
      },
      target: {
        fontFamily: "Microsoft YaHei",
        fontSize: 40,
        primaryColor: "#F5F5F5",
        outlineColor: "#0F0F0F",
        backColor: "#000000",
        outline: 2.0,
        shadow: 1.0,
        borderStyle: "outline",
        borderOpacity: 90,
      },
      layout: {
        marginV: 50,
        alignment: 2,
        bilingualLineGap: 12,
      },
    },
  },
  {
    id: "modern-streaming",
    labelKey: "settings:subtitle.stylePresetModernStreaming",
    style: {
      source: {
        fontFamily: "Helvetica",
        fontSize: 42,
        primaryColor: "#FFFFFF",
        outlineColor: "#000000",
        backColor: "#000000",
        outline: 0.1,
        shadow: 0.0,
        borderStyle: "box",
        borderOpacity: 70,
      },
      target: {
        fontFamily: "PingFang SC",
        fontSize: 38,
        primaryColor: "#FFFFFF",
        outlineColor: "#000000",
        backColor: "#000000",
        outline: 0.1,
        shadow: 0.0,
        borderStyle: "box",
        borderOpacity: 70,
      },
      layout: {
        marginV: 60,
        alignment: 2,
        bilingualLineGap: 10,
      },
    },
  },
  {
    id: "documentary-gold",
    labelKey: "settings:subtitle.stylePresetDocumentaryGold",
    style: {
      source: {
        fontFamily: "Arial",
        fontSize: 42,
        primaryColor: "#FFFFFF",
        outlineColor: "#151515",
        backColor: "#000000",
        outline: 2.5,
        shadow: 1.5,
        borderStyle: "outline",
        borderOpacity: 85,
      },
      target: {
        fontFamily: "Microsoft YaHei",
        fontSize: 38,
        primaryColor: "#FFCC33",
        outlineColor: "#151515",
        backColor: "#000000",
        outline: 2.5,
        shadow: 1.5,
        borderStyle: "outline",
        borderOpacity: 85,
      },
      layout: {
        marginV: 45,
        alignment: 2,
        bilingualLineGap: 14,
      },
    },
  },
  {
    id: "vibrant-variety",
    labelKey: "settings:subtitle.stylePresetVibrantVariety",
    style: {
      source: {
        fontFamily: "Arial",
        fontSize: 48,
        primaryColor: "#FFFF00",
        outlineColor: "#2A1100",
        backColor: "#000000",
        outline: 5.0,
        shadow: 3.0,
        borderStyle: "outline",
        borderOpacity: 100,
      },
      target: {
        fontFamily: "SimHei",
        fontSize: 44,
        primaryColor: "#00FFFF",
        outlineColor: "#111111",
        backColor: "#000000",
        outline: 5.0,
        shadow: 3.0,
        borderStyle: "outline",
        borderOpacity: 100,
      },
      layout: {
        marginV: 55,
        alignment: 2,
        bilingualLineGap: 8,
      },
    },
  },
  {
    id: "midnight-soft",
    labelKey: "settings:subtitle.stylePresetMidnightSoft",
    style: {
      source: {
        fontFamily: "Helvetica",
        fontSize: 42,
        primaryColor: "#D1D5DB",
        outlineColor: "#2D3748",
        backColor: "#000000",
        outline: 1.8,
        shadow: 0.0,
        borderStyle: "outline",
        borderOpacity: 75,
      },
      target: {
        fontFamily: "PingFang SC",
        fontSize: 38,
        primaryColor: "#E2E8F0",
        outlineColor: "#2D3748",
        backColor: "#000000",
        outline: 1.8,
        shadow: 0.0,
        borderStyle: "outline",
        borderOpacity: 75,
      },
      layout: {
        marginV: 40,
        alignment: 2,
        bilingualLineGap: 12,
      },
    },
  },
  {
    id: "executive-news",
    labelKey: "settings:subtitle.stylePresetExecutiveNews",
    style: {
      source: {
        fontFamily: "Arial",
        fontSize: 38,
        primaryColor: "#FFFFFF",
        outlineColor: "#1E3A8A",
        backColor: "#000000",
        outline: 0.1,
        shadow: 0.0,
        borderStyle: "box",
        borderOpacity: 95,
      },
      target: {
        fontFamily: "Microsoft YaHei",
        fontSize: 36,
        primaryColor: "#FFFFFF",
        outlineColor: "#1E3A8A",
        backColor: "#000000",
        outline: 0.1,
        shadow: 0.0,
        borderStyle: "box",
        borderOpacity: 95,
      },
      layout: {
        marginV: 70,
        alignment: 1,
        bilingualLineGap: 8,
      },
    },
  },
];
