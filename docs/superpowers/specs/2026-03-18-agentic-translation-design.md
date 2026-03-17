# Agentic Translation Design (EN -> ZH)

Date: 2026-03-18  
Status: Approved for planning  
Scope: Single video only

## 1. Goal

Build an AI-agent-driven translation stage between ASR completion and SRT generation.

Pipeline boundary:
- Input: English ASR segments (`id/start/end/text`)
- Output: Chinese subtitle text mapped to original segments for existing SRT generation

Constraints:
- OAI-compatible cloud API only (`API_KEY`, `BASE_URL`, `MODEL`)
- No external web retrieval during translation
- Knowledge sources limited to subtitle context + user glossary
- Single-video autonomous loop (no cross-video memory)

## 2. Product Intent

Primary quality targets:
- Subtitle readability (short, natural, conversational Chinese, reading-speed aware)
- Terminology accuracy and consistency (professional terms and named entities)
- Strategy auto-adaptation by subtitle style while preserving subtitle suitability

## 3. Approach Selection

Chosen approach: Linear 3-agent loop
- `Planner -> Translator -> Reviewer`
- If Reviewer rejects, return to Translator with explicit rewrite instructions
- Maximum 3 rounds per window

Why selected:
- Closest to autonomous coding-agent style (plan, execute, review loop)
- Higher controllability than single-pass pipeline
- Lower implementation complexity than adversarial or routed multi-agent variants

## 4. Architecture and Module Boundaries

Target location: `src-tauri/src/services/transcription/`

Modules:
1. `agent_orchestrator.rs`
- Owns state machine and round control
- States: `PlanReady -> Translating(round_n) -> Reviewing(round_n) -> Accepted | Fallback`

2. `planner_agent.rs`
- Reads sampled full-video subtitles + glossary
- Produces `TranslationPlan` (style, terminology, length/readability rules, forbidden patterns)

3. `translator_agent.rs`
- Translates current window using `TranslationPlan`
- Accepts prior reviewer feedback for rewrite rounds
- Produces draft with confidence metadata

4. `reviewer_agent.rs`
- Reviews draft against source, glossary, and plan
- Produces pass/fail decision, structured issues, and rewrite instructions

5. `finalizer.rs`
- Assembles accepted translations into original segment order
- Keeps timestamps and cue order unchanged

6. `oai_compatible_client.rs`
- Unified API layer for all agents
- Handles timeout/retry/error classification/response parsing
- Decouples vendor-specific details from business logic

## 5. Data Contracts

### `TranslationPlan`
- `target_lang: "zh-CN"`
- `style_policy`: conversational, concise, subtitle-readability-first
- `terminology_rules`: glossary with lock semantics
- `length_rules`: max chars per cue, prefer short clauses
- `forbidden_patterns`: overly literary output, rigid literalism that hurts readability

### `DraftTranslation`
- `window_id`
- `items: [{segment_id, translated_text, confidence}]`
- optional self-notes for reviewer context

### `ReviewResult`
- `pass: bool`
- `score: {readability, terminology, fidelity}`
- `issues: [{segment_id, type, message}]`
- `rewrite_instructions`

### `AgentRoundRecord`
- round number (1..3)
- translator output snapshot
- reviewer result snapshot
- cost metrics (token/time)

## 6. Windowing and Context Strategy

Window size (locked for MVP):
- `25-40` subtitle segments per window
- Default: `32`

Context rules:
- Include limited previous-window summary for coherence
- Preserve strict segment mapping by `segment_id`
- If token budget exceeded, auto-split window and retry

## 7. Round Decision and Fallback

Pass conditions:
- Readability score and terminology score meet configured thresholds
- No critical issue (term mistranslation, meaning inversion, factual mistranslation)

Retry behavior:
- Reviewer fail => Translator rewrite with reviewer instructions
- Max 3 rounds per window

Fallback policy:
- If still fail after round 3, keep best-scored round output
- Mark window/file with `needs_manual_review=true`
- Do not block full SRT export

## 8. Runtime Configuration

User-configurable (frontend -> tauri payload):
- `apiKey`
- `baseUrl`
- `model`
- `windowSize` (default 32, range 25-40)
- `temperature` (recommend low for term stability)
- `glossary` (`source -> target`)
- `strictTerminology` (hard fail for glossary violations)

System defaults:
- `maxRounds = 3`
- Exponential backoff retries for API failures

## 9. Error Handling and Resilience

- API/network/transient errors: retry with exponential backoff + jitter (max 3)
- Structured-output parse failures: one format-repair retry
- Token limit overflow: automatic window bisection and rerun
- Partial window failures: continue remaining windows and summarize unresolved ones

## 10. Integration Rules

- Keep existing Tauri command names stable
- Insert translation agent loop between ASR completion and current SRT generation step
- Keep existing SRT generator interfaces compatible by replacing only segment text
- Do not move business logic to frontend

## 11. MVP Acceptance Criteria

Functional:
- EN ASR input consistently produces ZH subtitle text and reaches existing SRT export

Quality:
- Glossary consistency meets target threshold (e.g., >=95% on glossary-hit segments)
- Readability improves versus literal baseline (shorter average cue length, fewer overlong cues)

Reliability:
- Single window failure does not fail full task
- Final deliverable always exportable (with review flag when needed)

Compatibility:
- No breaking changes to existing frontend invoke command names

## 12. Out of Scope (MVP)

- Cross-video persistent memory
- External online retrieval / fact lookup
- Multi-language bidirectional routing
- Human-in-the-loop UI review panel redesign

## 13. Risks and Follow-ups

Main risks:
- Cost/latency growth with 3-round loops
- Threshold tuning for reviewer pass/fail sensitivity
- Structured prompt drift across different OAI-compatible providers

Recommended next step:
- Move to implementation planning via writing-plans workflow
