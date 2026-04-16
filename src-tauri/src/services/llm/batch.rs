use std::future::Future;
use std::sync::Arc;

use tokio::sync::Semaphore;
use tokio::task::JoinSet;

pub async fn run_indexed_concurrent<T, R, E, F, Fut>(
    items: Vec<T>,
    concurrency: usize,
    worker: F,
    join_error: impl Fn(String) -> E + Clone + Send + 'static,
) -> Vec<(usize, Result<R, E>)>
where
    T: Send + 'static,
    R: Send + 'static,
    E: Send + 'static,
    F: Fn(T) -> Fut + Clone + Send + 'static,
    Fut: Future<Output = Result<R, E>> + Send + 'static,
{
    if items.is_empty() {
        return Vec::new();
    }

    let concurrency = concurrency.max(1);
    let semaphore = Arc::new(Semaphore::new(concurrency));
    let mut join_set: JoinSet<(usize, Result<R, E>)> = JoinSet::new();

    for (index, item) in items.into_iter().enumerate() {
        let semaphore = Arc::clone(&semaphore);
        let worker = worker.clone();
        let join_error = join_error.clone();
        join_set.spawn(async move {
            let permit = semaphore.acquire_owned().await;
            let _permit = match permit {
                Ok(v) => v,
                Err(err) => {
                    return (
                        index,
                        Err(join_error(format!("semaphore acquire failed: {err}"))),
                    );
                }
            };
            (index, worker(item).await)
        });
    }

    let mut out = Vec::new();
    while let Some(joined) = join_set.join_next().await {
        match joined {
            Ok(v) => out.push(v),
            Err(err) => out.push((
                usize::MAX,
                Err(join_error(format!("task join error: {err}"))),
            )),
        }
    }
    out.sort_by_key(|(index, _)| *index);
    out
}
