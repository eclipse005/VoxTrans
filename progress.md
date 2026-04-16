# Progress

## Current Goal

Only refactor the `step2` sentence-building stage for now:

- input: `asr.json`
- output: `step2_source_sentences.json`
- do **not** run later translation stages yet

The target is:

- translation-friendly source sentences
- complete semantic units as much as possible
- multi-language compatible
- weak-model friendly (`LongCat` baseline)
- faster than the earlier heavy LLM windowing approach

## Current Architecture Direction

The old `step2` designs were tried and are no longer the preferred direction:

1. pairwise `MERGE/SPLIT/UNSURE` boundary classification
2. sliding-window "first sentence end" prediction
3. sliding-window grouped sentence prediction with overlap

These improved quality somewhat, but were still too slow and/or still produced fragments.

The current intended direction is:

- `atoms + sparse LLM + DP`

Meaning:

1. build small `micro_chunks` / atoms first
2. use hard rules only for truly hard boundaries
3. use LLM only on uncertain boundaries
4. use DP to choose final translation-ready sentence spans

## Files Already Changed

Main files:

- `D:\voxtrans\src-tauri\src\services\transcription\sentence_boundary.rs`
- `D:\voxtrans\src-tauri\src\services\transcription\mod.rs`
- `D:\voxtrans\src-tauri\src\commands\transcription.rs`
- `D:\voxtrans\src-tauri\src\main.rs`

Relevant shared LLM files inspected:

- `D:\voxtrans\src-tauri\src\services\llm\client.rs`
- `D:\voxtrans\src-tauri\src\services\llm\json_guard.rs`
- `D:\voxtrans\src-tauri\src\services\llm\batch.rs`

## Current `sentence_boundary.rs` Design

Current constants:

- `HARD_SPLIT_GAP_MS = 2000`
- `ATOM_MAX_WORDS = 18`
- `ATOM_MAX_DURATION_MS = 5200`
- `DP_MAX_ATOMS_PER_SENTENCE = 5`
- `HIGH_CONF_PUNCT_SPLIT_MIN_WORDS = 3`
- `BOUNDARY_CONTEXT_RADIUS = 1`

Current flow:

1. normalize and beautify words
2. build `micro_chunks`
3. classify boundaries:
   - hard split if gap >= 2000ms
   - high-confidence punctuation split for some chunk endings
   - otherwise ask LLM
4. run DP per hard-split region
5. reconstruct `translation_sentences`
6. emit `boundaries` for debugging

Public output structure already exists in `step2_source_sentences.json`:

- `microChunks`
- `boundaries`
- `translationSentences`

## CLI / Command Entry

The dedicated CLI mode already exists:

```powershell
D:\voxtrans\target\debug\voxtrans.exe --voxtrans-build-source-sentences --asr-path "D:\voxtrans\target\debug\output\Donald Trump & Volodymyr Zelensky’s explosive White House fight IN FULL_1775960547688-ucuk23\asr.json"
```

Tauri command already exists too:

- `build_source_sentences`

## Saved LLM Config Used

The CLI now reads saved app settings automatically when explicit LLM args are not passed.

Current saved settings found in:

- `C:\Users\ADMIN\AppData\Roaming\com.voxtrans.desktop\settings.json`

Observed values:

- `translateApiKey = "1"`
- `translateBaseUrl = "http://localhost:8088/v1"`
- `translateModel = "LongCat"`
- `llmConcurrency = 4`

## Sample Used for Testing

Primary sample:

- `D:\voxtrans\target\debug\output\Donald Trump & Volodymyr Zelensky’s explosive White House fight IN FULL_1775960547688-ucuk23\asr.json`

Output written to:

- `D:\voxtrans\target\debug\output\Donald Trump & Volodymyr Zelensky’s explosive White House fight IN FULL_1775960547688-ucuk23\step2_source_sentences.json`

LLM raw logs:

- `D:\voxtrans\target\debug\output\Donald Trump & Volodymyr Zelensky’s explosive White House fight IN FULL_1775960547688-ucuk23\gpt.log`

## Latest Actual Run Result

Latest run command succeeded.

Measured wall time:

- `129112 ms`

Latest metrics from `step2_source_sentences.json`:

- `microChunkTotal = 111`
- `boundaryTotal = 110`
- `sentenceTotal = 99`
- `fragmentish = 5`
- `shortWords(<4 tokens) = 9`
- `under1s = 14`
- `avgWords = 7.64`
- `avgDurationSec = 2.36`

This is faster than the older heavy windowing version, but still not good enough.

## Current Output Quality Snapshot

Some parts are already acceptable:

- `Everybody has problems, even you.`
- `But you have nice ocean and don't feel now.`
- `Because you're in no position to dictate that.`

But some fragment-like bad outputs still exist:

- `And what you're doing is very disrespectful to the country, this country,`
- `Offer some words of appreciation for the United States of America and the president who's trying`
- `If you didn't have our military equipment, if you didn't have our military equipment, this`
- `Accept that there are disagreements, and let's go litigate those disagreements rather than trying to fight it`
- `Look, if you could get a ceasefire right now, I tell you take it so the bullets stop`

## Important Root Cause Already Found

The remaining bad fragments are not mainly from DP itself.

They are mostly from uncertain boundaries where the LLM call failed and the code fell back to:

- `llm_decision = UNSURE`

Examples observed in boundary debug data:

- `reason = llm_error:llm call failed after 4 attempts: kind=invalid_schema`
- `reason = llm_error:llm call failed after 4 attempts: kind=invalid_json`

So the main remaining problems are:

1. JSON/schema failures on weak model `LongCat`
2. fallback on those failures still allows split-heavy outcomes

## Important Observation About Performance

Concurrency is already wired through:

- `D:\voxtrans\src-tauri\src\services\llm\batch.rs`
- `run_indexed_concurrent(...)`

So the latest slowness is **not** mainly because concurrency is missing.

The current bottleneck is:

- a relatively small number of uncertain boundaries
- but some of those boundary calls are slow and/or repeatedly fail JSON/schema validation

In other words:

- LLM call count is already much lower than before
- the remaining expensive calls are still costly on `LongCat`

## Shared LLM Repair Layer Status

The latest user-approved direction:

- build a **shared LLM repair prompt framework**
- use it anywhere a JSON response is malformed
- do **not** keep retrying the original task many times

Desired chain:

1. main call
2. local JSON repair / extraction
3. specialized LLM repair prompt for bad JSON
4. if repair still fails, apply task-specific safe fallback

This should be shared infrastructure, not only for `step2`.

Relevant files changed for that:

- `D:\voxtrans\src-tauri\src\services\llm\client.rs`
- `D:\voxtrans\src-tauri\src\services\llm\json_guard.rs`

This shared repair layer is now implemented.

Current shared behavior:

1. main LLM call
2. local JSON extraction / minor repair
3. shared LLM JSON repair prompt if JSON/schema still fails
4. task-level fallback only after repair fails

Additional notes:

- local extraction now records the repair source (`raw`, `thought_stripped`, `fenced_json`, `balanced_json`, `common_repair`)
- `client.rs` now logs `repair_requested` and `ok_after_repair`
- original-task blind retries are no longer the main path for JSON/schema failures

## Strong Product / Design Constraints From User

These were explicit and should be preserved:

- do not keep patching blindly; design from the source
- support arbitrary language pairs, not only English -> Chinese
- avoid heavy language-specific hardcoded syntax rules
- only very hard constraints should stay in code
- one accepted hard rule: long pause `>= 2000ms` must split
- terminal punctuation can be used as a strong signal, but not as an absolute truth
- sentence units should be complete enough for direct translation
- translation-oriented grouping is more important than display-oriented short chunks

The user also agreed with the higher-level direction:

- first use punctuation / strong cues to cut into atoms
- then use DP to decide merges
- let LLM handle only ambiguous boundaries

## Step2 Fallback Tuning Status

Also implemented:

- `sentence_boundary.rs` now treats `llm_error:*` boundaries as merge-biased in DP
- these boundaries still keep `llm_decision = UNSURE` for debugging, but:
  - fallback confidence is reduced
  - merge penalty is cheaper
  - split penalty is more expensive
- boundary prompt was narrowed so weak models can return a smaller JSON object:
  - required field is now mainly `decision`

## Latest Actual Run Result After Repair Layer + Merge-Biased Fallback

Rebuilt binary and reran:

```powershell
D:\voxtrans\target\debug\voxtrans.exe --voxtrans-build-source-sentences --asr-path "D:\voxtrans\target\debug\output\Donald Trump & Volodymyr Zelensky’s explosive White House fight IN FULL_1775960547688-ucuk23\asr.json"
```

Measured wall time:

- `61317 ms`

Latest metrics from `step2_source_sentences.json`:

- `microChunkTotal = 111`
- `boundaryTotal = 110`
- `sentenceTotal = 98`
- `fragmentish = 18`  `(quick heuristic: short text OR under 1s)`
- `shortWords(<4 tokens) = 10`
- `under1s = 13`
- `avgWords = 7.71`
- `avgDurationSec = 2.39`
- `llmErrorBoundaries = 0`

Important interpretation:

- the shared repair layer appears to have removed the observed `llm_error` boundaries on the sample
- runtime improved significantly versus the earlier `129112 ms` sample run
- remaining weak spots now look more like segmentation policy / short-utterance tradeoffs than raw JSON failure

## Recommended Next Step

Do these next, in this order:

### 1. Investigate remaining short-sentence over-splitting

Now that `llm_error` is gone on the sample, focus on why short outputs still remain, especially cases like:

- `Wait a minute.`
- `Don't listen.`
- `What about US?`
- `One more question.`

Need to distinguish:

- acceptable short complete utterances
- true translation-harmful fragments

### 2. Revisit punctuation-driven atom / split bias

Current atoms still close on terminal punctuation, and some boundaries may still be too easy to split around short clauses.

Likely next direction:

- keep punctuation as a strong signal
- but reduce split bias for very short clauses unless the next unit clearly starts fresh

### 3. Tighten evaluation heuristics

The current quick fragment heuristic is crude.
Next pass should evaluate:

- incomplete dependency patterns
- clause carryover
- repeated left-edge truncation patterns

instead of counting all short complete utterances as equally bad.

## Last Verified Build/Test State

Verified before handoff:

- `cargo test -p voxtrans sentence_boundary` passed
- `cargo build -p voxtrans` passed

Latest sample run also completed successfully and wrote:

- `D:\voxtrans\target\debug\output\Donald Trump & Volodymyr Zelensky’s explosive White House fight IN FULL_1775960547688-ucuk23\step2_source_sentences.json`

## Important Note for the Next AI

Do **not** spend time on later translation stages yet.
The user explicitly wants to focus stage by stage.

Current focus is only:

- `asr.json -> step2_source_sentences.json`

The next AI should continue from:

- shared LLM JSON repair infrastructure
- then `step2` merge-biased fallback tuning
