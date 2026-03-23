use std::path::PathBuf;
use std::process::Command;

pub fn resolve_bundled_or_path(program: &str) -> PathBuf {
    let exe_name = format!("{program}{}", std::env::consts::EXE_SUFFIX);
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let bundled = exe_dir.join("bin").join(&exe_name);
            if bundled.is_file() {
                return bundled;
            }
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
