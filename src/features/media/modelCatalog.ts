import type { AlignModel, AsrModel, DemucsModel, ModelTarget } from "./types";

/** Stable list of ASR models offered in settings / downloads. */
export const ASR_MODELS = [
  "Qwen3-ASR-0.6B",
  "Qwen3-ASR-1.7B",
  "cohere-transcribe-03-2026",
  "moss-transcribe-diarize",
] as const satisfies readonly AsrModel[];

export const DEFAULT_ALIGN_MODEL = "Qwen3-ForcedAligner-0.6B" as const satisfies AlignModel;
export const DEFAULT_DEMUCS_MODEL = "htdemucs_ft" as const satisfies DemucsModel;

/** Fixed chunk target (seconds) when MOSS is the active ASR. */
export const MOSS_FIXED_CHUNK_SECONDS = 180;

export type ModelTagId =
  | "recommended"
  | "lightweight"
  | "highAccuracy"
  | "multilingual"
  | "langs14"
  | "european"
  | "params2b"
  | "chunk180"
  | "meeting";

export type AsrCatalogEntry = {
  id: AsrModel;
  /** i18n key under settings:models.* */
  descKey: string;
  tagIds: readonly ModelTagId[];
};

export const ASR_CATALOG: readonly AsrCatalogEntry[] = [
  {
    id: "Qwen3-ASR-0.6B",
    descKey: "settings:models.asr06bDesc",
    tagIds: ["recommended", "lightweight", "multilingual"],
  },
  {
    id: "Qwen3-ASR-1.7B",
    descKey: "settings:models.asr17bDesc",
    tagIds: ["highAccuracy", "multilingual"],
  },
  {
    id: "cohere-transcribe-03-2026",
    descKey: "settings:models.asrCohereDesc",
    tagIds: ["langs14", "european", "params2b"],
  },
  {
    id: "moss-transcribe-diarize",
    descKey: "settings:models.asrMossDesc",
    tagIds: ["chunk180", "meeting"],
  },
] as const;

export type SupportCatalogEntry = {
  target: Exclude<ModelTarget, "asr">;
  id: AlignModel | DemucsModel;
  titleKey: string;
  descKey: string;
  roleKey: string;
};

export const SUPPORT_CATALOG: readonly SupportCatalogEntry[] = [
  {
    target: "align",
    id: DEFAULT_ALIGN_MODEL,
    titleKey: "settings:models.alignTitle",
    descKey: "settings:models.alignDesc",
    roleKey: "models:section.alignRole",
  },
  {
    target: "demucs",
    id: DEFAULT_DEMUCS_MODEL,
    titleKey: "settings:models.demucsTitle",
    descKey: "settings:models.demucsDesc",
    roleKey: "models:section.demucsRole",
  },
] as const;

export function isAsrModel(value: string | undefined | null): value is AsrModel {
  if (!value) return false;
  return (ASR_MODELS as readonly string[]).includes(value);
}

export function asrUsesFixedChunk(model: AsrModel): boolean {
  return model === "moss-transcribe-diarize";
}

export function tagLabelKey(tag: ModelTagId): string {
  return `models:tags.${tag}`;
}
