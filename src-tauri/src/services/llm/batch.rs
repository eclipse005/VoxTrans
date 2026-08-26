use std::collections::{HashMap, HashSet};
use std::future::Future;

/// Run items in index order. Item `i` starts only after item `i-1` has
/// finished (or was precomputed). Precomputed indices skip the worker.
///
/// Use this when each item's input depends on prior items' outputs — translation
/// batch N reads bilingual previousLines from batch N-1, so those calls cannot
/// overlap.
pub async fn run_indexed_chained_idempotent<T, R, E, F, Fut, P, D, DFut>(
    items: Vec<T>,
    worker: F,
    join_error: impl Fn(String) -> E,
    on_progress: P,
    precomputed: HashMap<usize, R>,
    on_item_done: D,
) -> Vec<(usize, Result<R, E>)>
where
    T: Send,
    R: Clone + Send,
    E: Send,
    F: Fn(T) -> Fut,
    Fut: Future<Output = Result<R, E>>,
    P: Fn(usize, usize, Option<&R>),
    D: Fn(usize, R) -> DFut,
    DFut: Future<Output = Result<(), String>>,
{
    let total = items.len();
    let skip: HashSet<usize> = precomputed.keys().copied().collect();
    let mut out: Vec<(usize, Result<R, E>)> = Vec::with_capacity(total.max(precomputed.len()));

    for (idx, result) in precomputed {
        out.push((idx, Ok(result)));
    }

    if items.is_empty() {
        on_progress(total, total, None);
        out.sort_by_key(|(index, _)| *index);
        return out;
    }

    let mut done = out.len();
    on_progress(done, total.max(done), None);

    for (batch_idx, item) in items.into_iter().enumerate() {
        if skip.contains(&batch_idx) {
            continue;
        }
        match worker(item).await {
            Ok(val) => {
                if let Err(e) = on_item_done(batch_idx, val.clone()).await {
                    done += 1;
                    on_progress(done, total.max(done), None);
                    out.push((
                        batch_idx,
                        Err(join_error(format!("persist unit result failed: {e}"))),
                    ));
                    continue;
                }
                done += 1;
                on_progress(done, total.max(done), Some(&val));
                out.push((batch_idx, Ok(val)));
            }
            Err(e) => {
                done += 1;
                on_progress(done, total.max(done), None);
                out.push((batch_idx, Err(e)));
            }
        }
    }

    out.sort_by_key(|(index, _)| *index);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[tokio::test]
    async fn chained_later_item_observes_predecessor() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let last = Arc::new(AtomicUsize::new(0));
        let last_for_worker = last.clone();
        let items = vec![1usize, 2, 3];
        let out = run_indexed_chained_idempotent::<_, _, _, _, _, _, _, _>(
            items,
            move |item| {
                let last = last_for_worker.clone();
                async move {
                    let prev = last.load(Ordering::SeqCst);
                    assert_eq!(prev, item - 1, "batch {item} started before {prev} finished");
                    last.store(item, Ordering::SeqCst);
                    Ok::<usize, String>(item)
                }
            },
            |msg: String| msg,
            |_done, _total, _result: Option<&usize>| {},
            HashMap::new(),
            |_idx, _val| async { Ok(()) },
        )
        .await;
        assert_eq!(out.len(), 3);
        assert!(out.iter().all(|(_, r)| r.is_ok()));
        assert_eq!(last.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn chained_skips_precomputed_indices() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_worker = calls.clone();
        let mut precomputed = HashMap::new();
        precomputed.insert(0usize, 100usize);
        let items = vec![10usize, 20, 30];
        let out = run_indexed_chained_idempotent::<_, _, _, _, _, _, _, _>(
            items,
            move |item| {
                let calls = calls_for_worker.clone();
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok::<usize, String>(item)
                }
            },
            |msg: String| msg,
            |_done, _total, _result: Option<&usize>| {},
            precomputed,
            |_idx, _val| async { Ok(()) },
        )
        .await;
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        let mut got: Vec<_> = out
            .into_iter()
            .map(|(idx, r)| (idx, r.expect("ok")))
            .collect();
        got.sort_by_key(|(idx, _)| *idx);
        assert_eq!(got, vec![(0, 100), (1, 20), (2, 30)]);
    }
}
