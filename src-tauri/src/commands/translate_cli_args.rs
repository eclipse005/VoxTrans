pub(super) fn maybe_run_cli_mode(
    run_arg: &str,
    runner: fn(&[String]) -> Result<(), String>,
) -> bool {
    let args = std::env::args().collect::<Vec<_>>();
    if args.len() < 2 || args[1] != run_arg {
        return false;
    }

    let code = match runner(&args[2..]) {
        Ok(()) => 0,
        Err(err) => {
            eprintln!("{err}");
            1
        }
    };
    std::process::exit(code);
}

pub(super) fn required_cli_value(
    args: &[String],
    idx: usize,
    flag: &str,
) -> Result<String, String> {
    args.get(idx)
        .cloned()
        .ok_or_else(|| format!("{flag} requires value"))
}
