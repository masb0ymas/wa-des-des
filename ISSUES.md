# Security issues

Audit of the working tree on 2026-10-08 (v0.1.4 plus the uncommitted Telegram/Slack work).
Method: code read of `src-tauri/`, `src/`, the capabilities, the release workflow and the bundled
config; `wry` 0.57.0 source for engine defaults; `pnpm audit`. Nothing was exploited against a
running build, and Rust dependencies were **not** audited (`cargo-audit` is not installed).

Threat model: the account webviews run third-party code (WhatsApp Web, Telegram Web, Slack). The
shell must keep that code, and anything it navigates to, away from the app's commands, from other
accounts and from the device.

| ID  | Severity | Issue                                                                     | Status                    |
| --- | -------- | ------------------------------------------------------------------------- | ------------------------- |
| H1  | High     | Account webviews navigate anywhere, and every page gets camera/microphone | Fixed; Slack partly       |
| M1  | Medium   | Remote pages can forge app events                                         | Fixed                     |
| M2  | Medium   | Session capability covers more origins than it needs                      | Fixed                     |
| M3  | Medium   | Release job runs mutable third-party actions next to the signing key      | Fixed                     |
| M4  | Medium   | Release binaries are unsigned; users are told to bypass Gatekeeper        | **Open**                  |
| M5  | Medium   | Account isolation silently absent on macOS 11–13                          | Fixed                     |
| L1  | Low      | "Remove" and "Clear data" do not confirm the wipe                         | Remove fixed; Clear open  |
| L2  | Low      | Pages can open the system browser without a user gesture                  | Fixed                     |
| L3  | Low      | Vulnerable development dependencies                                       | Fixed except `braces`     |
| L4  | Low      | Telemetry is configured with `send_default_pii(true)`                     | Fixed                     |
| L5  | Low      | `NSAllowsLocalNetworking` ships in production                             | Fixed                     |

No critical issue was found.

The fixes pass `cargo test` and `pnpm build`. They have **not** been exercised in a running app:
log in to each service, place a call, open a link and remove an account before releasing. Each
entry below keeps the original finding and ends with what was done.

## High

### H1 — Account webviews navigate anywhere, and every page gets camera/microphone

- **Where:** `build_account_webview`, `src-tauri/src/lib.rs:758` (no `on_navigation`, no permission
  handler; `enable_clipboard_access()` at line 771).
- **What:** nothing restricts where an account webview's top frame can go. Whatever page it lands
  on is shown inside WaDesk with no address bar, and inherits what the webview was given:
  - macOS: `wry` answers every `getUserMedia` request with `WKPermissionDecision::Grant` when the
    app installs no permission handler (`wry_web_view_ui_delegate.rs`, the `else` branch). After
    the user has approved the one-time OS prompt for WaDesk, **any origin** in that webview gets
    the camera and microphone with no per-site prompt.
  - Linux/Windows: `enable_clipboard_access()` lets page script read the clipboard.
  - All platforms: the page looks like a trusted account pane, which makes it a phishing surface.
- **Scenario:** a same-tab link, a redirect or an injected script in one of the three services
  navigates the pane to `attacker.example`; that page records audio/video or reads the clipboard.
- **Fix:** add `on_navigation` with a per-service host allowlist and send everything else to the
  system browser (the `on_new_window` handler already does this for new windows). Install a
  permission handler that allows camera/microphone only for the account's own service origin.
  Slack's SSO sign-in passes through third-party identity providers in the same tab, so its
  allowlist needs a decision: open sign-in externally, or allow navigation but never media.
- **Done:** `build_account_webview` now has a page-load hook and a permission hook. A top-level
  load of another site is opened in the system browser and the pane returns to its service (to
  `about:blank` if that repeats within 10 s, so a captive portal cannot loop it). Permission
  requests are denied whenever the top frame is off the service's domain. `on_navigation` was not
  used: WKWebView calls it for iframes too, without saying which frame.
- **Still open:** Slack panes may leave the site, so the phishing surface remains there; they get
  no camera/microphone while away. The bounce happens once the foreign page starts loading, so on
  Linux/Windows its script can still read the clipboard for a moment.

## Medium

### M1 — Remote pages can forge app events

- **Where:** `src-tauri/capabilities/session.json` (`core:event:allow-emit`),
  `src/main.ts:368`, `watch_session` / `watch_notifications` in `src-tauri/src/lib.rs`.
- **What:** `allow-emit` lets a remote page emit **any** event name, to every listener, and the
  payload carries no trustworthy sender.
  - `accounts://added` is meant to come from the backend, but the dock applies whatever settings
    object arrives. The saved account list is safe (`set_settings` overwrites `accounts` from
    disk), but the forged User-Agent, zoom, notification and autostart values are written out the
    next time the user changes any setting, and the dock shows forged accounts until restart.
  - `session://unread` and `session://notification` take the webview label from the payload, so
    one account's page can fake another account's badge or raise a notification in its name.
  - `update://progress` and `notifications://denied` can be forged too (cosmetic).
- **Fix:** replace the emit permission with one command, e.g. `session_report(kind, payload)`,
  that takes the calling `Webview` from Tauri and derives the label from it; grant only that
  command to `wa-*` and drop `core:event:allow-emit`. Independently, make the `accounts://added`
  listener ignore its payload and call `get_settings`.
- **Done:** both. `session_report` is the only command in the session capability, and
  `core:event:allow-emit` is gone, so pages can no longer emit events at all.

### M2 — Session capability covers more origins than it needs

- **Where:** `src-tauri/capabilities/session.json`, `remote.urls`.
- **What:** IPC is granted to `https://*.whatsapp.com/*`, `https://*.whatsapp.net/*` and
  `https://*.slack.com/*` (the Slack wildcard is part of the uncommitted work). Only the app pages
  run the bootstrap script's reporting. Every extra host — media CDNs, marketing sites, workspace
  subdomains — is an origin from which M1 can be reached.
- **Fix:** `https://web.whatsapp.com/*`, `https://web.telegram.org/*`, `https://app.slack.com/*`.
- **Done:** as above.

### M3 — Release job runs mutable third-party actions next to the signing key

- **Where:** `.github/workflows/release.yml` (lines 28–66).
- **What:** the job has `contents: write` and receives `TAURI_SIGNING_PRIVATE_KEY`, and every
  action is referenced by a movable tag (`tauri-apps/tauri-action@v0`,
  `dtolnay/rust-toolchain@stable`, `swatinem/rust-cache@v2`, `pnpm/action-setup@v4`,
  `actions/*@v5`). Whoever can move one of those tags can alter the toolchain or the build, or
  read the key. The updater trusts anything signed with that key, so this is the path to shipping
  a malicious update to every install. Likelihood is low; impact is the whole user base.
- **Fix:** pin each action to a commit SHA, and keep them current with Dependabot/Renovate.
- **Done:** every action is pinned to the commit its tag pointed at on 2026-10-08. No update bot
  was added.

### M4 — Release binaries are unsigned; users are told to bypass Gatekeeper

- **Where:** `src-tauri/tauri.conf.json:79` (`"signingIdentity": "-"`), release body in
  `.github/workflows/release.yml:74`.
- **What:** macOS builds are ad-hoc signed and not notarized, Windows builds carry no certificate,
  and the release notes tell users to run `xattr -cr`. A first install therefore has no publisher
  check at all, and users learn to strip quarantine from this app. In-app updates are fine: they
  are verified against the minisign public key in the config.
- **Fix:** Developer ID signing plus notarization on macOS, a code-signing certificate on Windows.
  Until then, publish checksums with each release.
- **Open:** needs an Apple Developer ID and a Windows certificate, which are not in the repo.

### M5 — Account isolation silently absent on macOS 11–13

- **Where:** `src-tauri/tauri.conf.json:78` (`minimumSystemVersion: "11.0"`),
  `build_account_webview`.
- **What:** per-account data stores need macOS 14. On older systems `wry` falls back to the
  default store, so two accounts of the same service share one login, with no warning. Accounts of
  different services stay apart, because they are different origins.
- **Fix:** raise the minimum to 14.0, or refuse to add a second account of a service there.
- **Done:** `add_account` refuses a second account of the same service below macOS 14 and says
  why in the activity log. The minimum version is unchanged, so nobody loses the app. Accounts
  that already share a login on such a system are left as they are.

## Low

### L1 — "Remove" and "Clear data" do not confirm the wipe

- **Where:** `remove_account` (`src-tauri/src/lib.rs:538`), `clear_session_data` (line 639).
- **What:** `clear_all_browsing_data` is asynchronous and reports no completion. `remove_account`
  closes the webview straight after calling it, and on macOS the data-store directory itself is
  never removed; `clear_session_data` relies on fixed sleeps. The UI says the login "is deleted
  from this device", which is not guaranteed.
- **Fix:** on macOS remove the store by identifier and wait for the completion handler; elsewhere
  delete the profile directory after the webview has closed.
- **Done:** `remove_account` removes the macOS data store by identifier after closing the
  webview, retrying for 1.5 s while the engine still holds it.
- **Still open:** `clear_session_data` keeps its fixed waits; Linux/Windows removal is unchanged.

### L2 — Pages can open the system browser without a user gesture

- **Where:** `on_new_window`, `src-tauri/src/lib.rs:775`.
- **What:** any `window.open` from an account page opens the default browser. The scheme
  allowlist (`http`, `https`, `mailto`) is right, but there is no gesture check or rate limit, so a
  page can open arbitrary sites or flood the browser with tabs.
- **Fix:** rate-limit per webview.
- **Done:** `open_external` allows one URL per 500 ms across the app.

### L3 — Vulnerable development dependencies

- **Where:** `package.json`; `pnpm audit` reports 17 advisories (7 high, 7 moderate, 3 low).
- **What:** `vite` 7.1.12 has dev-server file-read and `server.fs.deny` bypass advisories (fixed
  after 7.3.4); `release-it` pulls in vulnerable `undici`, `basic-ftp` and `braces`. None of it is
  shipped in the app — the exposure is the developer's machine while `pnpm dev` runs.
- **Fix:** bump `vite` past 7.3.4 and `release-it`; add `cargo audit` and `pnpm audit` to CI.
- **Done:** `vite` 7.3.7; `undici` and `basic-ftp` forced to patched releases through
  `overrides` in `pnpm-workspace.yaml`. `pnpm audit` is down to one advisory.
- **Still open:** `braces` (via `@release-it/bumper`) has no patched release to move to. No audit
  step was added to CI, and the Rust dependencies remain unaudited.

### L4 — Telemetry is configured with `send_default_pii(true)`

- **Where:** `src-tauri/src/telemetry.rs:65`.
- **What:** reporting is off unless `WA_SENTRY_DSN` is set, so nothing is sent today. If it is
  ever enabled, the PII flag is already on, in an app whose data is private conversations.
- **Fix:** set it to `false`, or remove the module (nothing sets the DSN).
- **Done:** set to `false`.

### L5 — `NSAllowsLocalNetworking` ships in production

- **Where:** `src-tauri/Info.plist:18`.
- **What:** the exception allows cleartext HTTP to local addresses from every webview. It appears
  to be needed only for the dev server (`http://localhost:1420`); the bundled app serves its UI
  over the `tauri://` scheme. Worth confirming, then removing from release builds.
- **Done:** the key is removed. Not confirmed by running `pnpm tauri dev`; if the dev window
  comes up blank, this is the change to revert.

## Checked and found sound

- Custom commands are reachable only from the bundled `main` and `overlay` webviews
  (`capabilities/launcher.json`, `local: true`).
- The launcher has a CSP, `withGlobalTauri` is off, and the UI writes untrusted text with
  `textContent` only — no `innerHTML`, no `eval`.
- Account ids are validated as hex before they become webview labels, directory names or store
  identifiers; `open_chat` builds its URL from digits only.
- Notification text from pages is treated as untrusted and length-bounded.
- Updates are fetched over HTTPS and verified against a pinned public key.
- No secrets or private keys in the tracked files.
- On macOS 14+ each added account has its own data store on disk; "Clear data" and "Remove" act on
  that account's store only.
