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
/// * `on_progress` – sync callback fired from the **serial** join loop only,
///   never from a concurrent worker. The third argument carries a reference
///   to the result that just completed (`None` for the seed/initial calls
///   that have no single new result). Because it runs on the join loop,
///   callers can safely accumulate partial state here without locking.
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
    P: Fn(usize, usize, Option<&R>) + Send + Sync + 'static,
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
        on_progress(total, total, None);
        out.sort_by_key(|(index, _)| *index);
        return out;
    }

    on_progress(out.len(), total, None);

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
        // Resolve the join, then fan out: report progress (borrowing a
        // reference to the result) BEFORE pushing it into `out`, since the
        // progress closure runs on this serial loop and only needs a borrow.
        match joined {
            Ok((idx, Ok(val))) => {
                done += 1;
                on_progress(done, total, Some(&val));
                out.push((idx, Ok(val)));
            }
            Ok((idx, Err(e))) => {
                done += 1;
                on_progress(done, total, None);
                out.push((idx, Err(e)));
            }
            Err(err) => {
                done += 1;
                on_progress(done, total, None);
                out.push((
                    usize::MAX,
                    Err(join_error(format!("task join error: {err}"))),
                ));
            }
        }
    }
    out.sort_by_key(|(index, _)| *index);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // Worker that doubles its input; yields once so completion order is not
    // trivially identical to spawn order, exercising the serial-progress path.
    fn double_worker(item: usize) -> impl Future<Output = Result<usize, String>> {
        async move {
            tokio::task::yield_now().await;
            Ok(item * 2)
        }
    }

    #[tokio::test]
    async fn progress_callback_receives_each_result_and_counts() {
        let items = vec![1usize, 2, 3];
        let item_count = items.len();
        let seen: Arc<std::sync::Mutex<Vec<(usize, usize, Option<usize>)>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen_clone = seen.clone();

        let _out = run_indexed_concurrent_idempotent::<_, _, _, _, _, _, _, _>(
            items,
            2,
            move |item| double_worker(item),
            |msg: String| msg,
            move |done, total, result: Option<&usize>| {
                seen_clone
                    .lock()
                    .unwrap()
                    .push((done, total, result.copied()));
            },
            HashMap::new(),
            |_idx, _val| async { Ok(()) },
        )
        .await;

        let recorded = seen.lock().unwrap().clone();
        // One initial seed call (None) + one per item.
        assert_eq!(recorded.len(), item_count + 1);
        // The seed call carries no result.
        assert_eq!(recorded[0].2, None);
        // Every subsequent call carries a concrete result.
        assert!(recorded[1..].iter().all(|(_, _, r)| r.is_some()));
        // `done` and `total` are monotonic and consistent.
        assert_eq!(recorded.last().unwrap().0, recorded.last().unwrap().1);
        // All doubled results appear exactly once.
        let mut results: Vec<_> = recorded[1..]
            .iter()
            .map(|(_, _, r)| r.unwrap())
            .collect();
        results.sort_unstable();
        assert_eq!(results, vec![2, 4, 6]);
    }

    #[tokio::test]
    async fn failed_worker_reports_progress_without_result_but_still_completes() {
        let items = vec![1usize, 2];
        let seen: Arc<std::sync::Mutex<Vec<Option<usize>>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen_clone = seen.clone();

        let out = run_indexed_concurrent_idempotent::<_, usize, String, _, _, _, _, _>(
            items,
            1,
            |item| async move {
                if item == 2 {
                    Err("boom".to_string())
                } else {
                    Ok(item)
                }
            },
            |msg: String| msg,
            move |_done, _total, result: Option<&usize>| {
                seen_clone.lock().unwrap().push(result.copied());
            },
            HashMap::new(),
            |_idx, _val| async { Ok(()) },
        )
        .await;

        // The successful item carries a result; the failed one carries None.
        assert!(seen.lock().unwrap().iter().any(|r| r == &Some(1)));
        assert!(seen.lock().unwrap().iter().any(|r| r.is_none()));
        // The final results vector has one Ok and one Err.
        assert_eq!(out.len(), 2);
        assert!(out.iter().any(|(_, r)| r.is_ok()));
        assert!(out.iter().any(|(_, r)| r.is_err()));
    }
}
