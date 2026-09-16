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
    /// Path to a custom sound file (wav/mp3/ogg/flac/...) to play when a
    /// reminder pops up. If unset, falls back to the desktop's short
    /// `canberra-gtk-play --id=message` alert sound.
    #[serde(default)]
    pub sound_file: Option<String>,
    /// Default browser command to use for opening meeting links. If unset,
    /// falls back to `xdg-open`. Can include arguments, e.g.,
    /// `"firefox -P work"` or `"google-chrome --new-window"`.
    #[serde(default)]
    pub default_browser: Option<String>,
    /// Per-provider browser overrides.
    #[serde(default)]
    pub meeting_browsers: MeetingBrowsersConfig,
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MeetingBrowsersConfig {
    /// Browser command for Microsoft Teams links.
    pub teams: Option<String>,
    /// Browser command for Google Meet links.
    pub googlemeet: Option<String>,
    /// Browser command for Zoom links.
    pub zoom: Option<String>,
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
    /// Opt-in staleness check (for `type = "ics"`): if the source hasn't
    /// produced fresh data within this many seconds, the backend reports an
    /// error instead of silently keeping old data. Unset or `0` disables the
    /// check. Useful for `.ics` files a periodic exporter (e.g.
    /// `tools/owa-exporter`) is supposed to refresh; not recommended for
    /// files you only edit by hand.
    #[serde(default)]
    pub stale_after_secs: Option<u64>,
    /// Opt-in grace period (for `type = "ics"` with `stale_after_secs` set):
    /// suppress staleness warnings for this many seconds after the daemon starts,
    /// allowing the exporter (e.g. systemd timer with `OnBootSec=2min`) time to
    /// run before checks activate. When the grace period expires, an immediate
    /// export run is triggered. Unset or `0` disables the grace period.
    #[serde(default)]
    pub staleness_grace_period_secs: Option<u64>,
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
            sound_file: None,
            default_browser: None,
            meeting_browsers: MeetingBrowsersConfig::default(),
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

# Custom sound to play when a reminder pops up (wav/mp3/ogg/flac/...).
# If unset, falls back to the short desktop "message" alert sound.
# sound_file = "/home/you/Music/notification.mp3"

# Browser configuration for opening meeting links.
# If unset, falls back to xdg-open (system default browser).
# Can specify a default browser for all meetings:
# default_browser = "firefox"
# default_browser = "google-chrome --new-window"
# default_browser = "microsoft-edge"

# Or configure per-provider browsers:
# [meeting_browsers]
# teams = "microsoft-edge --app=https://teams.microsoft.com"
# googlemeet = "google-chrome --new-window"
# zoom = "firefox -P work"

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_browser_config_parsing() {
        let toml_text = r#"
poll_interval_secs = 60
refresh_interval_secs = 15
global_reminder_minutes = 15
default_browser = "firefox"

[snooze]
before_start = [0, 5, 10, 15, 30, 60]
after_now = [5, 10, 15, 30]

[meeting_browsers]
teams = "microsoft-edge --app=https://teams.microsoft.com"
googlemeet = "google-chrome --new-window"
zoom = "firefox -P work"

[[backends]]
type = "ics"
id = "test"
file = "/tmp/test.ics"
"#;

        let cfg: Config = toml::from_str(toml_text).expect("Failed to parse config");
        
        assert_eq!(cfg.default_browser.as_deref(), Some("firefox"));
        assert_eq!(cfg.meeting_browsers.teams.as_deref(), 
                   Some("microsoft-edge --app=https://teams.microsoft.com"));
        assert_eq!(cfg.meeting_browsers.googlemeet.as_deref(), 
                   Some("google-chrome --new-window"));
        assert_eq!(cfg.meeting_browsers.zoom.as_deref(), 
                   Some("firefox -P work"));
    }

    #[test]
    fn test_browser_config_defaults() {
        let toml_text = r#"
poll_interval_secs = 60
refresh_interval_secs = 15
global_reminder_minutes = 15

[[backends]]
type = "ics"
id = "test"
file = "/tmp/test.ics"
"#;

        let cfg: Config = toml::from_str(toml_text).expect("Failed to parse config");
        
        assert_eq!(cfg.default_browser, None);
        assert_eq!(cfg.meeting_browsers.teams, None);
        assert_eq!(cfg.meeting_browsers.googlemeet, None);
        assert_eq!(cfg.meeting_browsers.zoom, None);
    }
}
