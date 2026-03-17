# Agentic EN->ZH Translation Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a fully autonomous, single-video EN->ZH translation agent loop between ASR completion and SRT generation, using an OAI-compatible cloud API.

**Architecture:** Keep existing Tauri command names and queue workflow unchanged at boundaries. Insert a new backend post-ASR agent loop (`Planner -> Translator <-> Reviewer`, max 3 rounds) inside `services/transcription`, then feed accepted Chinese text back into existing segment/SRT output structures. Frontend adds translation settings (`apiKey/baseUrl/model/glossary/strictTerminology/windowSize`) and passes them through existing `run_post_asr_pipeline` invoke payload.

**Tech Stack:** Rust (Tauri v2 backend), TypeScript/React frontend, `reqwest` JSON calls to OAI-compatible APIs, existing queue + preferences persistence.

---

## File Structure Map

Backend (new):
- Create: `src-tauri/src/services/transcription/agent/mod.rs`
- Create: `src-tauri/src/services/transcription/agent/types.rs`
- Create: `src-tauri/src/services/transcription/agent/oai_compatible_client.rs`
- Create: `src-tauri/src/services/transcription/agent/planner_agent.rs`
- Create: `src-tauri/src/services/transcription/agent/translator_agent.rs`
- Create: `src-tauri/src/services/transcription/agent/reviewer_agent.rs`
- Create: `src-tauri/src/services/transcription/agent/finalizer.rs`
- Create: `src-tauri/src/services/transcription/agent/orchestrator.rs`

Backend (modify):
- Modify: `src-tauri/src/services/transcription/mod.rs`
- Modify: `src-tauri/src/services/transcription/pipeline.rs`
- Modify: `src-tauri/src/commands/transcription.rs` (phase names only if needed)
- Modify: `src-tauri/src/services/preferences.rs`

Frontend (modify):
- Modify: `src/features/media/types.ts`
- Modify: `src/features/media/stateMachine.ts`
- Modify: `src/app/state/appReducer.ts`
- Modify: `src/app/hooks/useSettingsController.ts`
- Modify: `src/app/components/SettingsModal.tsx`
- Modify: `src/app/api/transcribe.ts`
- Modify: `src/app/hooks/queue/useQueueRunner.ts`
- Modify: `src/app/state/queueDomainActions.ts`
- Modify: `src/app/hooks/useWorkspacePersistence.ts`
- Modify: `src/app/components/MediaList.tsx`

Testing:
- Create: `src-tauri/src/services/transcription/agent/tests.rs` (or `#[cfg(test)]` blocks in new agent files)
- Modify/add: lightweight frontend type-level/logic coverage if test harness exists; otherwise validate with `npm run lint` + `npm run build`

Docs:
- Modify: `AGENTS.md` only if architecture section must be updated to reflect new translation-agent modules.

## Task 1: Define Agent Contracts and Configuration Model

**Files:**
- Create: `src-tauri/src/services/transcription/agent/types.rs`
- Modify: `src-tauri/src/services/transcription/agent/mod.rs`
- Test: `src-tauri/src/services/transcription/agent/types.rs` (`#[cfg(test)]`)

- [ ] **Step 1: Write failing Rust tests for clamp/default rules**

```rust
#[test]
fn translation_config_clamps_window_and_rounds() {
    let cfg = TranslationAgentConfig::new(5, 99, true);
    assert_eq!(cfg.window_size, 25);
    assert_eq!(cfg.max_rounds, 3);
    assert!(cfg.strict_terminology);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p voxtrans translation_config_clamps_window_and_rounds -- --exact`
Expected: FAIL with missing `TranslationAgentConfig`.

- [ ] **Step 3: Implement `types.rs` minimal structs**

```rust
pub struct TranslationAgentConfig {
    pub window_size: usize,
    pub max_rounds: u8,
    pub strict_terminology: bool,
}

impl TranslationAgentConfig {
    pub fn new(window_size: usize, max_rounds: u8, strict_terminology: bool) -> Self {
        Self {
            window_size: window_size.clamp(25, 40),
            max_rounds: max_rounds.clamp(1, 3),
            strict_terminology,
        }
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p voxtrans translation_config_clamps_window_and_rounds -- --exact`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/services/transcription/agent/mod.rs src-tauri/src/services/transcription/agent/types.rs
git commit -m "feat(transcription): add agent config and core contracts"
```

## Task 2: Implement OAI-Compatible Client with Retry/Error Classification

**Files:**
- Create: `src-tauri/src/services/transcription/agent/oai_compatible_client.rs`
- Modify: `src-tauri/src/services/transcription/agent/mod.rs`
- Test: `src-tauri/src/services/transcription/agent/oai_compatible_client.rs` (`#[cfg(test)]` for parse/error mapping)

- [ ] **Step 1: Write failing tests for response parsing and non-200 errors**
- [ ] **Step 2: Run tests and confirm failures**

Run: `cargo test -p voxtrans oai_client_ -- --nocapture`
Expected: FAIL due to missing parser/client.

- [ ] **Step 3: Implement minimal request/response structs and parser**
- [ ] **Step 4: Add retry policy helper (`max 3`, exponential backoff + jitter placeholder hook)**
- [ ] **Step 5: Re-run tests to green**

Run: `cargo test -p voxtrans oai_client_ -- --nocapture`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/services/transcription/agent/mod.rs src-tauri/src/services/transcription/agent/oai_compatible_client.rs
git commit -m "feat(transcription): add OAI-compatible translation client"
```

## Task 3: Implement Planner Agent (Single-Video Plan Generation)

**Files:**
- Create: `src-tauri/src/services/transcription/agent/planner_agent.rs`
- Modify: `src-tauri/src/services/transcription/agent/mod.rs`
- Test: `src-tauri/src/services/transcription/agent/planner_agent.rs` (`#[cfg(test)]`)

- [ ] **Step 1: Write failing tests for glossary lock + style policy defaults**
- [ ] **Step 2: Run tests and confirm failure**

Run: `cargo test -p voxtrans planner_agent_ -- --nocapture`
Expected: FAIL due to missing planner.

- [ ] **Step 3: Implement planner prompt input builder with sampled subtitle context**
- [ ] **Step 4: Implement `TranslationPlan` output validation (required fields, non-empty policies)**
- [ ] **Step 5: Re-run tests**

Run: `cargo test -p voxtrans planner_agent_ -- --nocapture`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/services/transcription/agent/mod.rs src-tauri/src/services/transcription/agent/planner_agent.rs
git commit -m "feat(transcription): add planner agent for EN->ZH subtitle strategy"
```

## Task 4: Implement Translator and Reviewer Agents

**Files:**
- Create: `src-tauri/src/services/transcription/agent/translator_agent.rs`
- Create: `src-tauri/src/services/transcription/agent/reviewer_agent.rs`
- Modify: `src-tauri/src/services/transcription/agent/mod.rs`
- Test: `src-tauri/src/services/transcription/agent/translator_agent.rs`
- Test: `src-tauri/src/services/transcription/agent/reviewer_agent.rs`

- [ ] **Step 1: Write failing translator test for segment-id-preserving draft output**
- [ ] **Step 2: Write failing reviewer test for hard-fail on strict glossary mismatch**
- [ ] **Step 3: Run both tests and confirm failures**

Run: `cargo test -p voxtrans translator_agent_ -- --nocapture`
Expected: FAIL (missing translator agent/logic).

Run: `cargo test -p voxtrans reviewer_agent_ -- --nocapture`
Expected: FAIL (missing reviewer agent/logic).

- [ ] **Step 4: Implement translator draft schema + confidence fields**
- [ ] **Step 5: Implement reviewer scoring (`readability`, `terminology`, `fidelity`) and issue list**
- [ ] **Step 6: Re-run both tests**

Run: `cargo test -p voxtrans translator_agent_ -- --nocapture`
Expected: PASS.

Run: `cargo test -p voxtrans reviewer_agent_ -- --nocapture`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/services/transcription/agent/mod.rs src-tauri/src/services/transcription/agent/translator_agent.rs src-tauri/src/services/transcription/agent/reviewer_agent.rs
git commit -m "feat(transcription): add translator/reviewer agent loop components"
```

## Task 5: Implement Orchestrator + Finalizer and Insert Into Post-ASR Pipeline

**Files:**
- Create: `src-tauri/src/services/transcription/agent/orchestrator.rs`
- Create: `src-tauri/src/services/transcription/agent/finalizer.rs`
- Modify: `src-tauri/src/services/transcription/mod.rs`
- Modify: `src-tauri/src/services/transcription/pipeline.rs`
- Modify: `src-tauri/src/commands/transcription.rs`
- Test: `src-tauri/src/services/transcription/agent/orchestrator.rs`
- Test: `src-tauri/src/services/transcription/pipeline.rs`

- [ ] **Step 1: Write failing orchestrator test for `max_rounds=3` stop condition**
- [ ] **Step 2: Write failing pipeline test for preserving original timestamps/order while replacing text**
- [ ] **Step 3: Run tests and confirm failures**

Run: `cargo test -p voxtrans orchestrator_ -- --nocapture`
Expected: FAIL before implementation.

Run: `cargo test -p voxtrans post_asr_pipeline_ -- --nocapture`
Expected: FAIL before implementation.

- [ ] **Step 4: Implement orchestrator state machine and window splitter (`25-40`, default 32)**
- [ ] **Step 5: Implement fallback behavior (`needs_manual_review=true`, keep best-scored round)**
- [ ] **Step 6: Wire orchestrator into `run_post_asr_pipeline` between beautify and segment->SRT handoff**
- [ ] **Step 7: Emit new phase markers (e.g. `planning`, `translating`, `reviewing`, `segment`) without renaming existing command**
- [ ] **Step 8: Re-run tests**

Run: `cargo test -p voxtrans orchestrator_ -- --nocapture`
Expected: PASS.

Run: `cargo test -p voxtrans post_asr_pipeline_ -- --nocapture`
Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add src-tauri/src/services/transcription/mod.rs src-tauri/src/services/transcription/pipeline.rs src-tauri/src/commands/transcription.rs src-tauri/src/services/transcription/agent/orchestrator.rs src-tauri/src/services/transcription/agent/finalizer.rs
git commit -m "feat(transcription): integrate agentic EN->ZH loop into post-ASR pipeline"
```

## Task 6: Persist and Validate Translation Settings in Backend Preferences

**Files:**
- Modify: `src-tauri/src/services/preferences.rs`
- Modify: `src-tauri/src/commands/preferences.rs` (if struct exposure changes)
- Test: `src-tauri/src/services/preferences.rs` (`#[cfg(test)]` for default/clamp)

- [ ] **Step 1: Write failing tests for new settings defaults and clamps (`windowSize 25..40`)**
- [ ] **Step 2: Run tests and confirm failure**

Run: `cargo test -p voxtrans preferences_ -- --nocapture`
Expected: FAIL for missing fields/keys.

- [ ] **Step 3: Add settings keys and serde fields (`translationApiKey`, `translationBaseUrl`, `translationModel`, `translationWindowSize`, `translationStrictTerminology`, `translationGlossary`)**
- [ ] **Step 4: Implement load/save migration-safe defaults**
- [ ] **Step 5: Re-run tests**

Run: `cargo test -p voxtrans preferences_ -- --nocapture`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/services/preferences.rs src-tauri/src/commands/preferences.rs
git commit -m "feat(settings): persist translation agent configuration"
```

## Task 7: Frontend Type and Settings UI Wiring

**Files:**
- Modify: `src/features/media/types.ts`
- Modify: `src/features/media/stateMachine.ts`
- Modify: `src/app/state/appReducer.ts`
- Modify: `src/app/hooks/useSettingsController.ts`
- Modify: `src/app/components/SettingsModal.tsx`
- Modify: `src/app/api/transcribe.ts`
- Modify: `src/app/hooks/queue/useQueueRunner.ts`
- Modify: `src/app/state/queueDomainActions.ts`
- Modify: `src/app/hooks/useWorkspacePersistence.ts`
- Modify: `src/app/components/MediaList.tsx`

- [ ] **Step 1: Add failing TS compile expectations by introducing new required settings fields in `SavedSettings`**
- [ ] **Step 2: Run build and capture failures**

Run: `npm run build`
Expected: FAIL with missing new settings wiring.

- [ ] **Step 3: Update reducers/default state/draft state for translation settings**
- [ ] **Step 4: Add settings controls in modal (保持中文文案简洁) and save validation**
- [ ] **Step 5: Extend `runPostAsrPipeline` payload and `useQueueRunner` callsite**
- [ ] **Step 6: Run lint + build to verify frontend consistency**

Run: `npm run lint`
Expected: PASS.

Run: `npm run build`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src/features/media/types.ts src/features/media/stateMachine.ts src/app/state/appReducer.ts src/app/hooks/useSettingsController.ts src/app/components/SettingsModal.tsx src/app/api/transcribe.ts src/app/hooks/queue/useQueueRunner.ts src/app/state/queueDomainActions.ts src/app/hooks/useWorkspacePersistence.ts src/app/components/MediaList.tsx
git commit -m "feat(frontend): wire translation-agent settings and post-ASR request payload"
```

## Task 8: End-to-End Verification and Guardrails

**Files:**
- Modify: `AGENTS.md` (only if architecture text must reflect new modules)
- Optional create: `docs/superpowers/plans/verification-notes-2026-03-18.md`

- [ ] **Step 1: Run backend checks required by repo policy**

Run: `cargo check -p voxtrans`
Expected: PASS.

- [ ] **Step 2: Run core check only if `voxtrans-core` changed**

Run: `cargo check -p voxtrans-core`
Expected: PASS or SKIP (if untouched).

- [ ] **Step 3: Run frontend checks required by repo policy**

Run: `npm run lint`
Expected: PASS.

Run: `npm run build`
Expected: PASS.

- [ ] **Step 4: Smoke test one EN input with glossary and strict mode enabled**

Manual expected:
- post-ASR phases include planning/translating/reviewing/segment
- output SRT generated successfully
- glossary terms are consistent
- timestamps remain unchanged

- [ ] **Step 5: Commit final docs adjustments (if any)**

```bash
git add AGENTS.md docs/superpowers/plans/verification-notes-2026-03-18.md
git commit -m "docs: update architecture notes and verification record"
```

## Execution Notes

- Follow @superpowers/test-driven-development for each task loop (fail -> minimal pass -> refactor).
- Use frequent, scoped commits; avoid mixed-purpose commits.
- Keep command names stable (`run_post_asr_pipeline` unchanged).
- Keep business logic in Tauri backend; frontend only handles config/input/output wiring.
- Do not add cross-video memory or online retrieval in MVP.
