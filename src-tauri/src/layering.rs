//! Structural check: the inner layers must keep pointing inward.
//!
//! Layering inside a single crate is a convention the compiler cannot enforce —
//! an outward reference compiles, links and runs exactly like an inward one. It
//! therefore erodes by default: `602b9bb` removed **54** outward references from
//! `domain/` and `db/` that had accumulated that way, one plausible-looking
//! import at a time. Nothing prevented the next 54.
//!
//! `clippy.toml` cannot express this rule either: its configuration is
//! crate-global, and `crate::services` is perfectly legitimate everywhere except
//! inside these two layers. So the rule is spelled out as a test over the source
//! tree — deliberately dumb, but precise about *where* it applies.
//!
//! Paired with `domain`'s inability to reach the app log directly (which is what
//! forced `StepExecution::cache_discarded` to report the condition as a value
//! instead of logging it), this is the one place the layering is mechanical
//! rather than aspirational.

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    /// Layers that must not know about anything outward.
    const INNER_LAYERS: &[&str] = &["domain", "db"];

    /// Paths that would point an inner layer outward. Written without the
    /// `crate::` prefix so that relative and aliased forms (`super::services::`,
    /// `use crate::services as x`) cannot slip past. `db::` is deliberately
    /// absent: `domain::pipeline` still holds it, and that edge is tracked
    /// separately (`602b9bb`'s "domain is left with a single external edge").
    const FORBIDDEN: &[&str] = &["services::", "commands::"];

    fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
        let entries = fs::read_dir(dir)
            .unwrap_or_else(|err| panic!("cannot read {}: {err}", dir.display()));
        for entry in entries {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                rust_files(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
    }

    /// `crate::services` mentioned while explaining the rule is not a violation.
    fn code_before_comment(line: &str) -> &str {
        line.split("//").next().unwrap_or("")
    }

    #[test]
    fn inner_layers_do_not_reach_outward() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut violations = Vec::new();

        for layer in INNER_LAYERS {
            let mut files = Vec::new();
            rust_files(&src.join(layer), &mut files);
            assert!(
                !files.is_empty(),
                "found no .rs files under {}/ — the scan is looking in the wrong place",
                layer
            );

            for file in files {
                let text = fs::read_to_string(&file)
                    .unwrap_or_else(|err| panic!("cannot read {}: {err}", file.display()));
                for (index, line) in text.lines().enumerate() {
                    let code = code_before_comment(line);
                    if FORBIDDEN.iter().any(|path| code.contains(path)) {
                        violations.push(format!(
                            "  {}:{}: {}",
                            file.strip_prefix(&src).unwrap_or(&file).display(),
                            index + 1,
                            line.trim()
                        ));
                    }
                }
            }
        }

        assert!(
            violations.is_empty(),
            "{}/ and {}/ must depend inward only; found {} outward reference(s).\n\
             Fix by reporting the condition as a value and letting the outer layer record it \
             (see `StepExecution::cache_discarded`):\n{}",
            INNER_LAYERS[0],
            INNER_LAYERS[1],
            violations.len(),
            violations.join("\n")
        );
    }
}
