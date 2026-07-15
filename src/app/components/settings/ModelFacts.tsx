import { useTranslation } from "react-i18next";
import { factLabelKey, type ModelFactId } from "../../../features/media/modelCatalog";

type ModelFactsProps = {
  facts?: readonly ModelFactId[];
};

/**
 * Decision chips only: recommend + accuracy + languages + speed (+ constraints).
 * Download size is shown once next to actions — not repeated here.
 */
export function ModelFacts({ facts = [] }: ModelFactsProps) {
  const { t } = useTranslation(["models"]);
  const ordered = orderFacts(facts);
  if (ordered.length === 0) return null;

  return (
    <span className="model-tag-row">
      {ordered.map((fact) => (
        <span key={fact} className={`model-tag model-tag-${factGroup(fact)}`}>
          {t(factLabelKey(fact))}
        </span>
      ))}
    </span>
  );
}

const FACT_ORDER: readonly ModelFactId[] = [
  "recommended",
  "accHigher",
  "accBalanced",
  "accGood",
  "langWide",
  "lang14",
  "langMeeting",
  "langLimited",
  "speedFaster",
  "speedBalanced",
  "speedSlower",
  "chunk180",
];

function orderFacts(facts: readonly ModelFactId[]): ModelFactId[] {
  const set = new Set(facts);
  return FACT_ORDER.filter((id) => set.has(id));
}

function factGroup(fact: ModelFactId): string {
  if (fact === "recommended") return "recommended";
  if (fact.startsWith("acc")) return "accuracy";
  if (fact.startsWith("lang")) return "languages";
  if (fact.startsWith("speed")) return "speed";
  if (fact === "chunk180") return "constraint";
  return fact;
}
