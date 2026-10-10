use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};

use super::progress_parse::parse_progress_percent;
use crate::services::binary::{
    configure_background_command, describe_exit, resolve_bundled_or_path,
};

pub(super) fn run_demucs_with_progress<F>(
    model: &str,
    model_dir: &Path,
    output_root: &Path,
    demucs_input: &Path,
    mut on_progress: F,
) -> Result<(), String>
where
    F: FnMut(u32),
{
    let child = demucs_command()
        .arg("--model")
        .arg(model)
        .arg("--model-dir")
        .arg(model_dir)
        .arg("--stems")
        .arg("vocals")
        .arg("--json-progress")
        .arg("-o")
        .arg(output_root)
        .arg(demucs_input)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("Failed to start demucs: {}", err))?;

    on_progress(0);
    let (status, stderr) = drain_child(child, |line| {
        if let Some(percent) = parse_progress_percent(line) {
            on_progress(percent);
        }
    })?;
    if !status.success() {
        let reason = describe_exit(&status);
        let stderr = stderr.trim();
        if stderr.is_empty() {
            return Err(format!("demucs separation failed ({reason})"));
        }
        return Err(format!("demucs separation failed ({reason}): {stderr}"));
    }

    Ok(())
}

/// Wait for `child` while draining both of its pipes: stdout line by line (that
/// is where progress arrives), stderr on its own thread.
///
/// Reading stdout alone until EOF is a deadlock, not a simplification: a child
/// that fills the 64 KiB stderr pipe blocks on the write while this side waits
/// for a stdout EOF that cannot come. The hang looks exactly like a crash with
/// no log, so the drain is the fix — and `a_child_that_fills_its_stderr_pipe…`
/// keeps a child with a deliberately oversized stderr covered.
fn drain_child<F>(mut child: Child, mut on_stdout_line: F) -> Result<(ExitStatus, String), String>
where
    F: FnMut(&str),
{
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Failed to read child stdout".to_string())?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| "Failed to read child stderr".to_string())?;
    let stderr_reader = std::thread::spawn(move || {
        // Lossy on purpose: an invalid byte in a path or a warning must not turn
        // the whole stderr into an empty string (which would leave only the exit
        // code in the failure text).
        let mut bytes = Vec::new();
        let _ = stderr.read_to_end(&mut bytes);
        String::from_utf8_lossy(&bytes).into_owned()
    });

    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    loop {
        line.clear();
        let read = reader.read_line(&mut line).map_err(|err| err.to_string())?;
        if read == 0 {
            break;
        }
        on_stdout_line(line.trim());
    }

    let status = child
        .wait()
        .map_err(|err| format!("Failed to wait for child: {err}"))?;
    let stderr = stderr_reader.join().unwrap_or_default();
    Ok((status, stderr))
}

fn demucs_command() -> Command {
    let mut cmd = Command::new(resolve_demucs_program());
    configure_background_command(&mut cmd);
    cmd
}

fn resolve_demucs_program() -> PathBuf {
    if let Ok(custom) = std::env::var("VOXTRANS_DEMUCS_BIN") {
        let path = PathBuf::from(custom);
        if path.exists() {
            return path;
        }
    }

    resolve_bundled_or_path("demucs")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    /// The deadlock this module was fixed for, pinned: a child that writes more
    /// to stderr than the 64 KiB pipe buffer holds used to block forever while
    /// this side waited for stdout to end — a silent hang that looked like a
    /// crash with no log. Without the stderr drain this test never returns, so
    /// the watchdog *is* the assertion.
    #[cfg(windows)]
    #[test]
    fn a_child_that_fills_its_stderr_pipe_does_not_hang() {
        // ~160 KB to stderr, then a non-zero exit: big enough to fill the pipe
        // several times over.
        let child = Command::new("cmd")
            .arg("/c")
            .arg(
                "(for /L %i in (1,1,4000) do @echo \
                 xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx 1>&2) & exit 7",
            )
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn the noisy child");

        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(drain_child(child, |_| {}));
        });

        match receiver.recv_timeout(Duration::from_secs(60)) {
            Ok(Ok((status, stderr))) => {
                assert_eq!(status.code(), Some(7), "the child's exit code is preserved");
                assert!(
                    stderr.len() > 64 * 1024,
                    "the whole stderr must be drained, got {} bytes",
                    stderr.len()
                );
            }
            Ok(Err(err)) => panic!("draining failed: {err}"),
            Err(_) => panic!("deadlocked: a child with a full stderr pipe never finished"),
        }
    }
}
