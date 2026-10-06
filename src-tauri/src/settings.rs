//! Persistent launcher settings.

use serde::{Deserialize, Serialize};

use crate::ua;

const FILE_NAME: &str = "settings.json";

/// Id of the first account. It keeps the webview's default data store, so a login made before
/// multi-account support existed survives the upgrade.
pub const DEFAULT_ACCOUNT_ID: &str = "default";
/// Webview label prefix for account webviews; the account id is appended.
pub const ACCOUNT_LABEL_PREFIX: &str = "wa-";

/// One WhatsApp login, backed by its own webview and its own data store.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    /// `default`, or the creation time in nanoseconds as lowercase hex.
    pub id: String,
    pub name: String,
}

impl Account {
    pub fn new(name: String) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or_default();
        Self {
            id: format!("{nanos:x}"),
            name,
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
    /// Linked accounts, in dock order. Defaulted so a pre-multi-account file still parses.
    #[serde(default)]
    pub accounts: Vec<Account>,
}

impl Settings {
    pub fn for_platform(platform: &str) -> Self {
        Self {
            user_agent_preset: ua::default_preset(platform).to_string(),
            custom_user_agent: String::new(),
            native_notifications: true,
            badge_unread_count: true,
            autostart: false,
            zoom: 1.0,
            accounts: vec![Account {
                id: DEFAULT_ACCOUNT_ID.to_string(),
                name: "Account 1".to_string(),
            }],
        }
    }

    /// Clamps values that would leave the app in an unusable state, so a hand-edited or corrupted
    /// file cannot break startup.
    pub fn normalize(mut self, platform: &str) -> Self {
        if !ua::is_known_preset(&self.user_agent_preset) {
            self.user_agent_preset = ua::default_preset(platform).to_string();
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

        let added = Account::new("  Work  ".into());
        assert!(added.store_id().is_some());
        settings.accounts.push(added.clone());
        settings.accounts.push(added.clone());
        settings.accounts.push(Account {
            id: "../../etc".into(),
            name: "evil".into(),
        });
        let settings = settings.normalize("linux");
        assert_eq!(settings.accounts.len(), 2, "duplicate and unsafe ids are dropped");
        assert_eq!(settings.accounts[1].name, "Work");
        assert_eq!(settings.accounts[1].label(), format!("wa-{}", added.id));
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
