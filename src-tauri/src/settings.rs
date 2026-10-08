//! Persistent launcher settings.

use serde::{Deserialize, Serialize};

use crate::ua;

const FILE_NAME: &str = "settings.json";

/// Theme used when nothing has been chosen: follow the OS.
fn default_theme() -> String {
    "system".to_string()
}

/// Id of the first account. It keeps the webview's default data store, so a login made before
/// multi-account support existed survives the upgrade.
pub const DEFAULT_ACCOUNT_ID: &str = "default";
/// Webview label prefix for account webviews; the account id is appended.
pub const ACCOUNT_LABEL_PREFIX: &str = "wa-";

/// The web app an account loads.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Service {
    #[default]
    Whatsapp,
    Telegram,
    Slack,
}

impl Service {
    #[cfg(test)]
    pub const ALL: [Service; 3] = [Service::Whatsapp, Service::Telegram, Service::Slack];

    /// Page the account's webview starts on, and returns to after its data is cleared.
    pub fn url(self) -> &'static str {
        match self {
            Service::Whatsapp => "https://web.whatsapp.com/",
            Service::Telegram => "https://web.telegram.org/a/",
            Service::Slack => "https://app.slack.com/client",
        }
    }

    /// Registrable domain the service runs on; its subdomains count as the service too.
    pub fn domain(self) -> &'static str {
        match self {
            Service::Whatsapp => "whatsapp.com",
            Service::Telegram => "telegram.org",
            Service::Slack => "slack.com",
        }
    }

    /// Whether `url` is a page of this service, as opposed to wherever its webview navigated to.
    pub fn owns(self, url: &tauri::Url) -> bool {
        url.scheme() == "https"
            && url.host_str().is_some_and(|host| {
                host == self.domain()
                    || host
                        .strip_suffix(self.domain())
                        .is_some_and(|subdomain| subdomain.ends_with('.'))
            })
    }

    pub fn name(self) -> &'static str {
        match self {
            Service::Whatsapp => "WhatsApp",
            Service::Telegram => "Telegram",
            Service::Slack => "Slack",
        }
    }
}

/// One login to a [`Service`], backed by its own webview and its own data store.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    /// `default`, or the creation time in nanoseconds as lowercase hex.
    pub id: String,
    pub name: String,
    /// Defaulted so accounts saved before other services existed stay WhatsApp.
    #[serde(default)]
    pub service: Service,
}

impl Account {
    pub fn new(name: String, service: Service) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or_default();
        Self {
            id: format!("{nanos:x}"),
            name,
            service,
        }
    }

    pub fn label(&self) -> String {
        format!("{ACCOUNT_LABEL_PREFIX}{}", self.id)
    }

    /// Identifier of the isolated data store; `None` for the default account and for ids that are
    /// not plain hex (which would also not be valid webview labels or directory names).
    pub fn store_id(&self) -> Option<[u8; 16]> {
        if self.id.len() > 32 || !self.id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        u128::from_str_radix(&self.id, 16).ok().map(u128::to_le_bytes)
    }
}

/// Settings surfaced in the launcher UI.
///
/// Parsing is strict: a file missing any field is treated as corrupt and replaced by the platform
/// defaults in [`load`]. That is deliberate — the file is written whole by [`save`], so a partial
/// file means it was truncated or hand-edited, and a half-applied configuration is worse than none.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// One of [`ua::options`] ids.
    pub user_agent_preset: String,
    /// Raw UA string used when `user_agent_preset` is `custom`.
    pub custom_user_agent: String,
    /// Show native notifications for messages that arrive while the window is not focused.
    pub native_notifications: bool,
    /// Mirror the unread counter onto the dock/taskbar badge.
    pub badge_unread_count: bool,
    /// Launch on login.
    pub autostart: bool,
    /// Webview zoom factor.
    pub zoom: f64,
    /// `system`, `light` or `dark`. `system` follows the OS preference.
    #[serde(default = "default_theme")]
    pub theme: String,
    /// Linked accounts, in dock order. Defaulted so a pre-multi-account file still parses.
    #[serde(default)]
    pub accounts: Vec<Account>,
}

/// Largest grid side offered; WhatsApp Web is unusable in cells smaller than that.
pub const MAX_GRID: u32 = 4;

impl Settings {
    pub fn for_platform(platform: &str) -> Self {
        Self {
            user_agent_preset: ua::default_preset(platform).to_string(),
            custom_user_agent: String::new(),
            native_notifications: true,
            badge_unread_count: true,
            autostart: false,
            zoom: 1.0,
            theme: default_theme(),
            accounts: vec![Account {
                id: DEFAULT_ACCOUNT_ID.to_string(),
                name: "Account 1".to_string(),
                service: Service::default(),
            }],
        }
    }

    /// Clamps values that would leave the app in an unusable state, so a hand-edited or corrupted
    /// file cannot break startup.
    pub fn normalize(mut self, platform: &str) -> Self {
        if !ua::is_known_preset(&self.user_agent_preset) {
            self.user_agent_preset = ua::default_preset(platform).to_string();
        }
        if !matches!(self.theme.as_str(), "system" | "light" | "dark") {
            self.theme = default_theme();
        }
        if !self.zoom.is_finite() {
            self.zoom = 1.0;
        }
        self.zoom = self.zoom.clamp(0.5, 2.0);
        self.custom_user_agent = self.custom_user_agent.trim().to_string();

        // Ids become webview labels and directory names, so anything unexpected is dropped.
        let mut seen = std::collections::HashSet::new();
        self.accounts.retain(|account| {
            (account.id == DEFAULT_ACCOUNT_ID || account.store_id().is_some())
                && seen.insert(account.id.clone())
        });
        if self.accounts.is_empty() {
            self.accounts = Self::for_platform(platform).accounts;
        }
        for account in &mut self.accounts {
            account.name = account.name.trim().chars().take(32).collect();
            if account.name.is_empty() {
                account.name = "Account".to_string();
            }
        }
        self
    }

    /// The UA to install on the session webview, or `None` to keep the engine default.
    pub fn user_agent(&self) -> Option<String> {
        ua::resolve(&self.user_agent_preset, &self.custom_user_agent)
    }
}

fn path(app: &tauri::AppHandle) -> tauri::Result<std::path::PathBuf> {
    Ok(tauri::Manager::path(app).app_config_dir()?.join(FILE_NAME))
}

/// Reads the settings file, falling back to platform defaults.
///
/// A corrupt file is not an error worth failing startup for: it is replaced on the next write.
pub fn load(app: &tauri::AppHandle, platform: &str) -> Settings {
    let defaults = Settings::for_platform(platform);
    let Ok(path) = path(app) else {
        return defaults;
    };
    let Ok(raw) = std::fs::read_to_string(path) else {
        return defaults;
    };
    serde_json::from_str::<Settings>(&raw)
        .map(|settings| settings.normalize(platform))
        .unwrap_or(defaults)
}

pub fn save(app: &tauri::AppHandle, settings: &Settings) -> tauri::Result<()> {
    let path = path(app)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let raw = serde_json::to_string_pretty(settings)?;
    std::fs::write(path, raw)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_json() {
        let settings = Settings::for_platform("linux");
        let raw = serde_json::to_string(&settings).expect("serializes");
        let parsed: Settings = serde_json::from_str(&raw).expect("deserializes");
        assert_eq!(parsed.user_agent_preset, settings.user_agent_preset);
        assert_eq!(parsed.native_notifications, settings.native_notifications);
        assert_eq!(parsed.zoom, settings.zoom);
    }

    #[test]
    fn partial_files_are_rejected_so_load_can_fall_back_to_defaults() {
        assert!(serde_json::from_str::<Settings>("{}").is_err());
        assert!(serde_json::from_str::<Settings>(r#"{"zoom":1.0}"#).is_err());
    }

    #[test]
    fn normalize_replaces_an_unknown_preset_with_the_platform_default() {
        let mut settings = Settings::for_platform("linux");
        settings.user_agent_preset = "internet-explorer-6".into();
        let normalized = settings.normalize("macos");
        assert_eq!(normalized.user_agent_preset, "safari-macos");
    }

    #[test]
    fn normalize_clamps_zoom_and_rejects_non_finite_values() {
        let mut settings = Settings::for_platform("linux");
        settings.zoom = 99.0;
        assert_eq!(settings.clone().normalize("linux").zoom, 2.0);

        settings.zoom = 0.1;
        assert_eq!(settings.clone().normalize("linux").zoom, 0.5);

        settings.zoom = f64::NAN;
        assert_eq!(settings.normalize("linux").zoom, 1.0);
    }

    #[test]
    fn theme_defaults_to_following_the_system() {
        assert_eq!(Settings::for_platform("linux").theme, "system");
    }

    /// A settings file written before the theme existed must still load, with the theme defaulted.
    /// Parsing is otherwise strict, so a missing non-defaulted field would discard every setting.
    #[test]
    fn a_file_without_a_theme_still_loads_and_defaults_to_system() {
        let legacy = r#"{
            "userAgentPreset": "safari-macos",
            "customUserAgent": "",
            "nativeNotifications": true,
            "badgeUnreadCount": false,
            "autostart": false,
            "zoom": 1.25,
            "accounts": [{ "id": "default", "name": "AS" }],
            "gridCols": 2,
            "gridRows": 1
        }"#;

        let parsed: Settings = serde_json::from_str(legacy).expect("legacy file parses");
        assert_eq!(parsed.theme, "system");
        // The rest of the file is not thrown away.
        assert_eq!(parsed.user_agent_preset, "safari-macos");
        assert_eq!(parsed.zoom, 1.25);
        assert_eq!(parsed.accounts.len(), 1);
        assert!(!parsed.badge_unread_count);
    }

    #[test]
    fn normalize_rejects_an_unknown_theme() {
        let mut settings = Settings::for_platform("linux");
        settings.theme = "solarized".into();
        assert_eq!(settings.normalize("linux").theme, "system");

        for valid in ["system", "light", "dark"] {
            let mut settings = Settings::for_platform("linux");
            settings.theme = valid.into();
            assert_eq!(settings.normalize("linux").theme, valid);
        }
    }

    #[test]
    fn normalize_trims_the_custom_user_agent() {
        let mut settings = Settings::for_platform("linux");
        settings.custom_user_agent = "  Mozilla/5.0  ".into();
        assert_eq!(settings.normalize("linux").custom_user_agent, "Mozilla/5.0");
    }

    #[test]
    fn accounts_survive_old_files_and_reject_unsafe_ids() {
        let old = r#"{"userAgentPreset":"chrome-linux","customUserAgent":"","nativeNotifications":true,
            "badgeUnreadCount":true,"autostart":false,"zoom":1.0,"openChatsInNewWindow":false}"#;
        let mut settings = serde_json::from_str::<Settings>(old)
            .expect("pre-multi-account file parses")
            .normalize("linux");
        assert_eq!(settings.accounts.len(), 1);
        assert_eq!(settings.accounts[0].id, DEFAULT_ACCOUNT_ID);
        assert_eq!(settings.accounts[0].store_id(), None);
        assert_eq!(settings.accounts[0].service, Service::Whatsapp);

        let added = Account::new("  Work  ".into(), Service::Telegram);
        assert!(added.store_id().is_some());
        settings.accounts.push(added.clone());
        settings.accounts.push(added.clone());
        settings.accounts.push(Account {
            id: "../../etc".into(),
            name: "evil".into(),
            service: Service::Slack,
        });
        let settings = settings.normalize("linux");
        assert_eq!(settings.accounts.len(), 2, "duplicate and unsafe ids are dropped");
        assert_eq!(settings.accounts[1].name, "Work");
        assert_eq!(settings.accounts[1].label(), format!("wa-{}", added.id));

        // The service survives a save, and a pre-service account entry reads back as WhatsApp.
        let raw = serde_json::to_string(&settings).expect("serializes");
        assert!(raw.contains(r#""service":"telegram""#));
        let reloaded: Settings = serde_json::from_str(&raw).expect("deserializes");
        assert_eq!(reloaded.accounts[1].service, Service::Telegram);
        let legacy: Account = serde_json::from_str(r#"{"id":"default","name":"AS"}"#).expect("parses");
        assert_eq!(legacy.service, Service::Whatsapp);
    }

    #[test]
    fn a_service_owns_only_https_pages_of_its_own_domain() {
        let owns = |url: &str| Service::Whatsapp.owns(&tauri::Url::parse(url).expect("valid url"));
        assert!(owns("https://web.whatsapp.com/send?phone=1"));
        assert!(owns("https://whatsapp.com/"));
        assert!(!owns("http://web.whatsapp.com/"), "cleartext is not the service");
        assert!(!owns("https://evilwhatsapp.com/"), "a suffix match is not a subdomain");
        assert!(!owns("https://web.whatsapp.com.evil.example/"));
        assert!(!owns("https://web.telegram.org/a/"));
        assert!(!owns("about:blank"));
        for service in Service::ALL {
            assert!(service.owns(&tauri::Url::parse(service.url()).expect("valid url")));
        }
    }

    #[test]
    fn user_agent_follows_the_selected_preset() {
        let mut settings = Settings::for_platform("linux");
        assert!(settings.user_agent().is_some_and(|ua| ua.contains("Linux")));

        settings.user_agent_preset = ua::PRESET_ENGINE_DEFAULT.into();
        assert_eq!(settings.user_agent(), None);

        settings.user_agent_preset = ua::PRESET_CUSTOM.into();
        settings.custom_user_agent = "Mozilla/5.0 (Custom)".into();
        assert_eq!(settings.user_agent().as_deref(), Some("Mozilla/5.0 (Custom)"));
    }
}
