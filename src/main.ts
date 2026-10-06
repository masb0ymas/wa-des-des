import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  applyTheme,
  isThemeChoice,
  nextThemeChoice,
  themeLabel,
  watchSystemTheme,
  type ThemeChoice,
} from "./theme";

interface Supports {
  tray: boolean;
  nativeNotifications: boolean;
  badge: boolean;
  autostart: boolean;
  zoom: boolean;
  backgroundThrottling: boolean;
  downloads: boolean;
}

interface RuntimeInfo {
  platform: string;
  webviewVersion: string;
  appVersion: string;
  sessionDir: string;
  supports: Supports;
}

interface Account {
  id: string;
  name: string;
}

interface Settings {
  userAgentPreset: string;
  customUserAgent: string;
  nativeNotifications: boolean;
  badgeUnreadCount: boolean;
  autostart: boolean;
  zoom: number;
  accounts: Account[];
  gridCols: number;
  gridRows: number;
  theme: ThemeChoice;
}

interface UaOption {
  id: string;
  label: string;
  value: string;
  note: string;
}

interface UaOptions {
  snapshot: string;
  presets: UaOption[];
}

// Tauri injects `isTauri` on the main frame of every webview; absent in a plain browser preview.
const inTauri = "isTauri" in window;

function el<T extends HTMLElement>(id: string): T {
  const node = document.getElementById(id);
  if (!node) throw new Error(`missing #${id}`);
  return node as T;
}

function log(level: string, message: string): void {
  const list = el<HTMLOListElement>("log");
  const item = document.createElement("li");
  const time = document.createElement("time");
  time.textContent = new Date().toLocaleTimeString();
  const text = document.createElement("span");
  text.textContent = `${level} ${message}`;
  item.append(time, text);
  list.prepend(item);
  while (list.childElementCount > 60) list.lastElementChild?.remove();
}

/** Announces async status once, after the burst of updates settles. */
let announceTimer: number | undefined;
function announce(message: string): void {
  const region = el<HTMLParagraphElement>("status");
  region.textContent = "";
  window.clearTimeout(announceTimer);
  announceTimer = window.setTimeout(() => {
    region.textContent = message;
  }, 400);
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T | undefined> {
  if (!inTauri) {
    log("info", `${cmd} skipped: not running inside Tauri`);
    return undefined;
  }
  try {
    return await invoke<T>(cmd, args);
  } catch (error) {
    log("error", `${cmd}: ${String(error)}`);
    return undefined;
  }
}

function row(dl: HTMLElement, key: string, value: string): void {
  const dt = document.createElement("dt");
  dt.textContent = key;
  const dd = document.createElement("dd");
  dd.textContent = value;
  dl.append(dt, dd);
}

let settings: Settings;
let uaOptions: UaOption[] = [];
let uaSnapshot = "unknown";

function renderUserAgent(): void {
  const preset = el<HTMLSelectElement>("ua-preset");
  preset.replaceChildren(
    ...uaOptions.map((option) => {
      const node = document.createElement("option");
      node.value = option.id;
      node.textContent = option.label;
      node.title = option.note;
      return node;
    }),
  );
  preset.value = settings.userAgentPreset;

  const customField = el<HTMLDivElement>("ua-custom-field");
  customField.hidden = settings.userAgentPreset !== "custom";
  const customInput = el<HTMLInputElement>("ua-custom");
  customInput.value = settings.customUserAgent;

  const selected = uaOptions.find((o) => o.id === settings.userAgentPreset);
  el<HTMLParagraphElement>("ua-hint").textContent = selected?.note ?? "";

  const effective =
    settings.userAgentPreset === "custom"
      ? settings.customUserAgent.trim()
      : (uaOptions.find((o) => o.id === settings.userAgentPreset)?.value ?? "");
  el<HTMLElement>("ua-effective").textContent = effective || "(engine default)";
  el<HTMLSpanElement>("ua-snapshot").textContent = `UA snapshot ${uaSnapshot}`;
}

/** Unread counters reported by the account webviews, keyed by webview label (`wa-<id>`). */
const unread = new Map<string, number>();

/** Account shown in the content area; `null` while the settings panel is open. */
let active: string | null = null;
/** Multi-account view: several accounts tiled in a grid instead of the single `active` one. */
let grid = false;
/** Last account shown, which is what "Open chat" targets from the settings panel. */
let selected = "";

function button(text: string, onClick: () => void, className = ""): HTMLButtonElement {
  const node = document.createElement("button");
  node.type = "button";
  node.textContent = text;
  node.className = className;
  node.addEventListener("click", onClick);
  return node;
}

/** In-page replacement for `window.confirm`, which WKWebView under Tauri answers with `false`. */
function confirmAction(title: string, message: string, action: string): Promise<boolean> {
  const dialog = el<HTMLDialogElement>("confirm-dialog");
  el<HTMLElement>("confirm-title").textContent = title;
  el<HTMLElement>("confirm-message").textContent = message;
  el<HTMLButtonElement>("confirm-ok").textContent = action;
  // Escape leaves the previous value in place, which would turn a cancel into a confirm.
  dialog.returnValue = "";
  dialog.showModal();
  return new Promise((resolve) => {
    dialog.addEventListener("close", () => resolve(dialog.returnValue === "ok"), { once: true });
  });
}

async function show(id: string | null): Promise<void> {
  await call("switch_account", { id });
  grid = false;
  active = id;
  if (id) selected = id;
  renderAccounts();
}

/** Applies an account command's result; those commands return the updated settings. */
function applyAccounts(saved: Settings | undefined): void {
  if (!saved) return;
  settings = saved;
  if (!settings.accounts.some((account) => account.id === selected)) {
    selected = settings.accounts[0]?.id ?? "";
    // The backend falls back to the first account when the visible one is removed.
    if (active !== null) active = selected;
  }
  renderAccounts();
}

function renderAccounts(): void {
  el<HTMLButtonElement>("open-settings").setAttribute("aria-current", String(!grid && active === null));
  el<HTMLButtonElement>("open-grid").setAttribute("aria-current", String(grid));
  document.body.classList.toggle("grid", grid);
  const cells = settings.gridCols * settings.gridRows;
  el<HTMLElement>("grid-note").textContent =
    settings.accounts.length > cells
      ? `Showing ${cells} of ${settings.accounts.length} accounts, in dock order`
      : "";

  el<HTMLUListElement>("dock-accounts").replaceChildren(
    ...settings.accounts.map((account) => {
      const item = document.createElement("li");
      // One word: its first two letters. Several words: the first letter of the first two.
      const words = account.name.trim().split(/\s+/);
      const initials = (words.length > 1 ? words[0][0] + words[1][0] : words[0].slice(0, 2)).toUpperCase();
      const avatar = button(initials, () => void show(account.id), "dock-btn");
      avatar.title = account.name;
      avatar.setAttribute("aria-current", String(!grid && active === account.id));
      const count = unread.get(`wa-${account.id}`) ?? 0;
      if (count > 0) {
        const badge = document.createElement("span");
        badge.className = "badge";
        badge.textContent = count > 99 ? "99+" : String(count);
        // The badge is a number with no context on its own; fold it into the button's name.
        badge.setAttribute("aria-hidden", "true");
        avatar.setAttribute("aria-label", `${account.name}, ${count} unread`);
        avatar.append(badge);
      }
      item.append(avatar);
      return item;
    }),
  );

  el<HTMLUListElement>("accounts").replaceChildren(
    ...settings.accounts.map((account) => {
      const item = document.createElement("li");

      const name = document.createElement("input");
      name.type = "text";
      name.value = account.name;
      name.maxLength = 32;
      name.setAttribute("aria-label", "Account name");
      name.addEventListener("change", () => {
        void call<Settings>("rename_account", { id: account.id, name: name.value }).then(applyAccounts);
      });

      const remove = button(
        "Remove",
        () => {
          void (async () => {
            const ok = await confirmAction(
              `Remove "${account.name}"?`,
              "Its login and cached data are deleted from this device.",
              "Remove",
            );
            if (!ok) return;
            const saved = await call<Settings>("remove_account", { id: account.id });
            if (!saved) return;
            unread.delete(`wa-${account.id}`);
            applyAccounts(saved);
            log("info", `removed ${account.name}`);
          })();
        },
        "danger",
      );
      remove.disabled = settings.accounts.length <= 1;

      // Both commands bring the account on screen, so their effect is visible.
      const shown = (message: string) => {
        grid = false;
        active = selected = account.id;
        renderAccounts();
        log("info", message);
      };

      item.append(
        name,
        button("Reload", () => {
          void call<null>("reload_session", { id: account.id }).then((done) => {
            if (done !== undefined) shown(`reloaded ${account.name}`);
          });
        }),
        button(
          "Clear data…",
          () => {
            void (async () => {
              const ok = await confirmAction(
                `Clear data of "${account.name}"?`,
                "Cookies, cache and local storage are wiped. You will have to link the device again.",
                "Clear data",
              );
              if (!ok) return;
              const done = await call<null>("clear_session_data", { id: account.id });
              if (done === undefined) return;
              unread.delete(`wa-${account.id}`);
              shown(`cleared data of ${account.name}`);
            })();
          },
          "danger",
        ),
        remove,
      );
      return item;
    }),
  );
}

async function persist(): Promise<void> {
  const saved = await call<Settings>("set_settings", { settings });
  if (saved) settings = saved;
  renderUserAgent();
}

async function subscribeSessionEvents(): Promise<void> {
  if (!inTauri) return;

  await listen<{ count: number; label: string }>("session://unread", (event) => {
    const { count, label } = event.payload;
    if (unread.get(label) === count) return;
    unread.set(label, count);
    renderAccounts();

    const account = settings.accounts.find((candidate) => `wa-${candidate.id}` === label);
    announce(count === 0 ? `${account?.name ?? "Account"}: no unread messages` : `${count} unread in ${account?.name ?? "an account"}`);
  });

  // Pushed by each account's bootstrap script once the page reports its own UA.
  await listen<{ label: string; userAgent: string }>("session://user-agent", (event) => {
    const { userAgent } = event.payload;
    el<HTMLElement>("ua-actual").textContent = userAgent || "(empty)";
    const expected =
      settings.userAgentPreset === "custom"
        ? settings.customUserAgent.trim()
        : (uaOptions.find((o) => o.id === settings.userAgentPreset)?.value ?? "");

    if (expected && userAgent !== expected) {
      log("error", `webview reports a different User-Agent than the ${settings.userAgentPreset} preset`);
    }
  });

  // Sent by the backend once the dialog overlay has created an account and switched to it.
  await listen<Settings>("accounts://added", (event) => {
    grid = false;
    active = selected = event.payload.accounts[event.payload.accounts.length - 1].id;
    applyAccounts(event.payload);
    log("info", "account added");
  });

  await listen("session://unsupported", () => {
    log("error", "WhatsApp Web reports an unsupported browser — change the User-Agent preset");
  });
}

/** Paints the dock button and the settings select to match the current choice. */
function renderTheme(): void {
  const choice = settings.theme;

  const button = el<HTMLButtonElement>("cycle-theme");
  button.setAttribute("aria-label", `Theme: ${themeLabel(choice)}`);
  button.title = `Theme: ${themeLabel(choice)}`;
  // One glyph per mode; the system glyph stays for `system`.
  const icons: Array<[string, boolean]> = [
    ["icon-system", choice === "system"],
    ["icon-light", choice === "light"],
    ["icon-dark", choice === "dark"],
  ];
  for (const [className, visible] of icons) {
    button.querySelector<SVGElement>(`.${className}`)?.toggleAttribute("hidden", !visible);
  }

  const select = el<HTMLSelectElement>("set-theme");
  select.value = choice;
}

function bindCheckbox(id: string, key: "nativeNotifications" | "badgeUnreadCount" | "autostart"): void {
  const input = el<HTMLInputElement>(id);
  input.checked = settings[key];
  input.addEventListener("change", () => {
    settings[key] = input.checked;
    void persist().then(() => log("info", `${key} = ${input.checked}`));
  });
}

function bindControls(): void {
  el<HTMLSelectElement>("ua-preset").addEventListener("change", (event) => {
    settings.userAgentPreset = (event.target as HTMLSelectElement).value;
    renderUserAgent();
    void persist().then(() => log("info", `user agent preset = ${settings.userAgentPreset}`));
  });

  el<HTMLInputElement>("ua-custom").addEventListener("change", (event) => {
    settings.customUserAgent = (event.target as HTMLInputElement).value.trim();
    renderUserAgent();
    void persist();
  });

  el<HTMLSelectElement>("set-theme").addEventListener("change", (event) => {
    const value = (event.target as HTMLSelectElement).value;
    if (!isThemeChoice(value)) return;
    settings.theme = value;
    applyTheme(value);
    renderTheme();
    void persist().then(() => log("info", `theme = ${value}`));
  });

  el<HTMLButtonElement>("cycle-theme").addEventListener("click", () => {
    settings.theme = nextThemeChoice(settings.theme);
    applyTheme(settings.theme);
    renderTheme();
    void persist().then(() => log("info", `theme = ${settings.theme}`));
  });

  bindCheckbox("set-notifications", "nativeNotifications");
  bindCheckbox("set-badge", "badgeUnreadCount");
  bindCheckbox("set-autostart", "autostart");

  el<HTMLButtonElement>("open-settings").addEventListener("click", () => void show(null));

  el<HTMLButtonElement>("open-grid").addEventListener("click", () => {
    void call("show_grid").then(() => {
      grid = true;
      renderAccounts();
      const cells = settings.gridCols * settings.gridRows;
      announce(`Multi-account view, showing ${Math.min(cells, settings.accounts.length)} of ${settings.accounts.length} accounts`);
    });
  });

  for (const [id, key] of [["grid-cols", "gridCols"], ["grid-rows", "gridRows"]] as const) {
    const input = el<HTMLInputElement>(id);
    input.value = String(settings[key]);
    input.addEventListener("change", () => {
      settings[key] = Math.min(4, Math.max(1, Math.round(Number(input.value)) || 1));
      input.value = String(settings[key]);
      // The backend re-tiles the grid as part of saving.
      void persist().then(renderAccounts);
    });
  }

  el<HTMLButtonElement>("add-account").addEventListener("click", () => void call("open_add_dialog"));

  el<HTMLFormElement>("open-chat-form").addEventListener("submit", (event) => {
    event.preventDefault();
    const field = el<HTMLInputElement>("chat-number");
    const error = el<HTMLParagraphElement>("chat-number-error");
    const jid = field.value.trim();

    // Digits only: WhatsApp keys a chat by phone number, so anything else cannot resolve.
    if (!/^\d{6,15}$/.test(jid)) {
      error.textContent = "Enter the phone number in international format, digits only.";
      error.hidden = false;
      field.setAttribute("aria-invalid", "true");
      field.focus();
      return;
    }
    error.hidden = true;
    field.removeAttribute("aria-invalid");

    if (!selected) {
      error.textContent = "Add an account first.";
      error.hidden = false;
      return;
    }

    void call("open_chat", { id: selected, jid }).then(() => {
      grid = false;
      active = selected;
      renderAccounts();
    });
  });

  el<HTMLButtonElement>("test-notification").addEventListener("click", () => {
    void call("test_notification");
  });
}

async function renderRuntime(): Promise<void> {
  const info = await call<RuntimeInfo>("runtime_info");
  if (!info) return;

  const dl = el<HTMLElement>("runtime-info");
  dl.replaceChildren();
  row(dl, "Platform", info.platform);
  row(dl, "Engine", info.webviewVersion || "(not reported)");
  row(dl, "Data dir", info.sessionDir);
  row(dl, "App", info.appVersion);

  const supported = Object.entries(info.supports)
    .filter(([, on]) => on)
    .map(([name]) => name)
    .join(", ");
  row(dl, "Supported", supported || "none");

  el<HTMLElement>("session-path").textContent = `App data is stored in ${info.sessionDir}`;
  el<HTMLInputElement>("set-autostart").disabled = !info.supports.autostart;
  el<HTMLInputElement>("set-badge").disabled = !info.supports.badge;
  el<HTMLButtonElement>("test-notification").disabled = !info.supports.nativeNotifications;
}

async function main(): Promise<void> {
  const [loaded, options] = await Promise.all([
    call<Settings>("get_settings"),
    call<UaOptions>("ua_options"),
  ]);

  if (options) {
    uaOptions = options.presets;
    uaSnapshot = options.snapshot;
  }
  settings = loaded ?? {
    userAgentPreset: "engine-default",
    customUserAgent: "",
    nativeNotifications: true,
    badgeUnreadCount: true,
    autostart: false,
    zoom: 1,
    accounts: [{ id: "default", name: "Account 1" }],
    gridCols: 2,
    gridRows: 1,
    theme: "system",
  };

  // The backend shows the first account on startup.
  active = selected = settings.accounts[0]?.id ?? "";

  applyTheme(settings.theme);
  renderTheme();
  // Re-resolve only while the choice is `system`; an explicit choice ignores the OS.
  watchSystemTheme(
    () => settings.theme,
    () => renderTheme(),
  );

  renderUserAgent();
  renderAccounts();
  bindControls();
  await subscribeSessionEvents();
  await renderRuntime();
  log("info", inTauri ? "ready" : "browser preview (Tauri APIs disabled)");
}

/** The overlay webview: nothing but the "Add account" dialog, closed by tearing the webview down. */
async function runAddDialog(): Promise<void> {
  // The overlay is a separate webview that loads only the dialog, so it never runs `main()`. It
  // still has to paint the right theme: a light dialog over a dark window would be jarring, and the
  // bootstrap below sets `data-theme` on this document, not on the main one.
  await applyStoredTheme();

  const dialog = el<HTMLDialogElement>("add-account-dialog");
  const name = el<HTMLInputElement>("add-account-name");
  const error = el<HTMLParagraphElement>("add-account-error");

  dialog.addEventListener("close", () => {
    void (async () => {
      if (dialog.returnValue === "add") await call("add_account", { name: name.value });
      await call("close_add_dialog");
    })();
  });

  // Validate inline instead of leaving it to a native bubble: the same message pattern as the
  // chat field, and the dialog stays open with the caret back in the field.
  const submit = dialog.querySelector<HTMLButtonElement>('button[value="add"]');
  submit?.addEventListener("click", (event) => {
    if (name.value.trim().length > 0) return;
    event.preventDefault();
    error.textContent = "Enter a name for the account.";
    error.hidden = false;
    name.setAttribute("aria-invalid", "true");
    name.focus();
  });
  name.addEventListener("input", () => {
    if (name.value.trim().length === 0) return;
    error.hidden = true;
    name.removeAttribute("aria-invalid");
  });

  dialog.showModal();

  // `autofocus` on the field is what the dialog focusing steps look for, but focus assigned while
  // the document is still loading is discarded by the focus fixup rule — the dialog itself ended up
  // focused instead. Re-assert after load so the caret reliably lands in the field.
  const focusName = () => name.focus();
  focusName();
  if (document.readyState !== "complete") {
    window.addEventListener("load", focusName, { once: true });
  }
}

/** Reads the stored choice and paints this document before anything is shown. */
async function applyStoredTheme(): Promise<void> {
  const stored = await call<Settings>("get_settings");
  if (!stored) return;
  settings = stored;
  applyTheme(stored.theme);
}

if (location.hash === "#add-account-dialog") void runAddDialog();
else void main();
