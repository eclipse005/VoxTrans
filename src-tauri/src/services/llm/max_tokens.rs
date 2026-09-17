//! Output-budget resolution and the adaptive degrade ladder.
//!
//! Protocol-neutral: both wire dialects must send an output cap, and either
//! can have it rejected by an endpoint whose real cap is lower than the
//! generous default. The Claude family table supplies real ceilings where we
//! know them; everything else takes the conservative fallback and relies on
//! the ladder (a one-time 400, then the halved value is memoized per
//! `(base_url, model)` so later requests start there).

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use super::base_url::normalize_base_url;
use super::error::LlmError;

/// Output budget for models we cannot classify as Claude. kimi-code falls
/// back to 128000 because it only talks to real Claude models; our users
/// mostly point at compatible vendors whose caps are unknown. The generous
/// value is safe because the adaptive post below degrades on rejection:
/// an endpoint that caps `max_tokens` gets a one-time 400, the halved value
/// is memoized per (base_url, model), and every later request starts there.
pub(super) const FALLBACK_MAX_TOKENS: u32 = 16_384;

/// Parse `(family, major, minor)` from a model name. Handles both
/// `claude-sonnet-4-5` and `claude-3-5-sonnet` orderings; 8+ digit tokens are
/// date stamps and never read as versions. Any family-word hit counts as a
/// Claude-shaped model (wire-path parity with kimi-code).
pub(super) fn parse_claude_version(model: &str) -> Option<(&'static str, u32, Option<u32>)> {
    const FAMILIES: [&str; 5] = ["fable", "mythos", "opus", "sonnet", "haiku"];
    let normalized = model.to_ascii_lowercase();
    let tokens: Vec<&str> = normalized
        .split(['-', '_', '.'])
        .filter(|t| !t.is_empty())
        .collect();
    let fam_idx = tokens.iter().position(|t| FAMILIES.contains(t))?;
    let family: &'static str = match tokens[fam_idx] {
        "fable" => "fable",
        "mythos" => "mythos",
        "opus" => "opus",
        "sonnet" => "sonnet",
        _ => "haiku",
    };
    // 1–2 digit version; longer digit runs are dates like `-20250929`.
    let version = |t: &str| -> Option<u32> {
        if t.len() <= 2 && t.chars().all(|c| c.is_ascii_digit()) {
            t.parse().ok()
        } else {
            None
        }
    };
    // family-first: sonnet-4-5 / opus-4
    if let Some(major) = tokens.get(fam_idx + 1).copied().and_then(version) {
        let minor = tokens.get(fam_idx + 2).copied().and_then(version);
        return Some((family, major, minor));
    }
    // version-first: claude-3-5-sonnet / claude-3-sonnet
    if fam_idx >= 2
        && let Some(major) = version(tokens[fam_idx - 2])
    {
        let minor = version(tokens[fam_idx - 1]);
        return Some((family, major, minor));
    }
    // bare major: claude-3-opus / sonnet-3 (one version token before family)
    if fam_idx >= 1
        && let Some(major) = version(tokens[fam_idx - 1])
    {
        return Some((family, major, None));
    }
    None
}

/// Max-output ceiling per family/major/minor. Tables descend so an unknown
/// future minor inherits from its nearest known sibling, then the family-major
/// baseline (kimi-code `lookupClaudeCeiling` walk).
fn output_ceiling(family: &str, major: u32, minor: Option<u32>) -> Option<u32> {
    const K128: u32 = 128_000;
    const K64: u32 = 64_000;
    let walk = |minor: Option<u32>, table: &[(u32, u32)], baseline: u32| -> u32 {
        if let Some(m) = minor {
            for (floor, value) in table {
                if m >= *floor {
                    return *value;
                }
            }
        }
        baseline
    };
    match (family, major) {
        ("fable", 5) | ("mythos", 5) => Some(K128),
        ("opus", 4) => Some(walk(
            minor,
            &[(8, K128), (7, K128), (6, K128), (5, K64), (1, 32_000), (0, 32_000)],
            32_000,
        )),
        ("opus", 3) => Some(walk(minor, &[(7, 8_192), (5, 8_192), (1, 8_192), (0, 4_096)], 4_096)),
        ("sonnet", 5) => Some(K128),
        ("sonnet", 4) => Some(walk(
            minor,
            &[(6, K128), (5, K64), (4, K64), (3, K64), (2, K64), (1, K64), (0, K64)],
            K64,
        )),
        ("sonnet", 3) => Some(walk(minor, &[(7, 8_192), (5, 8_192), (1, 8_192), (0, 4_096)], 4_096)),
        // haiku-4-5 and haiku-4 share the same cap.
        ("haiku", 4) => Some(K64),
        ("haiku", 3) => Some(walk(minor, &[(5, 8_192), (0, 4_096)], 4_096)),
        _ => None,
    }
}

/// Resolve the output budget for a request: the family ceiling for known
/// Claude models, the generous fallback otherwise (see [`effective_max_tokens`]
/// for how rejections degrade).
pub(super) fn resolve_max_tokens(model: &str) -> u32 {
    parse_claude_version(model)
        .and_then(|(family, major, minor)| output_ceiling(family, major, minor))
        .unwrap_or(FALLBACK_MAX_TOKENS)
}

/// Endpoints that already rejected a `max_tokens` value once, remembered per
/// (normalized base_url, model): later requests start from the lower value
/// instead of eating the same 400 on every call (project's global-mutex
/// pattern, same shape as `STRUCTURED_OUTPUT_UNSUPPORTED` in client.rs).
static MAX_TOKENS_OVERRIDE: OnceLock<Mutex<HashMap<String, u32>>> = OnceLock::new();

fn override_key(base_url: &str, model: &str) -> String {
    format!("{}\u{1}{model}", normalize_base_url(base_url))
}

/// The `max_tokens` a request should carry: the recorded degrade value when
/// this endpoint+model rejected a larger one before, the family ceiling
/// otherwise.
pub(super) fn effective_max_tokens(base_url: &str, model: &str) -> u32 {
    MAX_TOKENS_OVERRIDE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .ok()
        .and_then(|map| map.get(&override_key(base_url, model)).copied())
        .unwrap_or_else(|| resolve_max_tokens(model))
}

pub(super) fn record_max_tokens_override(base_url: &str, model: &str, value: u32) {
    if let Ok(mut map) = MAX_TOKENS_OVERRIDE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
    {
        map.insert(override_key(base_url, model), value);
    }
}

/// Does this error mean "your `max_tokens` exceeds this endpoint's cap"?
/// Only schema-shaped 400/422s qualify; unrelated bad requests keep failing
/// loudly. Vendor messages vary ("max_tokens is too large", "supports at
/// most N completion tokens", "maximum output token limit"), so the match
/// is kept token-flavored and generous — a false positive only costs one
/// extra attempt at half the budget before the error surfaces again.
pub(super) fn is_max_tokens_rejection(err: &LlmError) -> bool {
    if !matches!(err.status, Some(400) | Some(422)) {
        return false;
    }
    let message = err.message.to_ascii_lowercase();
    message.contains("max_token")
        || (message.contains("token")
            && [
                "maximum",
                "too large",
                "at most",
                "exceed",
            ]
            .iter()
            .any(|word| message.contains(word)))
}

/// Next rung on the degrade ladder: half the budget, floored at 1024.
/// `None` once the floor is reached — the endpoint keeps rejecting even the
/// smallest sane budget, so the original error must surface.
pub(super) fn halve_max_tokens(current: u32) -> Option<u32> {
    let halved = (current / 2).max(1024);
    (halved < current).then_some(halved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::error::LlmErrorKind;

    #[test]
    fn max_tokens_known_models() {
        let r = |m: &str| resolve_max_tokens(m);
        assert_eq!(r("claude-fable-5"), 128_000);
        assert_eq!(r("claude-mythos-5"), 128_000);
        assert_eq!(r("claude-opus-4-8"), 128_000);
        assert_eq!(r("claude-opus-4-5"), 64_000);
        assert_eq!(r("claude-opus-4-1"), 32_000);
        assert_eq!(r("claude-sonnet-5"), 128_000);
        assert_eq!(r("claude-sonnet-4-6"), 128_000);
        assert_eq!(r("claude-sonnet-4-5"), 64_000);
        assert_eq!(r("claude-haiku-4-5"), 64_000);
        assert_eq!(r("claude-3-7-sonnet-latest"), 8_192);
        assert_eq!(r("claude-3-5-haiku-20241022"), 8_192); // date token ignored
        assert_eq!(r("claude-3-opus"), 4_096);
    }

    #[test]
    fn max_tokens_unknown_and_non_claude_fallback() {
        // Compatible-endpoint model names get the conservative fallback.
        assert_eq!(resolve_max_tokens("deepseek-v4-flash"), FALLBACK_MAX_TOKENS);
        assert_eq!(resolve_max_tokens("kimi-k2-thinking"), FALLBACK_MAX_TOKENS);
        assert_eq!(resolve_max_tokens(""), FALLBACK_MAX_TOKENS);
        // Unknown claude-shaped name still parses as Claude but has no ceiling.
        assert_eq!(resolve_max_tokens("sonnet-latest"), FALLBACK_MAX_TOKENS);
        assert_eq!(resolve_max_tokens("glm-4.6"), FALLBACK_MAX_TOKENS);
    }

    #[test]
    fn version_parser_forms() {
        assert_eq!(
            parse_claude_version("Claude-Sonnet-4.5"),
            Some(("sonnet", 4, Some(5)))
        );
        assert_eq!(
            parse_claude_version("claude-3-5-sonnet-20241022"),
            Some(("sonnet", 3, Some(5)))
        );
        assert_eq!(parse_claude_version("opus-4"), Some(("opus", 4, None)));
        assert_eq!(parse_claude_version("kimi-k2-thinking"), None);
        // Bare family word carries no version at all.
        assert_eq!(parse_claude_version("haiku-latest"), None);
        // Bare major before the family.
        assert_eq!(parse_claude_version("claude-3-opus"), Some(("opus", 3, None)));
    }

    #[test]
    fn haiku_latest_inherits_baseline() {
        // haiku with no usable major falls back conservatively.
        assert_eq!(resolve_max_tokens("haiku-latest"), FALLBACK_MAX_TOKENS);
    }

    fn rejection(status: u16, msg: &str) -> LlmError {
        let mut err = LlmError::new(LlmErrorKind::Http, msg.to_string());
        err.status = Some(status);
        err
    }

    #[test]
    fn max_tokens_rejection_detection() {
        assert!(is_max_tokens_rejection(&rejection(
            400,
            "http status 400: max_tokens is too large: 16384. This model supports at most 8192"
        )));
        assert!(is_max_tokens_rejection(&rejection(
            400,
            "http status 400: output token limit exceeded"
        )));
        assert!(is_max_tokens_rejection(&rejection(
            422,
            "http status 422: maximum completion tokens is 4096"
        )));
        // Unrelated 400s and non-client statuses never trigger the degrade.
        assert!(!is_max_tokens_rejection(&rejection(
            400,
            "http status 400: invalid model id"
        )));
        assert!(!is_max_tokens_rejection(&rejection(
            429,
            "http status 429: max_tokens quota exhausted"
        )));
        assert!(!is_max_tokens_rejection(&rejection(
            500,
            "http status 500: token service exploded"
        )));
    }

    #[test]
    fn max_tokens_halving_ladder_floors_at_1024() {
        assert_eq!(halve_max_tokens(16_384), Some(8_192));
        assert_eq!(halve_max_tokens(2_048), Some(1_024));
        // Floor reached: no further degrade, the error must surface.
        assert_eq!(halve_max_tokens(1_024), None);
        assert_eq!(halve_max_tokens(512), None);
    }

    #[test]
    fn max_tokens_override_memo_is_per_endpoint_and_model() {
        let base = format!("http://maxtok-test.local/{}", line!());
        let model_a = "probe-a";
        let model_b = "probe-b";
        assert_eq!(effective_max_tokens(&base, model_a), FALLBACK_MAX_TOKENS);
        record_max_tokens_override(&base, model_a, 4_096);
        assert_eq!(effective_max_tokens(&base, model_a), 4_096);
        // Same base, different model: unaffected.
        assert_eq!(effective_max_tokens(&base, model_b), FALLBACK_MAX_TOKENS);
        // Same model, different base: unaffected.
        assert_eq!(
            effective_max_tokens("http://maxtok-other.local", model_a),
            FALLBACK_MAX_TOKENS
        );
    }
}
