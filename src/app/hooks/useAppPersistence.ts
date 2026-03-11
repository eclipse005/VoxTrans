import { useEffect } from "react";
import type { SavedSettings } from "../../features/media/types";
import type { AppAction, AppState } from "../state/appReducer";
import type { HotwordCorrection, TermEntry } from "../types";

type PatchState = (payload: Partial<AppState>) => void;
type DispatchState = (action: AppAction) => void;

function isValidHotwordCorrection(value: unknown): value is HotwordCorrection {
  if (!value || typeof value !== "object") return false;
  const candidate = value as Partial<HotwordCorrection>;
  if (typeof candidate.enabled !== "boolean") return false;
  if (typeof candidate.activeGroupId !== "string") return false;
  if (!Array.isArray(candidate.groups) || candidate.groups.length === 0) return false;
  return candidate.groups.every((group) =>
    group
    && typeof group.id === "string"
    && typeof group.name === "string"
    && Array.isArray(group.keyterms)
    && group.keyterms.every((term) => typeof term === "string"));
}

function mapTermsToHotwordCorrection(terms: TermEntry[]): HotwordCorrection {
  return {
    enabled: true,
    activeGroupId: "group-0",
    groups: [{
      id: "group-0",
      name: "默认分组",
      keyterms: terms.map((item) => item.target.trim() ? `${item.source}:${item.target}` : item.source),
    }],
  };
}

export function useAppPersistence(terms: TermEntry[], hotwordCorrection: HotwordCorrection, dispatch: DispatchState, patch: PatchState) {
  useEffect(() => {
    try {
      const rawTerms = localStorage.getItem("voxtrans.terms");
      if (rawTerms) {
        const parsed = JSON.parse(rawTerms) as TermEntry[];
        if (Array.isArray(parsed)) {
          dispatch({ type: "set_terms", terms: parsed });
          const rawHotword = localStorage.getItem("voxtrans.hotwordCorrection");
          if (!rawHotword) {
            patch({ hotwordCorrection: mapTermsToHotwordCorrection(parsed) });
          }
        }
      }

      const rawSettings = localStorage.getItem("voxtrans.settings");
      if (rawSettings) {
        const parsed = JSON.parse(rawSettings) as SavedSettings;
        if (parsed?.provider && parsed?.chunkTargetSeconds) {
          patch({
            settings: parsed,
            draftProvider: parsed.provider,
            draftChunkInput: String(parsed.chunkTargetSeconds),
          });
        }
      }

      const rawLlm = localStorage.getItem("voxtrans.llm");
      if (rawLlm) {
        const parsed = JSON.parse(rawLlm) as { apiKey?: string; apiBase?: string; apiModel?: string };
        patch({
          draftApiKey: parsed.apiKey ?? "",
          draftApiBase: parsed.apiBase ?? "",
          draftApiModel: parsed.apiModel ?? "",
        });
      }

      const rawHotword = localStorage.getItem("voxtrans.hotwordCorrection");
      if (rawHotword) {
        const parsed = JSON.parse(rawHotword) as unknown;
        if (isValidHotwordCorrection(parsed)) {
          patch({ hotwordCorrection: parsed });
        }
      }
    } catch {
      // Ignore corrupted local storage.
    }
  }, [dispatch, patch]);

  useEffect(() => {
    localStorage.setItem("voxtrans.terms", JSON.stringify(terms));
  }, [terms]);

  useEffect(() => {
    localStorage.setItem("voxtrans.hotwordCorrection", JSON.stringify(hotwordCorrection));
  }, [hotwordCorrection]);
}
