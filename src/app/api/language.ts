import { invoke } from "@tauri-apps/api/core";
import type {
  AsrModel,
  AlignModel,
  SourceLanguageOption,
} from "@/generated/bindings";

export async function listSourceLanguages(
  asrModel: AsrModel,
  alignModel: AlignModel,
): Promise<SourceLanguageOption[]> {
  return invoke("list_source_languages", { asrModel, alignModel });
}
