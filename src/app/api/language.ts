import { invoke } from "@tauri-apps/api/core";
import type { AsrModel } from "../../generated/bindings/AsrModel";
import type { AlignModel } from "../../generated/bindings/AlignModel";
import type { SourceLanguageOption } from "../../generated/bindings/SourceLanguageOption";

export async function listSourceLanguages(
  asrModel: AsrModel,
  alignModel: AlignModel,
): Promise<SourceLanguageOption[]> {
  return invoke("list_source_languages", { asrModel, alignModel });
}
