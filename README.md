# WA Des Des

Desktop shell for WhatsApp Web built with Tauri v2. One window: a dock on the left edge lists the
accounts, and each account is its own native webview loading `https://web.whatsapp.com` next to it.
The settings live in the same window (gear button in the dock).

## Multiple accounts

Press **+** in the dock to add an account. It gets a new webview with an isolated data store
(WebView2/WebKitGTK: `<app data>/accounts/<id>`; WKWebView: a data-store identifier, macOS 14+),
so the accounts that are already linked are not reloaded or logged out. All accounts stay loaded in
the background and keep receiving messages; the dock shows each one's unread count. The first
account uses the engine's default store, which is where a login from the old two-window version
lives.

## Why the User-Agent matters

Tauri does not ship a browser. It embeds the OS webview, so the same app reports a different
`navigator.userAgent` on every platform:

| Platform | Engine | What the engine reports by default | WhatsApp Web |
|----------|--------|------------------------------------|--------------|
| Windows 10/11 | WebView2 (Chromium/Edge) | `… Chrome/154.0.0.0 Safari/537.36 Edg/154.0.0.0` | accepted |
| macOS | WKWebView (Safari/WebKit) | `… AppleWebKit/605.1.15 … Version/26.5 Safari/605.1.15` | accepted |
| Linux | WebKitGTK | `… AppleWebKit/605.1.15 …` plus a distro token, or a bare `WebKitGTK/2.x` | **rejected** |

WhatsApp Web gates its login page on the UA string. WebKitGTK frequently produces a string with no
recognised engine token, and the page answers with *"WhatsApp Web is not supported in your
browser"* instead of the QR/pairing flow. Setting a recent Chrome or Safari UA is the standard
workaround, and it is what this app does by default on Linux.

The UA is installed on the webview **before the first navigation**. None of the three engines
exposes a runtime setter — WebView2's `SetUserAgent` and WebKitGTK's `settings.set_user_agent` are
set-once, and WKWebView's `customUserAgent` has no effect on an already-loaded page — so changing
the preset recreates the account webviews (the logins live in the data stores and survive).

### Presets

Default per platform: `chrome-windows` on Windows, `safari-macos` on macOS (WKWebView really is a
Safari engine), `chrome-linux` on Linux.

`engine-default` sends the untouched UA. `custom` applies a verbatim string; empty or multi-line
values are rejected (a stray newline corrupts the request headers) and fall back to the engine
default.

UA versions are pinned with a snapshot date shown in the settings. Bump them periodically: a UA
that is several major versions behind is itself a fingerprint, and sites eventually refuse stale
engines.

### What is not spoofed

The UA string is only one of several signals a site can read. `navigator.userAgentData` (Client
Hints), the TLS fingerprint and the JavaScript engine remain those of the real webview, and the
injected bootstrap script only removes the `Electron`/`Tauri`/`wry` product tokens the engines
append. This is deliberate: faking the full fingerprint would require a patched engine, and a
Chromium UA on a WebKit engine advertises capabilities (codecs, WebRTC details) that do not exist.
If a site starts rejecting the app again, the honest fix is the engine, not a longer UA string.

## Requirements

- Node.js 20+ and pnpm
- Rust 1.77.2+ (`rustup`)
- Windows: WebView2 runtime (preinstalled on Windows 10/11; the installer bootstraps it if absent)
- macOS: Xcode command line tools
- Linux: `libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev`

## Development

```bash
pnpm install
pnpm tauri dev
```

`pnpm tauri dev` starts Vite on port 1420 and launches the app.

```bash
cargo test --manifest-path src-tauri/Cargo.toml   # UA resolution + settings
pnpm tauri build                                  # installers in src-tauri/target/release/bundle
```

## Layout

```
index.html, src/           dock + settings UI (bundled, no network access)
src-tauri/src/ua.rs        User-Agent policy: presets, validation, platform default
src-tauri/src/settings.rs  persisted settings and the account list
src-tauri/src/lib.rs       account webviews, tray, commands, session bootstrap script
src-tauri/capabilities/    IPC permissions, split per webview class
```

## Security model

The account webviews render third-party code from `web.whatsapp.com`. Tauri's ACL treats remote
origins as untrusted:

- Custom commands are **unreachable** from a remote page unless a capability explicitly lists the
  origin under `remote.urls`. `capabilities/session.json` grants the `wa-*` webviews only
  `core:event:allow-emit`, so WhatsApp Web code cannot reach any app command even if it calls
  `__TAURI_INTERNALS__.invoke`.
- `capabilities/launcher.json` is `local: true` and bound to the bundled `main` webview (by
  webview label, since every webview shares the one window), which is the only place that can
  change settings or accounts.
- The bundled UI has a Content-Security-Policy; the account webviews cannot have one, because a Tauri
  CSP is injected by the `tauri://` protocol handler and `web.whatsapp.com` is served over HTTPS.
  Do not add an inline `<meta http-equiv="Content-Security-Policy">` to `index.html` — it would be
  a second, unamendable policy that blocks Tauri's own IPC script, which needs a nonce.

## Platform notes

**macOS** — `Info.plist` adds the camera and microphone usage strings. Without them macOS kills the
process the moment WKWebView requests access, which is what makes voice notes and QR scanning crash
instead of degrading. Background throttling is disabled (macOS 14+ only; the other engines ignore
the setting) so timers keep accurate time while an account is hidden.

**Windows** — WebView2's default UA already identifies as Chromium/Edge and is accepted; the
`chrome-windows` preset exists to drop the `Edg/` token if a site treats it differently. The
installer bootstraps the WebView2 runtime when it is missing.

**Linux** — WebKitGTK is the engine that needs the UA override. `deb` bundles depend on
`libwebkit2gtk-4.1-0`. Media playback (`bundleMediaFramework`) is off by default; enable it if
voice messages do not play, at the cost of a larger AppImage. The tray icon needs an AppIndicator
host (GNOME users usually need an extension).

## Interface notes

The launcher is plain HTML/CSS/TypeScript — no UI framework, so there is no component library to
inherit an accessibility baseline from. The rules the markup holds itself to:

- **Two border tokens.** `--line` is decorative (1.4:1 against the surfaces, fine for card edges and
  dividers). Interactive control boundaries use `--line-strong` at 3.1:1, the minimum WCAG 1.4.11
  asks of a non-text UI boundary. Do not put `--line` on an input or button.
- **44px icon targets.** `--target` drives the dock buttons. The dock rail is 64px wide to hold one.
- **Press feedback by opacity, not transform.** Scaling an element inside the dock shifts its
  neighbours, which reads as jitter.
- **Every field has a visible label**, plus an inline error tied to it with `aria-describedby` and
  `role="alert"`. A placeholder is never the only label.
- **One live region** (`#status`, `role="status"`) announces a full sentence for async changes such
  as unread totals — not a bare number, and not one region per badge.
- **Reduced motion** disables the dialog entrance animation and every transition.
- **Themes.** Light and dark share one token set; only the palette blocks differ. Two mechanisms
  cooperate, because neither covers everything:
  - The **page** owns the colours, through `data-theme` on `<html>`. This is the only mechanism that
    reaches every webview: the account webviews are separate views loading WhatsApp Web and the
    overlay is a third one, and there is no cross-platform way to set a theme on a webview
    individually (wry's `with_theme` is WebView2-only).
  - The **native window** theme is set too, so the title bar and native form controls match. It is
    best-effort; where the platform ignores it the page still looks right.

  With the choice set to `system`, `data-theme` is *removed* rather than set to the resolved value.
  That keeps the `prefers-color-scheme` media query in charge, so the stylesheet itself picks up an
  OS change on the very first paint — before the module script runs. `--on-accent` is the one token
  that is not interchangeable between themes: dark ink on the dark accent is 6.3:1, but that same
  ink on the light accent would be 3.9:1, so light mode needs white.
- **Grid panes meet edge to edge.** The multi-account view tiles account webviews with no gap
  between them, and the last column/row is measured from the content edge rather than being given
  the computed cell size. An odd content width divides into a fractional cell (1117 / 2 = 558.5)
  and the engine rounds each view up to a whole pixel, so equal-sized cells both left a 1px seam
  and pushed the final pane 1px past the window edge.

## Error reporting

Sentry is wired up but **off by default**: with no DSN the SDK builds a disabled client, so nothing
is sent and no network call is made.

```bash
WA_SENTRY_DSN="https://<key>@o<org>.ingest.sentry.io/<project>" pnpm tauri dev
```

The DSN is read from `WA_SENTRY_DSN` rather than `SENTRY_DSN`, so an unrelated `SENTRY_DSN` already
in the environment cannot silently turn reporting on. It is deliberately not compiled into the
binary: a DSN is per-deployment, and a baked-in one cannot be pointed at a staging project without a
rebuild.

A missing, blank or malformed DSN disables reporting and logs one line — it never crashes the app.
That is worth stating because the documented tuple form is unsafe here:

```rust
// Panics at startup on a malformed DSN: the tuple impl calls .expect("invalid value for DSN").
sentry::init(("<PII>", ClientOptions { ..Default::default() }))
```

`ClientOptions` is also `#[non_exhaustive]` in `sentry` 0.49, so that struct literal no longer
compiles; `telemetry.rs` uses the setters and assigns `options.dsn` directly, which is equivalent
minus the panic.

### What `send_default_pii` does here

It is enabled, as documented, but be aware of its scope in this SDK:

- The Rust SDK has no HTTP-server integration, so the *"capture user IPs and sensitive headers"*
  behaviour the option is usually reached for does not exist in a desktop app — nothing sends
  headers.
- What it actually gates in `sentry` 0.49 is attaching the current user's id/email to **metrics**
  as attributes.
- `username` is attached independently of this flag and stays empty unless something calls
  `set_user`. This app never does, so no account name or phone number reaches Sentry.

Consequence for future code: with PII enabled, do not put account names, phone numbers or message
content into `capture_message`, `capture_error` or breadcrumbs. `telemetry::capture_error` is the
supported entry point; it walks the error's source chain and tags the event with platform, arch and
webview engine.

### Panic reporting and `panic = "abort"`

The release profile sets `panic = "abort"`. `sentry-panic` captures the panic and flushes inside its
hook, which still runs under abort, so panic events are delivered. The difference is that an abort
does not unwind, so the process dies immediately after the flush instead of unwinding the stack.
`debug-images` is disabled: the release profile strips symbols, which would leave that integration
with nothing to resolve.

## Known limitations

- **Multi-webview is a Tauri `unstable` feature.** `Window::add_child` may change between minor
  Tauri releases; the versions are pinned by `Cargo.lock`.
- **Account isolation needs macOS 14+.** Older WKWebView has no per-webview data store, so every
  account would share one login there.
- **The settings panel replaces the account view.** A native webview cannot be dimmed or overlaid
  by HTML, so opening the settings hides the accounts instead of showing a modal on top of them.
- **Media capture is granted, not configured.** Wry answers WKWebView's media-capture permission
  request with `Grant` and does not surface it to the app, so camera/microphone access cannot be
  made per-origin conditional from here.
- **Notifications are opt-in per site.** `bundle.identifier` is the notification identity; on Linux
  and Windows the app must be installed for the notification centre to accept it.
- **The unread badge is macOS-only.** `set_badge_count` is a no-op on Windows and unsupported on
  Linux; the settings panel reports the engine's capability set rather than pretending otherwise.
- **Downloads** are handled by the engine's default behaviour (WebView2 download UI, WKWebView
  download delegate, WebKitGTK download handler). There is no custom download manager.
