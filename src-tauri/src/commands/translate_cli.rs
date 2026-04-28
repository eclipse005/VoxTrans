use super::translate_cli_args::maybe_run_cli_mode;
use super::translate_cli_step5::run_build_step5_mode_from_args;
use super::translate_cli_terminology::run_build_terminology_mode_from_args;
use super::translate_cli_translation::run_build_translation_mode_from_args;

pub fn maybe_run_build_terminology_mode_from_args() -> bool {
    maybe_run_cli_mode(
        "--voxtrans-build-terminology",
        run_build_terminology_mode_from_args,
    )
}

pub fn maybe_run_build_translation_mode_from_args() -> bool {
    maybe_run_cli_mode(
        "--voxtrans-build-translation",
        run_build_translation_mode_from_args,
    )
}

pub fn maybe_run_build_step5_mode_from_args() -> bool {
    maybe_run_cli_mode("--voxtrans-build-step5", run_build_step5_mode_from_args)
}
