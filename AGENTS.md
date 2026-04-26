# AGENTS.md

Guidelines for Codex and other coding agents working in this repository.

## Working Style

- Prefer direct progress over over-discussion. Make reasonable assumptions, state them briefly, and keep moving.
- Ask only when the decision has meaningful product or codebase impact.
- Keep diffs surgical. Every changed line should map to the user request.
- Match the existing style and structure of the touched area.

## Simplicity First

- Implement the smallest change that fully solves the task.
- Do not add speculative abstractions, configuration, or compatibility layers.
- Avoid "future-proofing" unless the user explicitly asks for it.
- Prefer removing no-longer-needed code created by your change, but do not clean unrelated dead code unless requested.

## Project Boundaries

- Prefer editing source files under `src/`, `src-tauri/`, and `voxtrans-core/`.
- Treat `target/`, `dist/`, and `output/` as generated or runtime data unless the task is explicitly about inspecting artifacts.
- Do not modify logs, generated outputs, or history files as part of normal feature work.

## Product Scope

- Voxtrans is a general-purpose audio/video transcription and subtitle translation program. Optimize for broad language and media behavior, not for one sample video, channel, industry, or topic.
- Do not add hard-coded domain fixes, phrase rewrites, terminology substitutions, or prompt assumptions for trading, finance, sports, education, meetings, or any other special field. Domain-specific vocabulary must come from user-provided hotwords/terminology or model output, not from pipeline code.
- Language-aware processing is allowed only when it is genuinely generic language or subtitle behavior, such as CJK spacing, punctuation, text length units, source-residue detection, numeric consistency, or locale-specific formatting. Keep these rules tied to language properties, not content topics.
- Tests should use neutral fixtures unless a domain term is required to prove a generic rule. If a real-world artifact exposes a bug, reduce it to a domain-neutral reproduction before committing the test.
- If improving English-to-Chinese quality, keep the implementation reusable for other source/target pairs. Do not introduce Chinese phrase templates that invent content; prefer validation, segmentation, alignment, prompt constraints, and generic repair rules.

## Execution

- When fixing a bug, first identify the concrete failing behavior, then patch it, then verify it.
- When a task spans multiple stages, keep the implementation aligned with the actual pipeline stages and checkpoint files.
- Prefer targeted verification over broad churn: run the smallest useful check that proves the change.
- If tests or verification cannot be run, say so clearly.

## Safety

- Do not revert unrelated local changes.
- Do not refactor adjacent code unless it is necessary for the requested fix.
- Preserve user-facing workflow and stage semantics unless the task explicitly changes them.

## Communication

- Surface real tradeoffs, but keep recommendations concrete.
- Call out hidden risks such as stale checkpoints, mismatched stage outputs, or UI state getting out of sync with artifacts.
- Favor concise explanations tied to actual files and behavior in this repo.
