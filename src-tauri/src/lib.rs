//! WhatsApp Web desktop shell.
//!
//! Topology: one `main` window. Its own webview (label `main`, served from the bundled assets) draws
//! the account dock and the settings panel. Every account is a child webview (label `wa-<id>`)
//! that loads <https://web.whatsapp.com> to the right of the dock, with its own data store, so
//! adding or removing an account never touches the others. Only the active account is visible;
//! opening the settings hides them all, which reveals the panel underneath.
//!
//! HTML cannot be drawn over a native webview, so the "Add account" dialog lives in a third kind of
//! webview: a transparent `overlay` that covers the whole window while the dialog is open.
//!
//! Account webviews are built through [`tauri::webview::WebviewBuilder`] because the User-Agent and
//! the data store must be installed at webview creation time. Changing the UA therefore recreates
//! them (the logins live in the data stores and survive).

mod ua;

use serde::Serialize;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Mutex;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::webview::WebviewBuilder;
use tauri::{
    AppHandle, Emitter, Listener, LogicalPosition, LogicalSize, Manager, Url, Webview, WebviewUrl, Window,
};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_notification::NotificationExt as _;
use tauri_plugin_window_state::StateFlags;

mod settings;
use settings::{Account, Settings, ACCOUNT_LABEL_PREFIX, DEFAULT_ACCOUNT_ID};

/// Label of the only window, and of its bundled UI webview.
const MAIN_LABEL: &str = "main";
/// Label of the transparent webview that draws the "Add account" dialog above everything else.
const OVERLAY_LABEL: &str = "overlay";
/// Width of the account dock in logical pixels. Must match `--dock` in `src/styles.css`.
const DOCK_WIDTH: f64 = 64.0;
/// Tray icon id.
const TRAY_ID: &str = "wa-tray";

const WHATSAPP_URL: &str = "https://web.whatsapp.com/";
/// Chat deep link. `phone` is digits only, including the country code.
const WHATSAPP_CHAT_URL: &str = "https://web.whatsapp.com/send?phone=";

/// Platform snapshot for the settings panel and the UA resolver.
struct Platform {
    /// One of `macos`, `windows`, `linux`, `unknown`.
    id: &'static str,
    /// Human readable name of the webview engine backing the window.
    engine: &'static str,
}

fn platform() -> Platform {
    if cfg!(target_os = "macos") {
        Platform {
            id: "macos",
            engine: "WKWebView",
        }
    } else if cfg!(target_os = "windows") {
        Platform {
            id: "windows",
            engine: "WebView2",
        }
    } else if cfg!(target_os = "linux") {
        Platform {
            id: "linux",
            engine: "WebKitGTK",
        }
    } else {
        Platform {
            id: "unknown",
            engine: "webview",
        }
    }
}

/// Per-engine capability report, surfaced in the settings panel so unsupported toggles are visibly
/// disabled instead of silently doing nothing.
///
/// Every field is decided at compile time for the host platform: a Tauri build targets one OS, so
/// there is no runtime platform to parameterise this with.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Supports {
    tray: bool,
    native_notifications: bool,
    badge: bool,
    autostart: bool,
    zoom: bool,
    background_throttling: bool,
    downloads: bool,
}

fn supports() -> Supports {
    Supports {
        tray: true,
        // The notification plugin delegates to the platform's native notification centre, which
        // needs the app to be installed/registered to work reliably.
        native_notifications: cfg!(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux"
        )),
        // `set_badge_count` is a no-op on Windows and unsupported on Linux.
        badge: cfg!(target_os = "macos"),
        autostart: cfg!(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux"
        )),
        zoom: cfg!(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux"
        )),
        // Only macOS >= 14 and iOS >= 17 honour the policy; Linux/Windows ignore it.
        background_throttling: cfg!(target_os = "macos"),
        downloads: true,
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeInfo {
    platform: &'static str,
    webview_version: String,
    app_version: String,
    session_dir: String,
    supports: Supports,
}

/// Live state derived from the account webviews.
#[derive(Default)]
struct SessionState {
    /// Unread counter per webview label, parsed from each page's document title.
    unread: HashMap<String, u32>,
    /// Id of the account currently shown; `None` while the settings panel is open.
    active: Option<String>,
}

fn err(error: impl ToString) -> String {
    error.to_string()
}

fn main_window(app: &AppHandle) -> tauri::Result<Window> {
    app.get_window(MAIN_LABEL).ok_or(tauri::Error::WindowNotFound)
}

fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_window(MAIN_LABEL) {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Every account webview currently alive, with its account id.
fn account_webviews(app: &AppHandle) -> Vec<(String, Webview)> {
    let Some(window) = app.get_window(MAIN_LABEL) else {
        return Vec::new();
    };
    window
        .webviews()
        .into_iter()
        .filter_map(|webview| {
            let id = webview.label().strip_prefix(ACCOUNT_LABEL_PREFIX)?.to_string();
            Some((id, webview))
        })
        .collect()
}

fn account_webview(app: &AppHandle, id: &str) -> Result<Webview, String> {
    app.get_webview(&format!("{ACCOUNT_LABEL_PREFIX}{id}"))
        .ok_or_else(|| format!("account `{id}` has no webview"))
}

/// The area right of the dock, in logical pixels.
fn content_bounds(window: &Window) -> tauri::Result<(LogicalPosition<f64>, LogicalSize<f64>)> {
    let size = window.inner_size()?.to_logical::<f64>(window.scale_factor()?);
    Ok((
        LogicalPosition::new(DOCK_WIDTH, 0.0),
        LogicalSize::new((size.width - DOCK_WIDTH).max(0.0), size.height),
    ))
}

/// Keeps the account webviews glued to the content area. Done by hand rather than with
/// `auto_resize`, which scales proportionally and would let the fixed-width dock drift.
fn layout(window: &Window) {
    let Ok((position, size)) = content_bounds(window) else {
        return;
    };
    for webview in window.webviews() {
        if webview.label().starts_with(ACCOUNT_LABEL_PREFIX) {
            let _ = webview.set_position(position);
            let _ = webview.set_size(size);
        } else if webview.label() == OVERLAY_LABEL {
            let _ = webview.set_size(LogicalSize::new(size.width + DOCK_WIDTH, size.height));
        }
    }
}

/// Shows one account and hides the rest; `None` hides them all to reveal the settings panel.
fn show_account(app: &AppHandle, id: Option<&str>) {
    if let Ok(mut state) = app.state::<Mutex<SessionState>>().lock() {
        state.active = id.map(str::to_string);
    }
    for (account_id, webview) in account_webviews(app) {
        if Some(account_id.as_str()) == id {
            let _ = webview.show();
            let _ = webview.set_focus();
        } else {
            let _ = webview.hide();
        }
    }
}

fn active_account(app: &AppHandle) -> Option<String> {
    app.state::<Mutex<SessionState>>().lock().ok()?.active.clone()
}

#[tauri::command]
fn runtime_info(app: AppHandle) -> RuntimeInfo {
    let platform = platform();
    let session_dir = app
        .path()
        .app_data_dir()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| "unavailable".to_string());

    RuntimeInfo {
        platform: platform.id,
        webview_version: tauri::webview_version()
            .unwrap_or_else(|_| format!("{} (unknown)", platform.engine)),
        app_version: app.package_info().version.to_string(),
        session_dir,
        supports: supports(),
    }
}

#[tauri::command]
fn get_settings(app: AppHandle) -> Settings {
    settings::load(&app, platform().id)
}

// Commands that create webviews are `async`: `Window::add_child` deadlocks on Windows when called
// from a synchronous command.
#[tauri::command]
async fn set_settings(app: AppHandle, mut settings: Settings) -> Result<Settings, String> {
    let platform = platform();
    let previous = settings::load(&app, platform.id);
    // Accounts only change through the account commands, which also manage the webviews.
    settings.accounts = previous.accounts.clone();
    let settings = settings.normalize(platform.id);

    settings::save(&app, &settings).map_err(err)?;

    // Autostart is an OS registration, not a stored preference.
    if settings.autostart != previous.autostart {
        let autolaunch = app.autolaunch();
        let result = if settings.autostart {
            autolaunch.enable()
        } else {
            autolaunch.disable()
        };
        if let Err(error) = result {
            return Err(format!("autostart: {error}"));
        }
    }

    // The UA is baked into a webview at creation time, so it only takes effect on a rebuild.
    if settings.user_agent() != previous.user_agent() {
        let active = active_account(&app);
        for (_, webview) in account_webviews(&app) {
            let _ = webview.close();
        }
        for account in &settings.accounts {
            build_account_webview(&app, account, &settings).map_err(err)?;
        }
        show_account(&app, active.as_deref());
    } else {
        for (_, webview) in account_webviews(&app) {
            let _ = webview.set_zoom(settings.zoom);
        }
    }

    Ok(settings)
}

#[tauri::command]
fn ua_options() -> serde_json::Value {
    json!({
        "snapshot": ua::snapshot_date(),
        "presets": ua::options(),
    })
}

/// Opens the "Add account" dialog in a transparent webview covering the whole window, so the
/// current account stays visible, dimmed, behind it. Built on demand: the newest child webview is
/// the topmost one, and nothing can reorder them afterwards.
#[tauri::command]
async fn open_add_dialog(app: AppHandle) -> Result<(), String> {
    if app.get_webview(OVERLAY_LABEL).is_some() {
        return Ok(());
    }
    let window = main_window(&app).map_err(err)?;
    let (_, size) = content_bounds(&window).map_err(err)?;
    let builder = WebviewBuilder::new(
        OVERLAY_LABEL,
        WebviewUrl::App("index.html#add-account-dialog".into()),
    )
    .transparent(true);
    let webview = window
        .add_child(
            builder,
            LogicalPosition::new(0.0, 0.0),
            LogicalSize::new(size.width + DOCK_WIDTH, size.height),
        )
        .map_err(err)?;
    let _ = webview.set_focus();
    Ok(())
}

#[tauri::command]
fn close_add_dialog(app: AppHandle) {
    if let Some(webview) = app.get_webview(OVERLAY_LABEL) {
        let _ = webview.close();
    }
    // Hands keyboard focus back to whatever is on screen.
    show_account(&app, active_account(&app).as_deref());
}

/// Adds an account with a fresh, isolated data store and switches to it. The other accounts'
/// webviews are not touched. Called from the dialog overlay, so the dock is told through an event.
#[tauri::command]
async fn add_account(app: AppHandle, name: String) -> Result<Settings, String> {
    let platform = platform();
    let mut settings = settings::load(&app, platform.id);
    let name = match name.trim() {
        "" => format!("Account {}", settings.accounts.len() + 1),
        name => name.to_string(),
    };
    let account = Account::new(name);
    settings.accounts.push(account.clone());
    let settings = settings.normalize(platform.id);
    settings::save(&app, &settings).map_err(err)?;

    build_account_webview(&app, &account, &settings).map_err(err)?;
    show_account(&app, Some(&account.id));
    let _ = app.emit_to(MAIN_LABEL, "accounts://added", &settings);
    Ok(settings)
}

/// Unlinks an account: wipes its data store, closes its webview and forgets it.
#[tauri::command]
fn remove_account(app: AppHandle, id: String) -> Result<Settings, String> {
    let mut settings = settings::load(&app, platform().id);
    if settings.accounts.len() <= 1 {
        return Err("the last account cannot be removed; clear its data instead".into());
    }
    let index = settings
        .accounts
        .iter()
        .position(|account| account.id == id)
        .ok_or_else(|| format!("unknown account `{id}`"))?;
    let account = settings.accounts.remove(index);
    settings::save(&app, &settings).map_err(err)?;

    if let Some(webview) = app.get_webview(&account.label()) {
        let _ = webview.clear_all_browsing_data();
        let _ = webview.close();
    }
    // WebView2/WebKitGTK keep the profile in a directory; best effort, the engine may still hold it.
    #[cfg(not(target_os = "macos"))]
    if let Some(dir) = account_data_dir(&app, &account) {
        let _ = std::fs::remove_dir_all(dir);
    }

    let was_active = {
        let state = app.state::<Mutex<SessionState>>();
        let mut state = state.lock().map_err(|_| "state is poisoned".to_string())?;
        state.unread.remove(&account.label());
        state.active.as_deref() == Some(id.as_str())
    };
    if was_active {
        show_account(&app, settings.accounts.first().map(|account| account.id.as_str()));
    }
    Ok(settings)
}

#[tauri::command]
fn rename_account(app: AppHandle, id: String, name: String) -> Result<Settings, String> {
    let platform = platform();
    let mut settings = settings::load(&app, platform.id);
    let account = settings
        .accounts
        .iter_mut()
        .find(|account| account.id == id)
        .ok_or_else(|| format!("unknown account `{id}`"))?;
    account.name = name;
    let settings = settings.normalize(platform.id);
    settings::save(&app, &settings).map_err(err)?;
    Ok(settings)
}

/// Shows an account, or the settings panel when `id` is `None`.
#[tauri::command]
fn switch_account(app: AppHandle, id: Option<String>) -> Result<(), String> {
    if let Some(id) = &id {
        account_webview(&app, id)?;
    }
    show_account(&app, id.as_deref());
    Ok(())
}

/// Opens a single chat in the given account.
///
/// `jid` may be a bare phone number (digits, country code included) or a full WhatsApp JID; only
/// the leading digits are used.
#[tauri::command]
fn open_chat(app: AppHandle, id: String, jid: String) -> Result<(), String> {
    let digits: String = jid.trim().chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return Err(format!("`{jid}` does not start with a phone number"));
    }

    account_webview(&app, &id)?
        .navigate(parse_url(&format!("{WHATSAPP_CHAT_URL}{digits}")))
        .map_err(err)?;
    show_account(&app, Some(&id));
    Ok(())
}

/// Reloads an account and brings it on screen, so the reload is visible from the settings panel.
#[tauri::command]
fn reload_session(app: AppHandle, id: String) -> Result<(), String> {
    account_webview(&app, &id)?.reload().map_err(err)?;
    show_account(&app, Some(&id));
    Ok(())
}

/// Wipes cookies, cache and local storage of one account, forcing a re-link, then shows its QR.
#[tauri::command]
async fn clear_session_data(app: AppHandle, id: String) -> Result<(), String> {
    let webview = account_webview(&app, &id)?;
    // WhatsApp Web has to be unloaded first: a live page keeps its session in memory and writes it
    // straight back to storage, which is how a wipe followed by a plain reload stays logged in.
    webview
        .navigate(Url::parse("about:blank").map_err(err)?)
        .map_err(err)?;
    // ponytail: fixed waits, because neither the navigation nor the engine's data removal reports
    // completion through wry. If a wipe ever leaves the account linked, drive WKWebsiteDataStore /
    // WebView2 directly via `with_webview` and use their completion handlers.
    std::thread::sleep(std::time::Duration::from_millis(300));
    webview.clear_all_browsing_data().map_err(err)?;
    std::thread::sleep(std::time::Duration::from_millis(700));

    if let Ok(mut state) = app.state::<Mutex<SessionState>>().lock() {
        state.unread.remove(webview.label());
    }
    webview.navigate(parse_url(WHATSAPP_URL)).map_err(err)?;
    show_account(&app, Some(&id));
    Ok(())
}

#[tauri::command]
fn test_notification(app: AppHandle) -> Result<(), String> {
    let settings = settings::load(&app, platform().id);
    if !settings.native_notifications {
        return Err("native notifications are disabled in the settings".into());
    }

    app.notification()
        .builder()
        .title("WA Des Des")
        .body("Native notifications are wired up.")
        .show()
        .map_err(err)
}

/// Parses a compile-time-known WhatsApp URL.
///
/// The literals above are constant, so a failure is a programming error rather than user input;
/// panicking keeps the signature free of an impossible error path.
fn parse_url(url: &str) -> Url {
    Url::parse(url).expect("hard-coded WhatsApp URL is valid")
}

/// Profile directory of a non-default account on the engines that take one (WebView2, WebKitGTK).
#[cfg(not(target_os = "macos"))]
fn account_data_dir(app: &AppHandle, account: &Account) -> Option<std::path::PathBuf> {
    account.store_id()?;
    Some(app.path().app_data_dir().ok()?.join("accounts").join(&account.id))
}

/// Creates the account's webview if it does not exist yet, hidden, with the configured UA, zoom,
/// bootstrap script and an isolated data store.
fn build_account_webview(
    app: &AppHandle,
    account: &Account,
    settings: &Settings,
) -> tauri::Result<Webview> {
    let label = account.label();
    if let Some(existing) = app.get_webview(&label) {
        return Ok(existing);
    }
    let window = main_window(app)?;

    let mut builder = WebviewBuilder::new(&label, WebviewUrl::External(parse_url(WHATSAPP_URL)))
        .zoom_hotkeys_enabled(true)
        .enable_clipboard_access()
        .initialization_script(SESSION_BOOTSTRAP);

    // The UA has to be installed before the first navigation; there is no runtime setter.
    if let Some(user_agent) = settings.user_agent() {
        builder = builder.user_agent(&user_agent);
    }

    // Isolation is what makes the accounts independent logins. The default account keeps the
    // engine's default store. WKWebView has no data directory, only store identifiers (macOS 14+;
    // older systems fall back to the shared store, i.e. a single login).
    debug_assert!(account.id == DEFAULT_ACCOUNT_ID || account.store_id().is_some());
    #[cfg(target_os = "macos")]
    if let Some(store_id) = account.store_id() {
        builder = builder.data_store_identifier(store_id);
    }
    #[cfg(not(target_os = "macos"))]
    if let Some(dir) = account_data_dir(app, account) {
        builder = builder.data_directory(dir);
    }

    // WhatsApp Web never unloads, and hidden accounts must keep receiving messages, so the platform
    // default suspend policy only costs us. Only macOS exposes the switch.
    if cfg!(target_os = "macos") {
        builder =
            builder.background_throttling(tauri::utils::config::BackgroundThrottlingPolicy::Disabled);
    }

    let (position, size) = content_bounds(&window)?;
    let webview = window.add_child(builder, position, size)?;
    let _ = webview.set_zoom(settings.zoom);
    // Hidden until `show_account` picks it, so the settings panel or another account stays on top.
    let _ = webview.hide();
    Ok(webview)
}

/// Script injected at document start into every session webview.
///
/// Responsibilities, in order:
/// 1. Strip the tokens WhatsApp Web sniffs to reject embedded webviews (`Electron/`, `Tauri/`, and
///    the `wry` product token some engines append).
/// 2. Detect "browser not supported" responses and tell the settings panel, so the UA preset can
///    be changed without opening the web inspector.
/// 3. Provide a `Notification` implementation that is routed to native notifications.
/// 4. Report the unread counter to the Rust side.
///
/// The script is deliberately dependency-free and defensive: WhatsApp Web is a minified bundle that
/// can change at any deploy, so every lookup is optional.
const SESSION_BOOTSTRAP: &str = r#"
(() => {
  const strip = (value) =>
    value
      .replace(/\s*(Electron|Tauri|tauri|wry|wry\/[0-9.]+)\/[0-9A-Za-z.\-]+/g, '')
      .replace(/\s{2,}/g, ' ')
      .trim();

  const patchUserAgentData = () => {
    const uaData = navigator.userAgentData;
    if (!uaData) return;
    Object.defineProperty(uaData, 'brands', {
      configurable: true,
      get: () => [
        { brand: 'Chromium', version: '145' },
        { brand: 'Google Chrome', version: '145' },
        { brand: 'Not?A_Brand', version: '24' },
      ],
    });
    Object.defineProperty(uaData, 'mobile', { configurable: true, get: () => false });
  };

  const sanitize = () => {
    const clean = strip(navigator.userAgent);
    if (clean !== navigator.userAgent) {
      Object.defineProperty(navigator, 'userAgent', {
        configurable: true,
        get: () => clean,
      });
      Object.defineProperty(navigator, 'appVersion', {
        configurable: true,
        get: () => clean.replace('Mozilla/', ''),
      });
    }
    patchUserAgentData();
  };

  sanitize();
  document.addEventListener('DOMContentLoaded', sanitize);

  const label = (() => {
    try {
      return window.__TAURI_INTERNALS__?.metadata?.currentWebview?.label || 'wa-default';
    } catch (error) {
      return 'wa-default';
    }
  })();

  const report = (event, payload) => {
    try {
      window.__TAURI_INTERNALS__?.invoke('plugin:event|emit', {
        event,
        payload,
      });
    } catch (error) {
      console.debug('[wa] event failed', event, error);
    }
  };

  // Web notifications. WKWebView has no Notification API at all, and the other engines gate theirs
  // behind a permission prompt this shell never answers, so WhatsApp Web's "turn on notifications"
  // could not succeed. This stand-in always reports "granted" and forwards each notification to the
  // Rust side, which shows it through the OS (or drops it when the setting is off).
  class ShellNotification extends EventTarget {
    constructor(title, options) {
      super();
      const opts = options || {};
      this.title = String(title);
      this.body = String(opts.body ?? '');
      this.tag = String(opts.tag ?? '');
      this.data = opts.data ?? null;
      this.onclick = this.onclose = this.onerror = this.onshow = null;
      report('session://notification', { title: this.title, body: this.body, label });
    }
    close() {}
    static requestPermission(callback) {
      if (typeof callback === 'function') callback('granted');
      return Promise.resolve('granted');
    }
  }
  ShellNotification.permission = 'granted';
  ShellNotification.maxActions = 0;
  Object.defineProperty(window, 'Notification', {
    configurable: true,
    writable: true,
    value: ShellNotification,
  });

  try {
    const query = navigator.permissions.query.bind(navigator.permissions);
    navigator.permissions.query = (descriptor) =>
      descriptor && descriptor.name === 'notifications'
        ? Promise.resolve({ state: 'granted', onchange: null, addEventListener() {}, removeEventListener() {} })
        : query(descriptor);
  } catch (error) {
    // No Permissions API on this engine; Notification.permission above is enough.
  }

  const UNSUPPORTED = /browser (is )?not supported|update your browser|unsupported browser|please use the (latest )?(google )?chrome/i;
  const flagUnsupported = () => {
    const text = document.body ? document.body.innerText || '' : '';
    if (UNSUPPORTED.test(text.slice(0, 4000))) {
      report('session://unsupported', { userAgent: navigator.userAgent, label });
    }
  };

  const readUserAgent = () => {
    report('session://user-agent', { userAgent: navigator.userAgent, label });
  };

  const readUnread = () => {
    const match = /^\((\d+)\)/.exec(document.title || '');
    report('session://unread', { count: match ? Number(match[1]) : 0, label });
  };

  let pending = false;
  const schedule = (task) => () => {
    if (pending) return;
    pending = true;
    requestAnimationFrame(() => {
      pending = false;
      task();
    });
  };

  const observer = new MutationObserver(
    schedule(() => {
      flagUnsupported();
      readUnread();
    }),
  );

  const start = () => {
    observer.observe(document.documentElement, {
      childList: true,
      subtree: true,
      characterData: true,
    });
    schedule(() => {
      flagUnsupported();
      readUnread();
      readUserAgent();
    })();
  };

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', start);
  } else {
    start();
  }
})();
"#;

/// Menu wiring for the tray.
fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open WhatsApp", true, None::<&str>)?;
    let reload = MenuItem::with_id(app, "reload", "Reload current account", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[&open, &reload, &PredefinedMenuItem::separator(app)?, &quit],
    )?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .tooltip("WA Des Des")
        // On Linux/Windows the left click is the natural "show the app" gesture, so the menu is
        // bound to the right click only.
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            let id: &str = event.id.as_ref();
            match id {
                "open" => show_main(app),
                "reload" => {
                    if let Some(webview) =
                        active_account(app).and_then(|id| account_webview(app, &id).ok())
                    {
                        let _ = webview.reload();
                    }
                }
                "quit" => app.exit(0),
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }

    builder.build(app)?;
    Ok(())
}

/// Keeps the app badge in sync with the WhatsApp pages.
///
/// The bootstrap script reports `{ label, count }`; the webview label is what tells the accounts
/// apart. The dock listens to the same event for its per-account badges.
fn watch_session(app: &AppHandle) {
    let handle = app.clone();
    app.listen("session://unread", move |event| {
        let Ok(payload) = serde_json::from_str::<serde_json::Value>(event.payload()) else {
            return;
        };
        let Some(label) = payload.get("label").and_then(serde_json::Value::as_str) else {
            return;
        };
        let count = payload
            .get("count")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0) as u32;

        let total = {
            let state = handle.state::<Mutex<SessionState>>();
            let Ok(mut state) = state.lock() else {
                return;
            };
            if state.unread.get(label).copied() == Some(count) {
                return;
            }
            state.unread.insert(label.to_string(), count);
            state.unread.values().copied().sum::<u32>()
        };

        let settings = settings::load(&handle, platform().id);
        if !settings.badge_unread_count {
            return;
        }

        let Some(window) = handle.get_window(MAIN_LABEL) else {
            return;
        };

        #[cfg(target_os = "macos")]
        {
            let _ = window.set_badge_label(if total == 0 {
                None
            } else {
                Some(total.to_string())
            });
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (window, total);
        }
    });
}

/// Shows the notifications WhatsApp Web raises through the bootstrap script's `Notification`.
fn watch_notifications(app: &AppHandle) {
    let handle = app.clone();
    app.listen("session://notification", move |event| {
        let Ok(payload) = serde_json::from_str::<serde_json::Value>(event.payload()) else {
            return;
        };
        // The payload comes from a remote page: treat it as untrusted text and bound its size.
        let text = |key: &str, max: usize| -> String {
            payload
                .get(key)
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .chars()
                .take(max)
                .collect()
        };
        let label = text("label", 64);

        let settings = settings::load(&handle, platform().id);
        if !settings.native_notifications {
            return;
        }

        let mut title = text("title", 100);
        if title.is_empty() {
            title = "WhatsApp".to_string();
        }
        // With several accounts, say which one the message is for.
        if settings.accounts.len() > 1 {
            if let Some(account) = settings.accounts.iter().find(|a| a.label() == label) {
                title = format!("{title} · {}", account.name);
            }
        }

        let _ = handle
            .notification()
            .builder()
            .title(title)
            .body(text("body", 300))
            .show();
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default();

    // Must be the first plugin: a second launch has to hand over to the running instance and exit,
    // before any window is created.
    builder = builder.plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
        show_main(app);
    }));

    builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(
            tauri_plugin_window_state::Builder::new()
                .with_state_flags(StateFlags::SIZE | StateFlags::POSITION)
                .build(),
        )
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(Mutex::new(SessionState::default()))
        .invoke_handler(tauri::generate_handler![
            runtime_info,
            get_settings,
            set_settings,
            ua_options,
            open_add_dialog,
            close_add_dialog,
            add_account,
            remove_account,
            rename_account,
            switch_account,
            open_chat,
            reload_session,
            clear_session_data,
            test_notification,
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            build_tray(&handle)?;
            watch_session(&handle);
            watch_notifications(&handle);

            // Every account is loaded up front so hidden ones still receive messages.
            let settings = settings::load(&handle, platform().id);
            for account in &settings.accounts {
                build_account_webview(&handle, account, &settings)?;
            }
            show_account(&handle, settings.accounts.first().map(|account| account.id.as_str()));

            Ok(())
        })
        // Closing the window hides it; the tray keeps the app reachable and the accounts online.
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                let _ = window.hide();
            }
            tauri::WindowEvent::Resized(_) | tauri::WindowEvent::ScaleFactorChanged { .. } => {
                layout(window);
            }
            _ => {}
        })
        .run(tauri::generate_context!())
        .expect("error while running wa-des-des");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_ids_are_stable() {
        let platform = platform();
        assert!(matches!(
            platform.id,
            "macos" | "windows" | "linux" | "unknown"
        ));
        assert!(!platform.engine.is_empty());
    }

    #[test]
    fn supported_platform_gets_the_engine_ua_by_default() {
        let settings = Settings::for_platform(platform().id);
        assert!(ua::is_known_preset(&settings.user_agent_preset));
    }

    #[test]
    fn supports_matches_the_host_engine() {
        let supports = supports();
        assert_eq!(supports.badge, cfg!(target_os = "macos"));
        assert_eq!(
            supports.background_throttling,
            cfg!(target_os = "macos"),
            "only macOS honours BackgroundThrottlingPolicy"
        );
        assert!(supports.tray, "tray is available on every desktop target");
    }

    #[test]
    fn chat_urls_are_built_from_digits_only() {
        let digits: String = "62812-345 678@c.us"
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        assert_eq!(digits, "62812");
        assert_eq!(
            format!("{WHATSAPP_CHAT_URL}{digits}"),
            "https://web.whatsapp.com/send?phone=62812"
        );
    }

    #[test]
    fn bootstrap_script_strips_embedded_webview_tokens() {
        assert!(SESSION_BOOTSTRAP.contains("Electron"));
        assert!(SESSION_BOOTSTRAP.contains("Tauri"));
        assert!(SESSION_BOOTSTRAP.contains("wry"));
        assert!(SESSION_BOOTSTRAP.contains("session://user-agent"));
        assert!(SESSION_BOOTSTRAP.contains("session://unread"));
        assert!(SESSION_BOOTSTRAP.contains("currentWebview"));
        assert!(SESSION_BOOTSTRAP.contains("session://notification"));
    }

    #[test]
    fn whatsapp_urls_are_https() {
        assert!(WHATSAPP_URL.starts_with("https://"));
        assert!(WHATSAPP_CHAT_URL.starts_with("https://"));
    }
}
