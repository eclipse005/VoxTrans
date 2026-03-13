# AGENTS.md

## Agent Adaptation

If any part of this file is outdated, update it to match the repository before starting work.
Repository reality takes precedence over this document.

## Purpose

This file defines how AI agents should operate in the `voxtrans` repository.
Use it as the default execution guide for changes.

## Project Overview

`voxtrans` is a desktop app for media transcription (and future subtitle translation), built with:

- Tauri v2 desktop shell (`src-tauri/`)
- React + TypeScript frontend (`src/`)
- Rust transcription core (`voxtrans-core/`)

Current architecture:

- `src/`
- `src/app/`: UI components, reducer state, styles, app-level utilities
- `src/app/hooks/`: workflow hooks for queue, subtitle, workspace, and persistence flows
- `src/features/media/`: media task domain types and helpers
- `src-tauri/src/commands/`: Tauri command entrypoints
- `src-tauri/src/services/`: desktop-side business logic
- `src-tauri/src/services/transcription/`: post-ASR punctuation/hotword/transcription pipeline
- `src-tauri/src/services/translation/`: summary/translate/align/qa pipeline
- `src-tauri/src/prompt_builder.rs`: LLM prompt builders used by desktop services
- `voxtrans-core/`: ASR/transcription core (Parakeet v2, SRT generation)
- `model/`: local model files (not committed)
- `runtime/`: local ONNX Runtime files (not committed)
- `output/`: generated SRT outputs (not committed)

## Tech Stack

- Rust 2024 edition (workspace)
- Tauri `2.x`
- React `19.x`
- TypeScript `5.9`
- Vite `7.x`
- ESLint `9.x`
- `parakeet-rs` for ASR

## Commands

Run commands from the repository root.

- Install frontend deps: `npm install`
- Frontend dev server only: `npm run dev`
- Desktop dev (recommended): `npm run tauri dev`
- Frontend lint: `npm run lint`
- Frontend build: `npm run build`
- Desktop release bundle: `npm run tauri build`
- Rust check (desktop crate): `cargo check -p voxtrans-desktop`
- Rust check (core crate): `cargo check -p voxtrans-core`
- Rust check (workspace): `cargo check --workspace`

## Workflow

- For non-trivial tasks, outline a short plan before editing.
- Keep changes focused and minimal; avoid unrelated refactors.
- Prefer the simplest solution that fixes the root cause.
- Preserve the current single-repo structure.
- Prefer reusing existing components/utilities before adding new abstractions.
- When changing Tauri command payloads, update both:
  - Rust request/response structs in the owning service/domain module (for example `src-tauri/src/services/translation/domain.rs`)
  - Corresponding TS types at the frontend call site (for example `src/app/hooks/useQueueWorkflow.ts`, `src/app/types.ts`, or shared types in `src/features/media/types.ts`)

## Rules

- Do not break existing command names used by frontend `invoke` unless explicitly requested.
- Keep business logic in `voxtrans-core`; keep UI concerns in `src/app`.
- Do not rename files, move modules, or reshape public APIs unless required by the task.
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
3. `cargo check -p voxtrans-desktop`

If core logic changed, also run:

4. `cargo check -p voxtrans-core`

Practical scope examples:

- Frontend-only changes: run `npm run lint` and `npm run build`
- Tauri/Rust changes: also run `cargo check -p voxtrans-desktop`
- Core transcription logic changes: also run `cargo check -p voxtrans-core`

If any required step fails, fix the issue before finishing.
If a check cannot be run in the current environment, state that clearly.

## Output Expectations

When completing work, report:

- What changed
- Which files were affected
- Which verification commands were run and their results
- Any follow-up risks or recommended next steps
