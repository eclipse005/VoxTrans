# Hotwords Design

## Goal

Add a dedicated hotword correction feature for ASR output. Hotwords are separate from translation terminology: terminology guides translation wording, while hotwords repair source transcription errors before sentence segmentation and translation.

The feature must improve mixed Chinese/English, proper nouns, product names, names, and homophones without turning the transcript into broad LLM rewriting.

## Placement In Pipeline

Hotword correction runs after Step1 ASR and before Step2 sentence segmentation:

1. Step1 produces word-level ASR tokens.
2. Step1.5 hotword correction scans and repairs token text.
3. Step2 builds source sentence spans from corrected tokens.

Step2 input and output shapes stay compatible with the current pipeline. The only behavioral change is that Step2 receives corrected token text when hotwords are enabled.

The new artifact is `step_01_5_hotwords.json`.

## User Model

Hotwords are configured through a dedicated UI button, next to terminology but not inside terminology.

Saved settings add:

```ts
type HotwordGroup = {
  id: string;
  name: string;
  terms: HotwordTerm[];
};

type HotwordTerm = {
  id: string;
  word: string;
  aliases: string[];
  lang: "zh" | "non_zh" | "auto";
  note?: string;
};
```

Settings also add `enableHotwords: boolean` and `hotwordGroups: HotwordGroup[]`.

The UI must support multiple groups, inline add, batch paste, edit, delete, and save. Batch paste accepts lines like:

```text
Claude Code = cloud code, clod code, 克劳德代码
浩叔 = 浩书, 皓叔
```

## Preprocessing

Hotword entries are normalized before matching. Runtime matching uses normalized records, not raw UI entries.

Each normalized hotword contains:

```rust
word
aliases
generated_aliases
pinyin
first_letters
lang
```

Chinese entries:

- Trim and deduplicate `word` and `aliases`.
- Generate pinyin without tones for `word` and every Chinese alias.
- Generate first-letter abbreviations from pinyin.
- These derived fields are required in V1.

Non-Chinese entries:

- Trim and deduplicate `word` and `aliases`.
- Generate fuzzy pronunciation candidates when aliases are missing or incomplete.
- Deterministic fuzzy rules always run. A small LLM preprocessing call augments candidates only when LLM settings are available.
- Generated candidates are stored in the Step1.5 request/output for traceability.

The fuzzy candidate generator is not called for every subtitle sentence. It runs once per hotword correction job for configured hotwords, then local recall uses the generated candidates.

## Candidate Recall

Step1.5 scans the Step1 word tokens and builds replacement candidates through three paths.

1. Non-Chinese recall:
   - Tokenize ASCII-ish text.
   - Use sliding token windows.
   - Match against `word`, `aliases`, and `generated_aliases`.
   - Example: `cloud code` may recall target `Claude Code`.

2. Chinese alias recall:
   - Directly match configured Chinese aliases in source text.
   - Example: `浩书` may recall target `浩叔`.

3. Chinese homophone recall:
   - Slide character windows using the target hotword length.
   - Compare no-tone pinyin and first-letter strings.
   - Example: `皓叔` and `浩书` may recall `浩叔` even if not listed as aliases.

Recall results are merged and deduplicated by token range, source text, and target hotword. If multiple hotwords compete for the same range, prefer direct alias matches over homophone matches, then prefer longer matches.

## LLM Decision

The LLM is only called for recalled candidates. No recall means no LLM call.

For each candidate or small candidate batch, Step1.5 sends:

- current candidate text
- target hotword
- source transcript context around the candidate
- candidate source type: `alias`, `generated_alias`, `pinyin`, or `first_letters`

The LLM must return strict JSON:

```json
{
  "replace": true,
  "target": "Claude Code",
  "reason": "Context is about the product name Claude Code."
}
```

Only `replace: true` applies a correction. Invalid JSON, API failure, or refusal leaves the original text unchanged and records the skipped decision in the artifact.

Hotword correction must not rewrite unrelated text. The prompt must explicitly forbid grammar edits, paraphrasing, punctuation edits, and broad cleanup.

## Strict Replacement

Replacement applies only to the recalled token range:

- Single-token match: replace that token text.
- Multi-token match: merge into one token using the first token start time and last token end time.
- Removed merged tokens must not be passed to Step2.
- Timing is preserved from the original ASR range.

This keeps subtitle timing stable and prevents LLM output from shifting surrounding words.

## Artifact Shape

`step_01_5_hotwords.json` stores:

```json
{
  "task_id": "...",
  "media_path": "...",
  "source_lang": "...",
  "enabled": true,
  "hotwords": [],
  "candidates": [],
  "decisions": [],
  "corrections": [],
  "words": []
}
```

`words` is the corrected word-token list consumed by Step2. When hotwords are disabled or no hotwords exist, `words` equals Step1 words and the artifact records `enabled: false` or an empty hotword list.

## Cache Semantics

Step1 remains cacheable.

Step1.5 is its own pipeline checkpoint. Re-running from a clean task uses the current hotword settings snapshot. Existing stale checkpoints are not given special compatibility handling; the user tests with new task files.

Step2 consumes Step1.5 output when present in the same pipeline execution.

## Error Handling

Hotwords disabled or empty:

- Skip recall and LLM work.
- Emit a minimal artifact if Step1.5 runs.

No LLM settings available:

- Keep original ASR text unchanged.
- Record candidates as skipped because no LLM reviewer is available.
- Do not fail the whole transcription task.

LLM failure or invalid response:

- Keep original text for that candidate.
- Record the error in the decision entry.
- Continue processing other candidates.

Overlapping accepted corrections:

- Apply non-overlapping corrections only.
- Resolve conflicts by recall priority and match length.

## Frontend

Add a Hotwords modal modeled after the existing terminology modal, but with hotword-specific fields:

- Correct word
- Aliases
- Language selector: Auto, Chinese, Non-Chinese
- Batch import
- Group tabs

Settings modal adds an `enableHotwords` toggle near `enableTerminology`.

Queue settings snapshots include `hotwordGroups` and `enableHotwords` so queued tasks are reproducible.

## Backend Components

Add a new hotword service under `src-tauri/src/services/hotwords.rs`.

Core responsibilities:

- Normalize hotword groups.
- Generate Chinese pinyin and first letters.
- Generate non-Chinese fuzzy aliases once per job.
- Recall candidates from ASR tokens.
- Ask LLM for replacement decisions.
- Apply strict token-range corrections.
- Return corrected words and audit metadata.

Workspace adds `Step15HotwordsPipelineStep` between Step1 and Step2.

Preferences and command DTOs add hotword groups and enable flag, mirroring the existing terminology settings pattern.

## Testing

Rust unit tests cover:

- Chinese homophone recall without aliases.
- Chinese first-letter recall.
- Chinese direct alias recall.
- Non-Chinese generated alias recall when user provides no alias.
- Multi-token replacement merges timing correctly.
- LLM false decision leaves text unchanged.
- Invalid LLM decision leaves text unchanged.
- Empty or disabled hotwords pass through original words.

Frontend build verification covers TypeScript shape changes and modal integration.

## Out Of Scope For V1

- Auto-learning from manual subtitle edits.
- Import/export files beyond batch paste.
- Large industry hotword generation UI.
- Editing already translated subtitles retroactively.
- Replacing the terminology feature.
