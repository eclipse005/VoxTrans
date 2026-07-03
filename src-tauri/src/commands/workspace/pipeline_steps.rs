mod recognition;
mod translation;

pub(super) use recognition::{Step1AsrPipelineStep, Step2SegmentsPipelineStep};
pub(super) use translation::{Step3TerminologyPipelineStep, Step4TranslationPipelineStep};

/// Run a future from a sync callback that fires on a worker thread (e.g. the
/// ASR/align progress callback, which runs on the LLM HTTP client thread or
/// inside `spawn_blocking`). That thread may itself be a tokio multi-thread
/// runtime worker, where a plain `block_on` would panic — `block_in_place` is
/// the safe path there. Falls back to `tauri::async_runtime::block_on`
/// otherwise, so the future always runs regardless of caller context.
///
/// This is the canonical helper for "I'm in a sync callback and need to await
/// something" — use it instead of hand-rolling `Handle::try_current()` checks,
/// which tend to silently skip work when the caller is not on a multi-thread
/// runtime worker (see the recognition.rs save_* bug this centralizes).
pub(super) fn block_on_runtime_worker<F>(fut: F)
where
    F: std::future::Future,
{
    match tokio::runtime::Handle::try_current() {
        Ok(handle)
            if handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread =>
        {
            let _ = tokio::task::block_in_place(|| handle.block_on(fut));
        }
        _ => {
            let _ = tauri::async_runtime::block_on(fut);
        }
    }
}
