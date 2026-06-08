use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;

use tokio::sync::Semaphore;
use tokio::task::JoinSet;

/// Idempotent concurrent batch runner.
///
/// * `precomputed` – results that were already persisted in a previous run;
///   those indices are returned directly without calling `worker`.
/// * `on_item_done` – async callback fired after each successful
///   (non-precomputed) worker completion, giving the caller a chance to
///   persist the result before moving on.
pub async fn run_indexed_concurrent_idempotent<T, R, E, F, Fut, P, D, DFut>(
    items: Vec<T>,
    concurrency: usize,
    worker: F,
    join_error: impl Fn(String) -> E + Clone + Send + 'static,
    on_progress: P,
    precomputed: HashMap<usize, R>,
    on_item_done: D,
) -> Vec<(usize, Result<R, E>)>
where
    T: Send + 'static,
    R: Clone + Send + 'static,
    E: Send + 'static,
    F: Fn(T) -> Fut + Clone + Send + 'static,
    Fut: Future<Output = Result<R, E>> + Send + 'static,
    P: Fn(usize, usize) + Send + Sync + 'static,
    D: Fn(usize, R) -> DFut + Clone + Send + 'static,
    DFut: Future<Output = Result<(), String>> + Send + 'static,
{
    let total = items.len() + precomputed.len();
    let mut out: Vec<(usize, Result<R, E>)> = Vec::new();

    // Seed with precomputed results.
    for (idx, result) in precomputed {
        out.push((idx, Ok(result)));
    }

    if items.is_empty() {
        on_progress(total, total);
        out.sort_by_key(|(index, _)| *index);
        return out;
    }

    on_progress(out.len(), total);

    let concurrency = concurrency.max(1);
    let semaphore = Arc::new(Semaphore::new(concurrency));
    let mut join_set: JoinSet<(usize, Result<R, E>)> = JoinSet::new();

    for (batch_idx, item) in items.into_iter().enumerate() {
        let semaphore = Arc::clone(&semaphore);
        let worker = worker.clone();
        let join_error = join_error.clone();
        let on_item_done = on_item_done.clone();
        join_set.spawn(async move {
            let permit = semaphore.acquire_owned().await;
            let _permit = match permit {
                Ok(v) => v,
                Err(err) => {
                    return (
                        batch_idx,
                        Err(join_error(format!("semaphore acquire failed: {err}"))),
                    );
                }
            };
            let result = worker(item).await;
            match result {
                Ok(val) => {
                    if let Err(e) = on_item_done(batch_idx, val.clone()).await {
                        return (
                            batch_idx,
                            Err(join_error(format!("persist unit result failed: {e}"))),
                        );
                    }
                    (batch_idx, Ok(val))
                }
                Err(e) => (batch_idx, Err(e)),
            }
        });
    }

    let mut done = out.len();
    while let Some(joined) = join_set.join_next().await {
        match joined {
            Ok(v) => out.push(v),
            Err(err) => out.push((
                usize::MAX,
                Err(join_error(format!("task join error: {err}"))),
            )),
        }
        done += 1;
        on_progress(done, total);
    }
    out.sort_by_key(|(index, _)| *index);
    out
}
