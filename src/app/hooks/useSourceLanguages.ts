import { useQuery, useQueryClient } from "@tanstack/react-query";
import { listSourceLanguages } from "../api/language";
import type { AsrModel, AlignModel } from "../../generated/bindings";

const SOURCE_LANGUAGES_KEY = "sourceLanguages";

export function useSourceLanguages(asrModel: AsrModel, alignModel: AlignModel) {
  return useQuery({
    queryKey: [SOURCE_LANGUAGES_KEY, asrModel, alignModel],
    queryFn: () => listSourceLanguages(asrModel, alignModel),
    staleTime: Infinity,
  });
}

export function useInvalidateSourceLanguages() {
  const queryClient = useQueryClient();
  return () =>
    queryClient.invalidateQueries({ queryKey: [SOURCE_LANGUAGES_KEY] });
}
