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

/** Mirrors `Service` in src-tauri/src/settings.rs. */
const SERVICES = { whatsapp: "WhatsApp", telegram: "Telegram", slack: "Slack" } as const;
type Service = keyof typeof SERVICES;

/** Brand marks from Simple Icons (CC0), one path each on a 24x24 viewBox. */
const SERVICE_ICONS: Record<Service, string> = {
  whatsapp:
    "M17.472 14.382c-.297-.149-1.758-.867-2.03-.967-.273-.099-.471-.148-.67.15-.197.297-.767.966-.94 1.164-.173.199-.347.223-.644.075-.297-.15-1.255-.463-2.39-1.475-.883-.788-1.48-1.761-1.653-2.059-.173-.297-.018-.458.13-.606.134-.133.298-.347.446-.52.149-.174.198-.298.298-.497.099-.198.05-.371-.025-.52-.075-.149-.669-1.612-.916-2.207-.242-.579-.487-.5-.669-.51-.173-.008-.371-.01-.57-.01-.198 0-.52.074-.792.372-.272.297-1.04 1.016-1.04 2.479 0 1.462 1.065 2.875 1.213 3.074.149.198 2.096 3.2 5.077 4.487.709.306 1.262.489 1.694.625.712.227 1.36.195 1.871.118.571-.085 1.758-.719 2.006-1.413.248-.694.248-1.289.173-1.413-.074-.124-.272-.198-.57-.347m-5.421 7.403h-.004a9.87 9.87 0 01-5.031-1.378l-.361-.214-3.741.982.998-3.648-.235-.374a9.86 9.86 0 01-1.51-5.26c.001-5.45 4.436-9.884 9.888-9.884 2.64 0 5.122 1.03 6.988 2.898a9.825 9.825 0 012.893 6.994c-.003 5.45-4.437 9.884-9.885 9.884m8.413-18.297A11.815 11.815 0 0012.05 0C5.495 0 .16 5.335.157 11.892c0 2.096.547 4.142 1.588 5.945L.057 24l6.305-1.654a11.882 11.882 0 005.683 1.448h.005c6.554 0 11.89-5.335 11.893-11.893a11.821 11.821 0 00-3.48-8.413Z",
  telegram:
    "M11.944 0A12 12 0 0 0 0 12a12 12 0 0 0 12 12 12 12 0 0 0 12-12A12 12 0 0 0 12 0a12 12 0 0 0-.056 0zm4.962 7.224c.1-.002.321.023.465.14a.506.506 0 0 1 .171.325c.016.093.036.306.02.472-.18 1.898-.962 6.502-1.36 8.627-.168.9-.499 1.201-.82 1.23-.696.065-1.225-.46-1.9-.902-1.056-.693-1.653-1.124-2.678-1.8-1.185-.78-.417-1.21.258-1.91.177-.184 3.247-2.977 3.307-3.23.007-.032.014-.15-.056-.212s-.174-.041-.249-.024c-.106.024-1.793 1.14-5.061 3.345-.48.33-.913.49-1.302.48-.428-.008-1.252-.241-1.865-.44-.752-.245-1.349-.374-1.297-.789.027-.216.325-.437.893-.663 3.498-1.524 5.83-2.529 6.998-3.014 3.332-1.386 4.025-1.627 4.476-1.635z",
  slack:
    "M5.042 15.165a2.528 2.528 0 0 1-2.52 2.523A2.528 2.528 0 0 1 0 15.165a2.527 2.527 0 0 1 2.522-2.52h2.52v2.52zM6.313 15.165a2.527 2.527 0 0 1 2.521-2.52 2.527 2.527 0 0 1 2.521 2.52v6.313A2.528 2.528 0 0 1 8.834 24a2.528 2.528 0 0 1-2.521-2.522v-6.313zM8.834 5.042a2.528 2.528 0 0 1-2.521-2.52A2.528 2.528 0 0 1 8.834 0a2.528 2.528 0 0 1 2.521 2.522v2.52H8.834zM8.834 6.313a2.528 2.528 0 0 1 2.521 2.521 2.528 2.528 0 0 1-2.521 2.521H2.522A2.528 2.528 0 0 1 0 8.834a2.528 2.528 0 0 1 2.522-2.521h6.312zM18.956 8.834a2.528 2.528 0 0 1 2.522-2.521A2.528 2.528 0 0 1 24 8.834a2.528 2.528 0 0 1-2.522 2.521h-2.522V8.834zM17.688 8.834a2.528 2.528 0 0 1-2.523 2.521 2.527 2.527 0 0 1-2.52-2.521V2.522A2.527 2.527 0 0 1 15.165 0a2.528 2.528 0 0 1 2.523 2.522v6.312zM15.165 18.956a2.528 2.528 0 0 1 2.523 2.522A2.528 2.528 0 0 1 15.165 24a2.527 2.527 0 0 1-2.52-2.522v-2.522h2.52zM15.165 17.688a2.527 2.527 0 0 1-2.52-2.523 2.526 2.526 0 0 1 2.52-2.52h6.313A2.527 2.527 0 0 1 24 15.165a2.528 2.528 0 0 1-2.522 2.523h-6.313z",
};

function serviceIcon(service: Service): SVGSVGElement {
  const ns = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(ns, "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("aria-hidden", "true");
  svg.classList.add("brand");
  const path = document.createElementNS(ns, "path");
  path.setAttribute("d", SERVICE_ICONS[service]);
  svg.append(path);
  return svg;
}

interface Account {
  id: string;
  name: string;
  service: Service;
}

interface Settings {
  userAgentPreset: string;
  customUserAgent: string;
  nativeNotifications: boolean;
  badgeUnreadCount: boolean;
  autostart: boolean;
  zoom: number;
  accounts: Account[];
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

/* Chrome measures mirrored from src-tauri/src/lib.rs: the dock reserves the left edge, the grid
   bar sits above the grid, and grid cells may not shrink past the smallest usable pane. */
const DOCK_WIDTH = 64;
const GRID_BAR_HEIGHT = 44;
const GRID_MIN_CELL = { width: 360, height: 240 };

/** Mirrors `grid_shape` in src-tauri/src/lib.rs: near-square tiling capped at 4 per side, and
   capped by the window size so no cell drops below the smallest usable pane — a small window
   shows fewer accounts, which is what the grid note then explains. */
function gridCells(count: number, width = window.innerWidth, height = window.innerHeight): number {
  const colCap = Math.max(1, Math.floor((width - DOCK_WIDTH) / GRID_MIN_CELL.width));
  const rowCap = Math.max(1, Math.floor((height - GRID_BAR_HEIGHT) / GRID_MIN_CELL.height));
  const cols = Math.min(4, Math.max(1, Math.ceil(Math.sqrt(count))), colCap);
  const rows = Math.min(4, Math.ceil(count / cols), rowCap);
  return cols * rows;
}

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
  const cells = gridCells(settings.accounts.length);
  el<HTMLElement>("grid-note").textContent =
    settings.accounts.length > cells
      ? `Showing ${cells} of ${settings.accounts.length} accounts, in dock order`
      : "";

  el<HTMLUListElement>("dock-accounts").replaceChildren(
    ...settings.accounts.map((account) => {
      const item = document.createElement("li");
      // The service's mark tells the accounts apart at a glance; the name sits underneath.
      const described = `${account.name} · ${SERVICES[account.service]}`;
      const avatar = button("", () => void show(account.id), "dock-btn");
      avatar.append(serviceIcon(account.service));
      avatar.title = described;
      avatar.setAttribute("aria-label", described);
      avatar.dataset.service = account.service;
      avatar.setAttribute("aria-current", String(!grid && active === account.id));
      // Visible text for the same name the button already carries, so hidden from assistive tech.
      const name = document.createElement("span");
      name.className = "dock-name";
      name.textContent = account.name;
      name.setAttribute("aria-hidden", "true");
      const count = unread.get(`wa-${account.id}`) ?? 0;
      if (count > 0) {
        const badge = document.createElement("span");
        badge.className = "badge";
        badge.textContent = count > 99 ? "99+" : String(count);
        // The badge is a number with no context on its own; fold it into the button's name.
        badge.setAttribute("aria-hidden", "true");
        avatar.setAttribute("aria-label", `${described}, ${count} unread`);
        avatar.append(badge);
      }
      item.append(avatar, name);
      return item;
    }),
  );

  el<HTMLUListElement>("accounts").replaceChildren(
    ...settings.accounts.map((account) => {
      const item = document.createElement("li");

      const service = document.createElement("span");
      service.className = "service";
      service.dataset.service = account.service;
      service.textContent = SERVICES[account.service];

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
        service,
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
  // A bare signal: the settings are read back from the backend, never taken from the event.
  await listen("accounts://added", () => {
    void call<Settings>("get_settings").then((saved) => {
      if (!saved) return;
      grid = false;
      active = selected = saved.accounts[saved.accounts.length - 1].id;
      applyAccounts(saved);
      log("info", "account added");
    });
  });

  // Emitted while the downloaded update streams in; `total` stays null when unknown.
  await listen<{ received: number; total: number | null }>("update://progress", (event) => {
    const { received, total } = event.payload;
    const status = el<HTMLElement>("update-status");
    status.textContent =
      total !== null
        ? `Downloading update… ${Math.round((received / total) * 100)}%`
        : `Downloading update… ${(received / 1048576).toFixed(1)} MB`;
  });

  // Pushed once per run when the system reports notifications are disabled for the app.
  await listen<string>("notifications://denied", (event) => {
    log("error", event.payload);
  });

  await listen("session://unsupported", () => {
    log("error", "An account's page reports an unsupported browser — change the User-Agent preset");
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

/** Result of the update check: the installed version and the newer release, when one exists. */
interface UpdateCheck {
  currentVersion: string;
  update: { version: string; notes: string } | null;
}

/** Checks GitHub Releases and, with the user's confirmation, installs what it finds. */
async function checkForUpdates(): Promise<void> {
  const button = el<HTMLButtonElement>("check-updates");
  const status = el<HTMLElement>("update-status");
  button.disabled = true;
  status.textContent = "Checking for updates…";

  const result = await call<UpdateCheck>("check_for_updates");
  if (!result) {
    status.textContent = "Could not check for updates. See the activity log.";
    button.disabled = false;
    return;
  }
  if (!result.update) {
    status.textContent = `WaDesk v${result.currentVersion} is up to date.`;
    button.disabled = false;
    return;
  }

  const notes = result.update.notes.trim().slice(0, 300);
  const ok = await confirmAction(
    `Update to v${result.update.version}?`,
    notes.length > 0
      ? notes
      : "A newer version is available. The app restarts once the update is installed.",
    "Install update",
  );
  if (!ok) {
    button.disabled = false;
    status.textContent = "Update dismissed. You can check again at any time.";
    return;
  }
  // On success the app restarts, so an unresolved command is expected here.
  status.textContent = "Downloading update…";
  const done = await call<null>("install_update");
  if (done === undefined) {
    status.textContent = "The update could not be installed. See the activity log.";
    button.disabled = false;
  }
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

  el<HTMLButtonElement>("check-updates").addEventListener("click", () => void checkForUpdates());

  el<HTMLButtonElement>("open-settings").addEventListener("click", () => void show(null));

  el<HTMLButtonElement>("open-grid").addEventListener("click", () => {
    void call("show_grid").then(() => {
      grid = true;
      renderAccounts();
      const cells = gridCells(settings.accounts.length);
      announce(`Multi-account view, showing ${Math.min(cells, settings.accounts.length)} of ${settings.accounts.length} accounts`);
    });
  });

  // The backend re-tiles the grid natively while the window resizes; keep the note in step.
  window.addEventListener("resize", () => {
    if (grid) renderAccounts();
  });

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
    accounts: [{ id: "default", name: "Account 1", service: "whatsapp" }],
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
  const service = el<HTMLSelectElement>("add-account-service");
  const error = el<HTMLParagraphElement>("add-account-error");

  dialog.addEventListener("close", () => {
    void (async () => {
      if (dialog.returnValue === "add") await call("add_account", { name: name.value, service: service.value });
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
