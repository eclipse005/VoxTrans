use std::path::PathBuf;
use std::process::{Command, ExitStatus};

pub fn resolve_bundled_or_path(program: &str) -> PathBuf {
    let exe_name = format!("{program}{}", std::env::consts::EXE_SUFFIX);
    if let Ok(exe_path) = std::env::current_exe()
        && let Some(exe_dir) = exe_path.parent()
    {
        let bundled = exe_dir.join("bin").join(&exe_name);
        if bundled.is_file() {
            return bundled;
        }
    }
    PathBuf::from(exe_name)
}

pub fn configure_background_command(command: &mut Command) {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
}

/// Human-readable description of how a child process ended, for error messages:
/// an exit code, or the signal that killed it on Unix. Never empty.
pub fn describe_exit(status: &ExitStatus) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return format!("signal {signal}");
        }
    }
    match status.code() {
        Some(code) => format_exit_code(code),
        None => "unknown exit status".to_string(),
    }
}

fn format_exit_code(code: i32) -> String {
    // Windows reports faults as NTSTATUS values such as 0xC0000005 (access
    // violation) or 0xC000013A (Ctrl-C). `status.code()` hands those back as
    // large or negative i32s whose shape is only legible in hex; ordinary
    // application codes stay decimal because "exit code 1" is what a reader
    // expects to see.
    #[cfg(windows)]
    if !(0..=0xFFFF).contains(&code) {
        return format!("exit code 0x{:08X}", code as u32);
    }
    format!("exit code {code}")
}

#[cfg(test)]
mod tests {
    use super::describe_exit;
    use std::process::{Command, ExitStatus};

    /// Spawn a real process that exits with `code`, so the test covers the
    /// platform status plumbing rather than a hand-built value.
    fn status_for_exit_code(code: u32) -> ExitStatus {
        #[cfg(windows)]
        let status = Command::new("cmd")
            .args(["/C", &format!("exit {code}")])
            .status();
        #[cfg(not(windows))]
        let status = Command::new("sh")
            .args(["-c", &format!("exit {code}")])
            .status();
        status.expect("spawn exit helper")
    }

    #[test]
    fn describe_exit_keeps_small_codes_decimal() {
        let status = status_for_exit_code(3);

        assert_eq!(describe_exit(&status), "exit code 3");
    }

    #[cfg(windows)]
    #[test]
    fn describe_exit_prints_windows_fault_codes_as_hex() {
        use std::os::windows::process::ExitStatusExt;

        // 0xC0000005 is an access violation; `code()` hands it back negative.
        let status = ExitStatus::from_raw(0xC0000005u32);

        assert_eq!(describe_exit(&status), "exit code 0xC0000005");
    }

    #[cfg(unix)]
    #[test]
    fn describe_exit_names_the_signal_on_unix() {
        use std::os::unix::process::ExitStatusExt;

        // A raw wait status whose low bits are the killing signal.
        let status = ExitStatus::from_raw(9);

        assert_eq!(describe_exit(&status), "signal 9");
    }
}
