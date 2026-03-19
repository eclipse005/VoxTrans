# Task Engine Design

## Goals

- Move workflow orchestration to backend.
- Use one canonical task model for transcribe / transcribe+translate / translate-only.
- Keep frontend as intent trigger + read model renderer.
- Support step-level setting binding (`SNAPSHOT_AT_TASK_START` / `LIVE_AT_STEP_START`).

## Canonical Model

### Task Run

- `id`: task id
- `intent`: `TRANSCRIBE` | `TRANSCRIBE_TRANSLATE` | `TRANSLATE_ONLY`
- `state`: `CREATED` | `QUEUED` | `RUNNING` | `PAUSED` | `FAILED` | `COMPLETED` | `CANCELLED`
- `current_step`: current step key
- `progress_percent`: 0..100
- `error_code` / `error_message`
- `settings_snapshot_json`: task-level baseline snapshot
- `source_lang` / `target_lang`
- timestamps

### Step Run

- `task_id` + `step` + `attempt`
- `status`: `PENDING` | `RUNNING` | `COMPLETED` | `FAILED` | `SKIPPED` | `CANCELLED`
- `binding_mode`: `SNAPSHOT_AT_TASK_START` | `LIVE_AT_STEP_START`
- `input_hash`: idempotency input key
- `settings_snapshot_json`: effective settings used by this step
- `diagnostics_json`: timings/metrics/tool info
- `error_code` / `error_message`

### Artifacts

- `task_id`, `kind`, `path`
- `produced_by_step`
- `checksum`, `size_bytes`, `mime_type`
- `metadata_json`

## Intent Pipelines

### `TRANSCRIBE`

1. `separate` (optional)
2. `asr`
3. `punctuate` (optional)
4. `segment`
5. `render_source_srt`
6. `persist`

### `TRANSCRIBE_TRANSLATE`

1. `separate` (optional)
2. `asr`
3. `punctuate` (optional)
4. `segment`
5. `translate`
6. `render_target_srt`
7. `persist`

### `TRANSLATE_ONLY`

1. `load_source_segments`
2. `translate`
3. `render_target_srt`
4. `persist`

## Setting Binding Policy

- `asr`: `LIVE_AT_STEP_START`
- `punctuate`: `LIVE_AT_STEP_START`
- `segment`: `LIVE_AT_STEP_START`
- `translate`: `LIVE_AT_STEP_START`
- `render/persist`: `SNAPSHOT_AT_TASK_START`

This ensures changes before a step starts are applied to the current running task.

## Idempotency

- Each step computes `input_hash` from upstream artifact checksums + effective setting snapshot.
- If same `input_hash` with completed output exists, step can be skipped.

## Frontend Contract

- `enqueue_task_run`
- `list_task_runs`
- `get_task_run`
- `cancel_task_run`

Frontend should stop owning flow state machine and only consume task read model.

