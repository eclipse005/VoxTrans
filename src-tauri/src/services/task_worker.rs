use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;
use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions, SqliteSynchronous};
use tauri::Emitter;

use crate::app_state::TaskWorkerRuntime;
use crate::services::task_executor::{ExecuteTaskRunRequest, execute_task_run};
use crate::services::task_usage;

const WORKER_ARG: &str = "--voxtrans-worker";
const WORKER_EVENT_PREFIX: &str = "VOXTRANS_EVENT:";

#[derive(Debug, Deserialize)]
struct WorkerEventEnvelope {
    event: String,
    payload: Value,
}

pub fn maybe_run_worker_mode_from_args() -> bool {
    let args = std::env::args().collect::<Vec<_>>();
    if args.len() < 2 || args[1] != WORKER_ARG {
        return false;
    }
    let code = match run_worker_from_args(&args[2..]) {
        Ok(()) => 0,
        Err(err) => {
            eprintln!("{err}");
            1
        }
    };
    std::process::exit(code);
}

fn run_worker_from_args(args: &[String]) -> Result<(), String> {
    let mut task_id = String::new();
    let mut db_path = String::new();
    let mut intent: Option<String> = None;
    let mut idx = 0usize;
    while idx < args.len() {
        match args[idx].as_str() {
            "--task-id" => {
                idx += 1;
                if idx >= args.len() {
                    return Err("--task-id requires value".to_string());
                }
                task_id = args[idx].clone();
            }
            "--db-path" => {
                idx += 1;
                if idx >= args.len() {
                    return Err("--db-path requires value".to_string());
                }
                db_path = args[idx].clone();
            }
            "--intent" => {
                idx += 1;
                if idx >= args.len() {
                    return Err("--intent requires value".to_string());
                }
                intent = Some(args[idx].clone());
            }
            _ => {}
        }
        idx += 1;
    }

    if task_id.trim().is_empty() {
        return Err("worker mode: missing task id".to_string());
    }
    if db_path.trim().is_empty() {
        return Err("worker mode: missing db path".to_string());
    }

    tauri::async_runtime::block_on(async move {
        let options = SqliteConnectOptions::new()
            .filename(std::path::Path::new(&db_path))
            .create_if_missing(false)
            .synchronous(SqliteSynchronous::Normal);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .map_err(|err| err.to_string())?;
        task_usage::init_task_usage_pool(pool.clone());
        execute_task_run(
            &pool,
            None,
            ExecuteTaskRunRequest { task_id, intent },
        )
        .await
    })
}

pub async fn resolve_db_path(pool: &SqlitePool) -> Result<String, String> {
    let rows = sqlx::query_as::<_, (i64, String, String)>("PRAGMA database_list")
        .fetch_all(pool)
        .await
        .map_err(|err| err.to_string())?;
    rows.into_iter()
        .find(|(_, name, _)| name == "main")
        .map(|(_, _, file)| file)
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| "failed to resolve sqlite db path".to_string())
}

pub fn spawn_worker(
    runtime: &Arc<Mutex<TaskWorkerRuntime>>,
    db_path: &str,
    request: &ExecuteTaskRunRequest,
    app: Option<tauri::AppHandle>,
) -> Result<(), String> {
    let mut guard = runtime.lock().map_err(|_| "task worker lock poisoned".to_string())?;
    if let Some(child) = guard.child.as_mut() {
        if child.try_wait().map_err(|err| err.to_string())?.is_some() {
            guard.child = None;
            guard.running_task_id = None;
        } else {
            return Err("已有任务正在执行".to_string());
        }
    }

    let exe = std::env::current_exe().map_err(|err| err.to_string())?;
    let mut command = Command::new(exe);
    command
        .arg(WORKER_ARG)
        .arg("--task-id")
        .arg(request.task_id.trim())
        .arg("--db-path")
        .arg(db_path);
    if let Some(intent) = request.intent.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        command.arg("--intent").arg(intent);
    }
    let child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| err.to_string())?;
    let mut child = child;
    if let Some(app_handle) = app {
        if let Some(stdout) = child.stdout.take() {
            std::thread::spawn(move || {
                let reader = BufReader::new(stdout);
                for line in reader.lines().map_while(Result::ok) {
                    if let Some(raw) = line.strip_prefix(WORKER_EVENT_PREFIX) {
                        if let Ok(envelope) = serde_json::from_str::<WorkerEventEnvelope>(raw) {
                            let _ = app_handle.emit(envelope.event.as_str(), envelope.payload);
                        }
                    }
                }
            });
        }
    }

    guard.running_task_id = Some(request.task_id.clone());
    guard.child = Some(child);
    Ok(())
}

pub async fn wait_worker_finish(
    runtime: &Arc<Mutex<TaskWorkerRuntime>>,
    task_id: &str,
) -> Result<(), String> {
    loop {
        {
            let mut guard = runtime.lock().map_err(|_| "task worker lock poisoned".to_string())?;
            if guard.running_task_id.as_deref() != Some(task_id) {
                return Err("任务已被删除或终止".to_string());
            }
            let Some(child) = guard.child.as_mut() else {
                return Err("worker 已结束".to_string());
            };
            if let Some(status) = child.try_wait().map_err(|err| err.to_string())? {
                guard.child = None;
                guard.running_task_id = None;
                if status.success() {
                    return Ok(());
                }
                return Err(format!("worker 退出异常: {status}"));
            }
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

pub fn kill_worker_if_running(
    runtime: &Arc<Mutex<TaskWorkerRuntime>>,
    task_id: &str,
) -> Result<bool, String> {
    let mut guard = runtime.lock().map_err(|_| "task worker lock poisoned".to_string())?;
    if guard.running_task_id.as_deref() != Some(task_id) {
        return Ok(false);
    }
    let Some(mut child) = guard.child.take() else {
        guard.running_task_id = None;
        return Ok(false);
    };
    let _ = child.kill();
    let _ = child.try_wait();
    guard.running_task_id = None;
    Ok(true)
}
