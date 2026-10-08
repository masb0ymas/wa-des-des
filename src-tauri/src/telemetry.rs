//! Crash and error reporting.
//!
//! Sentry is optional: without a DSN the SDK builds a disabled client, so nothing is sent and no
//! network call is made. That is the default, and it keeps the app fully usable offline and for
//! users who do not want telemetry.

use sentry::types::Dsn;
use sentry::{ClientInitGuard, ClientOptions};
use std::str::FromStr;

/// Environment variable holding the DSN.
///
/// Deliberately not baked into the binary: a DSN is per-deployment, and a compiled-in one cannot be
/// pointed at a staging project without a rebuild. The SDK also reads `SENTRY_DSN` on its own, but
/// this app names its own variable so an unrelated `SENTRY_DSN` in the environment cannot silently
/// enable reporting.
pub const DSN_ENV: &str = "WA_SENTRY_DSN";

/// Parses the configured DSN.
///
/// Returns `None` for a missing, blank or malformed value, so a typo in the DSN disables reporting
/// instead of taking the app down. This matters because the tuple form of `sentry::init` calls
/// `.expect("invalid value for DSN")` on a malformed string: feeding it raw environment input would
/// turn a misconfigured DSN into a startup panic.
fn parse_dsn(raw: Option<&str>) -> Option<Dsn> {
    let raw = raw?.trim();
    if raw.is_empty() {
        return None;
    }
    match Dsn::from_str(raw) {
        Ok(dsn) => Some(dsn),
        Err(error) => {
            eprintln!("{DSN_ENV} is not a valid Sentry DSN ({error}); error reporting is off");
            None
        }
    }
}

/// Initialises error reporting and returns the guard that must stay alive for the whole process.
///
/// The guard owns the transport: dropping it shuts the transport down and any queued event is lost,
/// which is why the caller binds it for the duration of `run`.
///
/// `send_default_pii` stays off: the app's data is private conversations. Keep account names,
/// phone numbers and message content out of `capture_message`, `capture_error` and breadcrumbs.
pub fn init() -> ClientInitGuard {
    // Built with the setters rather than a struct literal: `ClientOptions` is `#[non_exhaustive]`
    // in this version, so `ClientOptions { .., ..Default::default() }` does not compile.
    let mut options = ClientOptions::new()
        // `maybe_release` is the setter built for `release_name!`, which yields an `Option`.
        .maybe_release(sentry::release_name!())
        .send_default_pii(false);

    // Assigned directly rather than through `.dsn(&str)`, which also panics on a malformed value.
    // This is equivalent to the `(dsn, options)` tuple form, minus that panic.
    options.dsn = parse_dsn(std::env::var(DSN_ENV).ok().as_deref());

    let guard = sentry::init(options);

    if guard.is_enabled() {
        sentry::configure_scope(|scope| {
            // Tags, not PII: enough to tell deployments apart in the dashboard.
            scope.set_tag("platform", std::env::consts::OS);
            scope.set_tag("arch", std::env::consts::ARCH);
            scope.set_tag("webview", webview_engine());
        });
    }

    guard
}

/// Name of the webview engine backing the window, per platform.
fn webview_engine() -> &'static str {
    if cfg!(target_os = "macos") {
        "wkwebview"
    } else if cfg!(target_os = "windows") {
        "webview2"
    } else if cfg!(target_os = "linux") {
        "webkitgtk"
    } else {
        "unknown"
    }
}

/// Reports an error with its full source chain.
///
/// The `sentry` crate has no blanket `From` for `anyhow`, and this app does not use `anyhow`, so
/// callers hand over an error that implements `std::error::Error`. A no-op when reporting is off.
pub fn capture_error<E: std::error::Error + ?Sized>(error: &E, context: &str) {
    if !sentry::Hub::current().client().is_some() {
        return;
    }

    // `event_from_error` walks the source chain and keeps every cause, which is what makes a
    // wrapped error readable in the dashboard.
    let mut event = sentry::event_from_error(error);
    event.level = sentry::protocol::Level::Error;
    event.message = Some(context.to_string());
    sentry::capture_event(event);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_dsn_disables_reporting() {
        assert!(parse_dsn(None).is_none());
    }

    #[test]
    fn blank_dsn_disables_reporting() {
        assert!(parse_dsn(Some("")).is_none());
        assert!(parse_dsn(Some("   ")).is_none());
        assert!(parse_dsn(Some("\t\n")).is_none());
    }

    /// A typo must not become a startup panic, which is what the tuple form of `init` would do.
    #[test]
    fn malformed_dsn_is_rejected_instead_of_panicking() {
        for bad in [
            "not-a-url",
            "https://example.com",       // no project id, no key
            "https://@sentry.io/1",      // empty public key
            "ftp://key@sentry.io/1",     // unsupported scheme
            "https://key@sentry.io",     // no project id
        ] {
            assert!(parse_dsn(Some(bad)).is_none(), "{bad} should be rejected");
        }
    }

    #[test]
    fn valid_dsn_is_accepted() {
        let parsed = parse_dsn(Some("https://key@o1.ingest.sentry.io/42"));
        assert!(parsed.is_some());
        let dsn = parsed.expect("dsn");
        assert_eq!(dsn.project_id().to_string(), "42");
    }

    #[test]
    fn surrounding_whitespace_is_tolerated() {
        // Trailing newlines are easy to introduce through env files and CI secrets.
        assert!(parse_dsn(Some("  https://key@o1.ingest.sentry.io/42\n")).is_some());
    }

    #[test]
    fn init_without_dsn_produces_a_disabled_client() {
        // SAFETY: single-threaded test; the variable is restored immediately below.
        let previous = std::env::var(DSN_ENV).ok();
        unsafe { std::env::remove_var(DSN_ENV) };

        let guard = init();
        assert!(!guard.is_enabled(), "no DSN must mean no reporting");

        if let Some(previous) = previous {
            unsafe { std::env::set_var(DSN_ENV, previous) };
        }
    }

    #[test]
    fn init_with_a_malformed_dsn_does_not_panic() {
        let previous = std::env::var(DSN_ENV).ok();
        unsafe { std::env::set_var(DSN_ENV, "definitely-not-a-dsn") };

        let guard = init();
        assert!(!guard.is_enabled());

        match previous {
            Some(previous) => unsafe { std::env::set_var(DSN_ENV, previous) },
            None => unsafe { std::env::remove_var(DSN_ENV) },
        }
    }

    #[test]
    fn webview_engine_matches_the_target() {
        let engine = webview_engine();
        assert!(["wkwebview", "webview2", "webkitgtk", "unknown"].contains(&engine));
        assert_eq!(engine == "unknown", !cfg!(any(target_os = "macos", target_os = "windows", target_os = "linux")));
    }
}
