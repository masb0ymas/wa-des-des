//! Webview User-Agent policy.
//!
//! WhatsApp Web gates its login page on `navigator.userAgent`: engines whose UA string it does not
//! recognise are answered with a "browser not supported" page instead of the QR/pairing flow.
//! The three desktop webview engines all report themselves differently:
//!
//! | Platform | Engine          | Default UA token                       |
//! |----------|-----------------|----------------------------------------|
//! | Windows  | WebView2        | `Edg/<version>` (Chromium based)       |
//! | macOS    | WKWebView       | `Safari/<version>`                     |
//! | Linux    | WebKitGTK       | `Safari/<version>` or a distro string  |
//!
//! WebView2 and WKWebView are normally accepted as-is. WebKitGTK is the problematic one: its UA
//! often lacks any recognised token, and WhatsApp Web refuses to boot. Overriding the UA with a
//! recent Chrome/Safari string is the documented workaround.
//!
//! The UA must be applied when the webview is created — the engines expose no runtime setter
//! (WebView2 `SetUserAgent` and WebKitGTK `settings.set_user_agent` are both set-once, and WKWebView
//! `customUserAgent` has no effect on an already loaded page), so changing it requires a rebuild of
//! the session window.

/// Snapshot date of the UA strings below.
///
/// Bump the versions (and this date) periodically: a UA that is several major versions behind is
/// itself a fingerprint, and sites eventually refuse stale engines.
const SNAPSHOT: &str = "2026-10-06";

/// Webview UA used for the Chrome presets. Chrome on desktop reports the frozen macOS token
/// `10_15_7` regardless of the real OS version, so these strings stay valid across macOS releases.
const UA_CHROME_MAC: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/145.0.0.0 Safari/537.36";
const UA_CHROME_WINDOWS: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/154.0.0.0 Safari/537.36";
const UA_CHROME_LINUX: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/148.0.0.0 Safari/537.36";

/// WKWebView is a Safari engine, so the Safari preset is the one that keeps the advertised
/// capabilities closest to reality. `Version/` is what sites sniff for Safari.
const UA_SAFARI_MAC: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.5 Safari/605.1.15";

const UA_FIREFOX_MAC: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:157.0) Gecko/20100101 Firefox/157.0";
const UA_FIREFOX_WINDOWS: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:153.0) Gecko/20100101 Firefox/153.0";
const UA_FIREFOX_LINUX: &str =
    "Mozilla/5.0 (X11; Linux x86_64; rv:155.0) Gecko/20100101 Firefox/155.0";

/// Sentinel preset: do not touch the webview UA at all.
pub const PRESET_ENGINE_DEFAULT: &str = "engine-default";
/// Sentinel preset: the user supplied the UA string.
pub const PRESET_CUSTOM: &str = "custom";

struct Preset {
    id: &'static str,
    label: &'static str,
    note: &'static str,
    user_agent: &'static str,
}

/// Presets in the order they are offered in the settings panel.
const PRESETS: &[Preset] = &[
    Preset {
        id: PRESET_ENGINE_DEFAULT,
        label: "Engine default",
        note: "Send the untouched webview UA. WebView2 and WKWebView are usually accepted; WebKitGTK is often rejected.",
        user_agent: "",
    },
    Preset {
        id: "chrome-macos",
        label: "Chrome on macOS",
        note: "Chromium UA. Use when WhatsApp Web reports an unsupported browser, including on WebKitGTK.",
        user_agent: UA_CHROME_MAC,
    },
    Preset {
        id: "chrome-windows",
        label: "Chrome on Windows",
        note: "Chromium UA. Matches what WebView2 actually is, minus the Edg/ token.",
        user_agent: UA_CHROME_WINDOWS,
    },
    Preset {
        id: "chrome-linux",
        label: "Chrome on Linux",
        note: "Chromium UA. The standard WebKitGTK workaround for WhatsApp Web.",
        user_agent: UA_CHROME_LINUX,
    },
    Preset {
        id: "safari-macos",
        label: "Safari on macOS",
        note: "Safari UA. Closest match for WKWebView capabilities; keeps FaceTime/WebRTC behaviour plausible.",
        user_agent: UA_SAFARI_MAC,
    },
    Preset {
        id: "firefox-macos",
        label: "Firefox on macOS",
        note: "Gecko UA. Only useful if a site mis-detects Chromium; the engine still behaves like WebKit.",
        user_agent: UA_FIREFOX_MAC,
    },
    Preset {
        id: "firefox-windows",
        label: "Firefox on Windows",
        note: "Gecko UA. Only useful if a site mis-detects Chromium; the engine still behaves like WebView2.",
        user_agent: UA_FIREFOX_WINDOWS,
    },
    Preset {
        id: "firefox-linux",
        label: "Firefox on Linux",
        note: "Gecko UA. Only useful if a site mis-detects Chromium; the engine still behaves like WebKitGTK.",
        user_agent: UA_FIREFOX_LINUX,
    },
    Preset {
        id: PRESET_CUSTOM,
        label: "Custom…",
        note: "Paste any UA string. Applied verbatim; it must not contain line breaks.",
        user_agent: "",
    },
];

/// Recommended preset for the running platform.
///
/// macOS defaults to Safari because WKWebView genuinely is a Safari engine; Windows defaults to
/// Chrome because WebView2 genuinely is Chromium. Linux defaults to Chrome because WebKitGTK's own
/// UA is the one WhatsApp Web rejects.
pub fn default_preset(platform: &str) -> &'static str {
    match platform {
        "macos" => "safari-macos",
        "windows" => "chrome-windows",
        _ => "chrome-linux",
    }
}

pub fn is_known_preset(id: &str) -> bool {
    PRESETS.iter().any(|preset| preset.id == id)
}

/// A UA string is only usable if it is a single non-empty line: the engines pass it straight to the
/// native setter, and a stray newline corrupts the request headers.
fn sanitize(user_agent: &str) -> Option<String> {
    let trimmed = user_agent.trim();
    if trimmed.is_empty() || trimmed.chars().any(char::is_control) {
        return None;
    }
    Some(trimmed.to_string())
}

/// Resolves the UA to install on the session webview.
///
/// `None` means "leave the engine UA alone", which is what the `engine-default` preset and any
/// invalid selection fall back to.
pub fn resolve(preset: &str, custom_user_agent: &str) -> Option<String> {
    if preset == PRESET_ENGINE_DEFAULT {
        return None;
    }

    if preset == PRESET_CUSTOM {
        return sanitize(custom_user_agent);
    }

    PRESETS
        .iter()
        .find(|entry| entry.id == preset && !entry.user_agent.is_empty())
        .and_then(|entry| sanitize(entry.user_agent))
}

/// Preset ids and labels for the settings UI.
pub fn options() -> Vec<serde_json::Value> {
    PRESETS
        .iter()
        .map(|preset| {
            serde_json::json!({
                "id": preset.id,
                "label": preset.label,
                "note": preset.note,
                "value": preset.user_agent,
            })
        })
        .collect()
}

/// The UA snapshot date, surfaced in the settings so a stale string is obvious.
pub fn snapshot_date() -> &'static str {
    SNAPSHOT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_default_leaves_the_webview_untouched() {
        assert_eq!(resolve(PRESET_ENGINE_DEFAULT, "ignored"), None);
    }

    #[test]
    fn chrome_presets_advertise_chrome() {
        let ua = resolve("chrome-linux", "").expect("preset resolves");
        assert!(ua.contains("Chrome/"), "{ua}");
        assert!(ua.contains("Linux x86_64"), "{ua}");
        assert!(!ua.contains("Edg/"), "{ua}");
    }

    #[test]
    fn safari_preset_advertises_safari_and_not_chrome() {
        let ua = resolve("safari-macos", "").expect("preset resolves");
        assert!(ua.contains("Version/") && ua.contains("Safari/"), "{ua}");
        assert!(!ua.contains("Chrome/"), "{ua}");
    }

    #[test]
    fn custom_preset_is_used_verbatim() {
        let custom = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 Chrome/200.0.0.0";
        assert_eq!(resolve(PRESET_CUSTOM, custom).as_deref(), Some(custom));
    }

    #[test]
    fn custom_preset_is_trimmed() {
        assert_eq!(
            resolve(PRESET_CUSTOM, "  Mozilla/5.0  ").as_deref(),
            Some("Mozilla/5.0")
        );
    }

    #[test]
    fn empty_and_multiline_custom_values_fall_back_to_the_engine_ua() {
        assert_eq!(resolve(PRESET_CUSTOM, ""), None);
        assert_eq!(resolve(PRESET_CUSTOM, "   "), None);
        assert_eq!(resolve(PRESET_CUSTOM, "Mozilla/5.0\r\nX-Injected: 1"), None);
        assert_eq!(resolve(PRESET_CUSTOM, "Mozilla/5.0\nX-Injected: 1"), None);
    }

    #[test]
    fn unknown_preset_falls_back_to_the_engine_ua() {
        assert_eq!(resolve("netscape-4", ""), None);
    }

    #[test]
    fn every_platform_gets_a_real_preset() {
        for platform in ["macos", "windows", "linux", "freebsd"] {
            let id = default_preset(platform);
            assert!(is_known_preset(id), "{platform} -> {id}");
            assert!(
                resolve(id, "").is_some(),
                "{platform} default must install a UA"
            );
        }
    }

    #[test]
    fn presets_are_unique_and_documented() {
        let mut ids: Vec<&str> = PRESETS.iter().map(|preset| preset.id).collect();
        ids.sort_unstable();
        let count = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), count, "duplicate preset id");

        for preset in PRESETS {
            assert!(!preset.label.is_empty(), "{} has no label", preset.id);
            assert!(!preset.note.is_empty(), "{} has no note", preset.id);
        }
    }

    #[test]
    fn options_expose_the_same_ids_as_the_resolver() {
        let options = options();
        assert_eq!(options.len(), PRESETS.len());
        for option in options {
            let id = option["id"].as_str().expect("id is a string");
            assert!(is_known_preset(id), "unknown id in options: {id}");
        }
    }
}
