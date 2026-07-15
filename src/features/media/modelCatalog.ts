import type { AlignModel, AsrModel, DemucsModel, ModelTarget } from "./types";

/** Stable list of ASR models offered in settings / downloads. */
export const ASR_MODELS = [
  "Qwen3-ASR-0.6B",
  "Qwen3-ASR-1.7B",
  "cohere-transcribe-03-2026",
  "moss-transcribe-diarize",
] as const satisfies readonly AsrModel[];

export const ALIGN_MODELS = [
  "mms-300m-1130-forced-aligner",
  "Qwen3-ForcedAligner-0.6B",
] as const satisfies readonly AlignModel[];

export const DEFAULT_ALIGN_MODEL = "mms-300m-1130-forced-aligner" as const satisfies AlignModel;
export const DEFAULT_DEMUCS_MODEL = "htdemucs_ft" as const satisfies DemucsModel;

/** Fixed chunk target (seconds) when MOSS is the active ASR. */
export const MOSS_FIXED_CHUNK_SECONDS = 180;

/**
 * User-facing fact chips (no download-size chip — size lives next to actions only).
 * Main trio: accuracy · languages · speed. Optional: recommended / hard constraints.
 */
export type ModelFactId =
  | "recommended"
  | "accHigher"
  | "accBalanced"
  | "accGood"
  | "langWide"
  | "lang14"
  | "langMeeting"
  | "langLimited"
  | "speedFaster"
  | "speedBalanced"
  | "speedSlower"
  | "chunk180";

export type AsrCatalogEntry = {
  id: AsrModel;
  /** i18n short title under models:names.* */
  nameKey: string;
  /** i18n key under settings:models.* */
  descKey: string;
  facts: readonly ModelFactId[];
};

export const ASR_CATALOG: readonly AsrCatalogEntry[] = [
  {
    id: "Qwen3-ASR-0.6B",
    nameKey: "models:names.asr06b",
    descKey: "settings:models.asr06bDesc",
    facts: ["recommended", "accGood", "langWide", "speedBalanced"],
  },
  {
    id: "Qwen3-ASR-1.7B",
    nameKey: "models:names.asr17b",
    descKey: "settings:models.asr17bDesc",
    facts: ["accHigher", "langWide", "speedBalanced"],
  },
  {
    id: "cohere-transcribe-03-2026",
    nameKey: "models:names.asrCohere",
    descKey: "settings:models.asrCohereDesc",
    facts: ["accHigher", "lang14", "speedFaster"],
  },
  {
    id: "moss-transcribe-diarize",
    nameKey: "models:names.asrMoss",
    descKey: "settings:models.asrMossDesc",
    facts: ["accBalanced", "langMeeting", "speedSlower", "chunk180"],
  },
] as const;

export type AlignCatalogEntry = {
  id: AlignModel;
  nameKey: string;
  descKey: string;
  facts: readonly ModelFactId[];
};

export const ALIGN_CATALOG: readonly AlignCatalogEntry[] = [
  {
    id: "mms-300m-1130-forced-aligner",
    nameKey: "models:names.alignCtc",
    descKey: "settings:models.alignCtcDesc",
    facts: ["recommended", "accGood", "langWide", "speedFaster"],
  },
  {
    id: "Qwen3-ForcedAligner-0.6B",
    nameKey: "models:names.alignQwen",
    descKey: "settings:models.alignQwenDesc",
    facts: ["accBalanced", "langLimited", "speedBalanced"],
  },
] as const;

export type SupportCatalogEntry = {
  target: Exclude<ModelTarget, "asr" | "align">;
  id: DemucsModel;
  nameKey: string;
  titleKey: string;
  descKey: string;
  roleKey: string;
};

export const SUPPORT_CATALOG: readonly SupportCatalogEntry[] = [
  {
    target: "demucs",
    id: DEFAULT_DEMUCS_MODEL,
    nameKey: "models:names.demucs",
    titleKey: "settings:models.demucsTitle",
    descKey: "settings:models.demucsDesc",
    roleKey: "models:section.demucsRole",
  },
] as const;

export function isAsrModel(value: string | undefined | null): value is AsrModel {
  if (!value) return false;
  return (ASR_MODELS as readonly string[]).includes(value);
}

export function isAlignModel(value: string | undefined | null): value is AlignModel {
  if (!value) return false;
  return (ALIGN_MODELS as readonly string[]).includes(value);
}

export function asrUsesFixedChunk(model: AsrModel): boolean {
  return model === "moss-transcribe-diarize";
}

export function factLabelKey(fact: ModelFactId): string {
  return `models:facts.${fact}`;
}

export function asrCatalogEntry(id: AsrModel): AsrCatalogEntry | undefined {
  return ASR_CATALOG.find((e) => e.id === id);
}

export function alignCatalogEntry(id: AlignModel): AlignCatalogEntry | undefined {
  return ALIGN_CATALOG.find((e) => e.id === id);
}
