//! Configuration loading for the daemon (`config.toml`).

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Top-level configuration, deserialized from `config.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// How often to poll calendar backends, in seconds.
    pub poll_interval_secs: u64,
    /// How often the UI re-evaluates which events are due, in seconds.
    pub refresh_interval_secs: u64,
    /// Global fallback reminder lead time (minutes) for events without one.
    pub global_reminder_minutes: i64,
    /// Snooze presets.
    pub snooze: SnoozeConfig,
    /// Configured calendar backends.
    pub backends: Vec<BackendConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SnoozeConfig {
    /// "Minutes before start" presets (multiples of 5).
    pub before_start: Vec<i64>,
    /// "Minutes from now" presets (multiples of 5).
    pub after_now: Vec<i64>,
}

/// A single backend definition. Currently `type = "ics"` and `type = "msgraph"`
/// are implemented; other types are reserved for future providers and are
/// skipped with a warning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendConfig {
    /// Backend kind, e.g. "ics", "caldav", "google", "msgraph".
    #[serde(rename = "type")]
    pub kind: String,
    /// Stable account identifier.
    pub id: String,
    /// Local `.ics` file path (for `type = "ics"`).
    #[serde(default)]
    pub file: Option<String>,
    /// HTTP(S) `.ics` URL (for `type = "ics"`).
    #[serde(default)]
    pub url: Option<String>,
    /// Azure application (client) ID (for `type = "msgraph"`).
    #[serde(default)]
    pub client_id: Option<String>,
    /// Azure tenant (for `type = "msgraph"`): `common` (default), `organizations`,
    /// `consumers`, or a specific tenant id.
    #[serde(default)]
    pub tenant: Option<String>,
    /// OAuth client secret (for `type = "google"`; not confidential for
    /// installed/limited-input apps).
    #[serde(default)]
    pub client_secret: Option<String>,
    /// Calendar id to read (for `type = "google"`), defaults to `primary`.
    #[serde(default)]
    pub calendar_id: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            poll_interval_secs: 60,
            refresh_interval_secs: 15,
            global_reminder_minutes: 15,
            snooze: SnoozeConfig::default(),
            backends: Vec::new(),
        }
    }
}

impl Default for SnoozeConfig {
    fn default() -> Self {
        Self {
            before_start: vec![0, 5, 10, 15, 30, 60],
            after_now: vec![5, 10, 15, 30],
        }
    }
}

impl Config {
    pub fn global_reminder(&self) -> Duration {
        Duration::from_secs((self.global_reminder_minutes.max(0) as u64) * 60)
    }

    pub fn poll_interval(&self) -> Duration {
        Duration::from_secs(self.poll_interval_secs.max(5))
    }
}

/// Default config file path: `$XDG_CONFIG_HOME/calendar-notifications-modal/config.toml`.
pub fn default_config_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "calendar-notifications-modal")
        .map(|p| p.config_dir().join("config.toml"))
}

/// Load configuration from `path`. If the file does not exist, a commented
/// sample is written and the default configuration is returned.
pub fn load_or_init(path: &Path) -> anyhow::Result<Config> {
    if path.exists() {
        let text = std::fs::read_to_string(path)?;
        let cfg: Config = toml::from_str(&text)?;
        Ok(cfg)
    } else {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, SAMPLE_CONFIG)?;
        tracing::warn!(
            "no config found; wrote a sample to {}. Edit it to add your calendars.",
            path.display()
        );
        Ok(Config::default())
    }
}

const SAMPLE_CONFIG: &str = r#"# Calendar Notifications Modal configuration.

# How often to poll calendars for new/updated events (seconds).
poll_interval_secs = 60
# How often the modal re-evaluates which events are currently due (seconds).
refresh_interval_secs = 15
# Reminder lead time used when an event has no reminder of its own (minutes).
global_reminder_minutes = 15

[snooze]
# "Remind me X minutes before start" choices (multiples of 5).
before_start = [0, 5, 10, 15, 30, 60]
# "Remind me in X minutes" choices once the event has started (multiples of 5).
after_now = [5, 10, 15, 30]

# Add one [[backends]] block per calendar.
#
# ICS (local file or HTTP(S) URL):
# [[backends]]
# type = "ics"
# id = "work"
# url = "https://example.com/calendar.ics"

# [[backends]]
# type = "ics"
# id = "personal"
# file = "/home/you/.local/share/calendars/personal.ics"

# Microsoft 365 / Outlook via Microsoft Graph. Register a public-client Azure
# app, allow public client (device code) flows, grant delegated Calendars.Read,
# then run once:  calendar-notifications-modal --login outlook
# tenant defaults to "common" (personal + work/school).
# [[backends]]
# type = "msgraph"
# id = "outlook"
# client_id = "00000000-0000-0000-0000-000000000000"
# tenant = "common"

# Google Calendar (personal account; no admin needed).
#
# Setup (once):
#   1. Google Cloud Console: create a project and enable the Google Calendar API.
#   2. OAuth consent screen: External; add yourself under "Test users".
#   3. Credentials > OAuth client ID > "TVs and Limited Input devices".
#   4. Put the client id + secret below, then run:
#        calendar-notifications-modal --login gcal
#   calendar_id defaults to "primary".
#   (In Testing mode, Google refresh tokens expire after 7 days; re-run --login.)
# [[backends]]
# type = "google"
# id = "gcal"
# client_id = "xxxxxxxx.apps.googleusercontent.com"
# client_secret = "your-google-client-secret"
# calendar_id = "primary"
"#;
