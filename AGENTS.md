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
