---
title: "Title-bar double-click Fill on macOS breaks only in a long-running instance, not in fresh builds"
author: Masb0ymas
date: 2026-11-23
branch: main
cost: { minutes: 40, tokens: 197120 }
id: l-46b34532
scope: repo
status: active
src: agent
touches:
  - src-tauri/tauri.conf.json
  - src-tauri/src/lib.rs
---

## Decided
- No code change yet for "double-click on the title bar does not fill the window" (reported on macOS 27). The trigger is still unknown, so any fix now would be a guess.
- The bug is runtime state, not config or code path: a fresh launch fills correctly, and quitting and relaunching WaDesk fixes it.

## Tried and ruled out
- The native title bar is real. `titleBarStyle: "Transparent"` keeps the content view below it (webviews start 32pt down), so no webview covers it and no drag region is needed.
- Fresh debug build, fresh release `.app` (separate identifier), the same 6 accounts copied from the user's settings, main and second display, hide→show via close button + relaunch, Fill via the Window menu then resize, switching accounts, opening Settings, cycling between them, and dragging the window across displays: double-click filled every time.
- In the broken instance, dragging the title bar still moved the window, and Window › Fill / Window › Zoom still worked while the app was active. Only the title-bar double-click was ignored, on both displays.
- No custom double-click handling exists: tao, wry and this app do not intercept title-bar clicks.

## Watch out
- The broken instance had an extra off-screen window at CG layer 103, alpha 0.1, 97x21, near the dock (likely a stuck tooltip). Fresh instances had none. It's a lead, not confirmed.
- Synthetic clicks from a shell process are ignored without Accessibility permission (`AXIsProcessTrusted` false). A cua-driver double-click (`count:2`, `delivery_mode:"foreground"`) works, but each CLI call needs the same `session` label and stdin from `/dev/null`, or it hangs in scripts.
- lldb cannot attach to the installed app (hardened runtime, ad-hoc signed).

In the code:
- `src-tauri/tauri.conf.json`: window `titleBarStyle: "Transparent"`
- `src-tauri/src/lib.rs`: `on_window_event` → `layout()` → `apply_view()` runs on every `Resized`, including `webview.set_focus()`
