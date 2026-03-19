# AGENTS.md

## Agent Adaptation

If any part of this file is outdated, update it to match the repository before starting work.
Repository reality takes precedence over this document.

## Purpose

This file defines how AI agents should operate in the `voxtrans` repository.
Use it as the default execution guide for changes.

## Local Mode

- This repository is maintained in single-developer local mode.
- Do not add or enforce team-collaboration process requirements in outputs.
- Focus on direct bug-fix and feature-delivery execution: locate issue, implement fix, verify, report.

## Project Overview

`voxtrans` is a desktop app for media transcription, translation, and subtitle editing, built with:

- Tauri v2 desktop shell (`src-tauri/`)
- React + TypeScript frontend (`src/`)
- Rust transcription core (`voxtrans-core/`)
- Rig (`rig-core`) as the LLM/Agent node integration layer on desktop services

Current architecture:

- `src/`
- `src/app/`: UI components, reducer state, styles, app-level utilities
- `src/app/api/`: frontend invoke/API wrappers for desktop commands
- `src/app/hooks/`: workflow hooks for queue, subtitle, workspace, and persistence flows
- `src/app/state/`: app/queue/settings/subtitle reducers and domain actions
- `src/features/media/`: media task domain types and helpers
- `src-tauri/src/commands/`: Tauri command entrypoints
- `src-tauri/src/db/`: SQLite setup and migration wiring
- `src-tauri/src/services/`: desktop-side business logic
- `src-tauri/src/services/task_engine.rs`: task lifecycle query/enqueue services (register upload / enqueue / list / get)
- `src-tauri/src/services/task_executor.rs`: task execution orchestration (single/batch enqueue+execute)
- `src-tauri/src/services/task_worker.rs`: worker-process runtime management (spawn/wait/kill)
- `src-tauri/src/services/transcription/`: post-ASR punctuation/hotword/transcription pipeline
- `src-tauri/src/services/translate/`: translation pipeline modules (Rig adapters/prompt/rules/validation)
- `src-tauri/src/services/translate/adapters/rig_node.rs`: shared Rig node client for JSON-constrained LLM calls
- `src-tauri/src/services/demucs/`: vocal separation services
- `voxtrans-core/`: ASR/transcription core (Parakeet v2, SRT generation)
- `model/`: local model files (not committed; may be absent before first setup)
- `runtime/`: local ONNX Runtime files (not committed; may be absent before first setup)
- `output/`: generated SRT outputs (not committed; may be absent before first export)

## Tech Stack

- Rust 2024 edition (workspace)
- Tauri `2.10.x`
- React `19.2.x`
- TypeScript `5.9.x`
- Vite `7.3.x`
- ESLint `9.39.x`
- `parakeet-rs` for ASR
- `rig-core` for provider-agnostic LLM access and concurrent node calls

## Commands

Run commands from the repository root.

- Install frontend deps: `npm install`
- Frontend dev server only: `npm run dev`
- Desktop dev (recommended): `npm run tauri dev`
- Frontend lint: `npm run lint`
- Frontend build: `npm run build`
- Desktop release bundle: `npm run tauri build`
- Rust check (desktop crate): `cargo check -p voxtrans`
- Rust check (core crate): `cargo check -p voxtrans-core`
- Rust check (workspace): `cargo check --workspace`

## Workflow

- For non-trivial tasks, outline a short plan before editing.
- Keep changes focused and minimal; avoid unrelated refactors.
- Prefer the simplest solution that fixes the root cause.
- Preserve the current single-repo structure.
- Prefer reusing existing components/utilities before adding new abstractions.
- Task lifecycle is command-driven:
  - Frontend sends commands only (`register_task_upload`, `enqueue_task_run`, `enqueue_and_execute_task_batch`, `delete_task_summaries`)
  - Backend (`task_runs`) is the source of truth and single writer for task lifecycle state
  - Frontend is a projection/read model; do not re-introduce frontend-owned queue lifecycle persistence
- When changing Tauri command payloads, update both:
  - Rust request/response structs in the owning service/domain module (for example `src-tauri/src/services/transcribe.rs`)
  - Corresponding TS types at the frontend call site (for example `src/app/hooks/useQueueWorkflow.ts`, `src/app/types.ts`, or shared types in `src/features/media/types.ts`)
- When changing task state fields or execution semantics, update all affected projections:
  - `task_engine` / `task_executor`
  - `workspace` loader/projection
  - frontend queue normalization/recovery logic

## Rules

- Do not break existing command names used by frontend `invoke` unless explicitly requested.
- Keep business logic in `voxtrans-core`; keep UI concerns in `src/app`.
- Do not rename files, move modules, or reshape public APIs unless required by the task.
- New LLM-facing integration should go through Rig node adapters; do not add a parallel ad-hoc LLM client stack.
- For punctuation/translation node calls, prefer `RigNodeClient` with explicit JSON validation and bounded concurrency.
- Do not add new dependence on `save_queue_state` for task lifecycle control; use task-engine commands.
- Do not commit generated/runtime artifacts:
  - `dist/`, `target/`, `output/`, `src-tauri/output/`
  - local model/runtime binaries in `model/` and `runtime/`
- Preserve existing Chinese user-facing copy unless the task explicitly requests copy changes.
- Keep new UI text concise and consistent with the current product tone.
- For long-running transcription tasks, avoid blocking UI; prefer async/background execution patterns.

## Verification

Before finishing a code change, run the relevant checks for the areas touched:

1. `npm run lint`
2. `npm run build`
3. `cargo check -p voxtrans`

If core logic changed, also run:

4. `cargo check -p voxtrans-core`

Practical scope examples:

- Frontend-only changes: run `npm run lint` and `npm run build`
- Tauri/Rust changes: also run `cargo check -p voxtrans`
- Core transcription logic changes: also run `cargo check -p voxtrans-core`

If any required step fails, fix the issue before finishing.
If a check cannot be run in the current environment, state that clearly.

## Output Expectations

When completing work, report:

- What changed
- Which files were affected
- Which verification commands were run and their results
- Any follow-up risks or recommended next steps
