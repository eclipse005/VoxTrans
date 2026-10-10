//! Headless entry point. Everything lives in `voxtrans::cli` so the command
//! implementations are part of the library (testable, and able to reach the
//! `pub(crate)` service internals the desktop app uses).
//!
//! Deliberately no `windows_subsystem = "windows"`: this binary is meant to be
//! driven from a console and must keep stdout/stderr.

fn main() -> std::process::ExitCode {
    voxtrans::cli::main()
}
