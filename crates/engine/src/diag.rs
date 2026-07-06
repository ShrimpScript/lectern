//! Opt-in engine diagnostics. Silent by default; set `LECTERN_DEBUG=1` (or any
//! truthy value) to trace key engine events to stderr — backend spawns today,
//! more paths over time. Zero dependencies, so the engine stays completely quiet
//! in normal use. Addresses the production-readiness audit's observability gap.

use std::sync::OnceLock;

/// Parse the `LECTERN_DEBUG` value: any non-empty value that isn't `0`/`false`
/// turns diagnostics on. Split out so it's unit-testable without touching env.
fn is_on(val: Option<&str>) -> bool {
    matches!(val, Some(v) if !v.is_empty() && v != "0" && !v.eq_ignore_ascii_case("false"))
}

/// Whether diagnostics are enabled — cached at first use (env is set at start).
fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| is_on(std::env::var("LECTERN_DEBUG").ok().as_deref()))
}

/// Emit a diagnostic line to stderr, but only when `LECTERN_DEBUG` is set.
/// `area` is a short subsystem tag (e.g. `"backend"`, `"conductor"`).
pub fn log(area: &str, msg: &str) {
    if enabled() {
        eprintln!("[lectern:{area}] {msg}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_debug_flag() {
        assert!(is_on(Some("1")));
        assert!(is_on(Some("true")));
        assert!(is_on(Some("yes")));
        assert!(!is_on(Some("0")));
        assert!(!is_on(Some("false")));
        assert!(!is_on(Some("FALSE")));
        assert!(!is_on(Some("")));
        assert!(!is_on(None));
    }

    #[test]
    fn log_never_panics() {
        // No-op when LECTERN_DEBUG is unset (default in CI); must never panic.
        log("test", "diagnostic line");
    }
}
