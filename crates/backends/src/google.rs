//! Google Calendar backend: reads a Google calendar over the Calendar API v3.
//!
//! Authentication uses the OAuth2 **device authorization grant** (Google's "TV
//! and Limited-Input Device" flow), which requires an OAuth client you create in
//! the Google Cloud console (`client_id` + `client_secret`; the secret is not
//! confidential for installed/limited-input apps). Tokens are cached on disk and
//! refreshed automatically, reusing [`crate::auth::TokenCache`].

use std::path::{Path, PathBuf};
use std::time::Duration as StdDuration;

use async_trait::async_trait;
use chrono::{DateTime, Duration, Local, NaiveDate, NaiveTime, TimeZone, Utc};
use cnm_core::backend::{BackendError, CalendarBackend};
use cnm_core::meeting::{MeetingLink, MeetingProvider};
use cnm_core::model::CalendarEvent;
use serde::Deserialize;

use crate::auth::TokenCache;

/// Read-only access to the user's calendars and events.
pub const SCOPE: &str = "https://www.googleapis.com/auth/calendar.readonly";

const DEVICE_CODE_URL: &str = "https://oauth2.googleapis.com/device/code";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const EVENTS_BASE: &str = "https://www.googleapis.com/calendar/v3/calendars";

/// Refresh the access token when within this margin of expiry.
const EXPIRY_MARGIN: Duration = Duration::seconds(60);

/// Details shown to the user to complete device sign-in.
#[derive(Debug, Clone)]
pub struct DeviceCodePrompt {
    pub verification_uri: String,
    pub user_code: String,
    pub device_code: String,
    pub interval: u64,
    pub expires_in: u64,
}

/// A configured Google OAuth client (device-code flow).
pub struct GoogleOAuth {
    client_id: String,
    client_secret: String,
    http: reqwest::Client,
}

#[derive(Deserialize)]
struct DeviceCodeRaw {
    device_code: String,
    user_code: String,
    // Google has historically used `verification_url`; newer responses also
    // include `verification_uri`. Accept either.
    #[serde(alias = "verification_uri")]
    verification_url: String,
    #[serde(default = "default_interval")]
    interval: u64,
    #[serde(default)]
    expires_in: u64,
}

fn default_interval() -> u64 {
    5
}

#[derive(Deserialize)]
struct TokenRaw {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    expires_in: i64,
}

#[derive(Deserialize)]
struct TokenError {
    error: String,
    #[serde(default)]
    error_description: Option<String>,
}

impl GoogleOAuth {
    pub fn new(client_id: impl Into<String>, client_secret: impl Into<String>) -> Self {
        Self {
            client_id: client_id.into(),
            client_secret: client_secret.into(),
            http: reqwest::Client::new(),
        }
    }

    /// Start the device authorization grant.
    pub async fn begin_device_code(&self) -> Result<DeviceCodePrompt, BackendError> {
        let resp = self
            .http
            .post(DEVICE_CODE_URL)
            .form(&[("client_id", self.client_id.as_str()), ("scope", SCOPE)])
            .send()
            .await
            .map_err(|e| BackendError::Network(e.to_string()))?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(BackendError::Auth(format!("device code request failed: {body}")));
        }

        let raw: DeviceCodeRaw = resp
            .json()
            .await
            .map_err(|e| BackendError::Parse(e.to_string()))?;
        Ok(DeviceCodePrompt {
            verification_uri: raw.verification_url,
            user_code: raw.user_code,
            device_code: raw.device_code,
            interval: raw.interval.max(1),
            expires_in: raw.expires_in,
        })
    }

    /// Poll the token endpoint until the user completes sign-in.
    pub async fn poll_for_token(
        &self,
        prompt: &DeviceCodePrompt,
    ) -> Result<TokenCache, BackendError> {
        let mut interval = prompt.interval;
        let deadline = Utc::now() + Duration::seconds(prompt.expires_in.max(60) as i64);

        loop {
            if Utc::now() > deadline {
                return Err(BackendError::Auth(
                    "device code expired before sign-in completed".to_string(),
                ));
            }
            tokio::time::sleep(StdDuration::from_secs(interval)).await;

            let resp = self
                .http
                .post(TOKEN_URL)
                .form(&[
                    ("client_id", self.client_id.as_str()),
                    ("client_secret", self.client_secret.as_str()),
                    ("device_code", prompt.device_code.as_str()),
                    (
                        "grant_type",
                        "urn:ietf:params:oauth:grant-type:device_code",
                    ),
                ])
                .send()
                .await
                .map_err(|e| BackendError::Network(e.to_string()))?;

            if resp.status().is_success() {
                let raw: TokenRaw = resp
                    .json()
                    .await
                    .map_err(|e| BackendError::Parse(e.to_string()))?;
                return into_cache(raw, None);
            }

            let err: TokenError = resp
                .json()
                .await
                .map_err(|e| BackendError::Parse(e.to_string()))?;
            match err.error.as_str() {
                "authorization_pending" => continue,
                "slow_down" => {
                    interval += 5;
                    continue;
                }
                other => {
                    let desc = err.error_description.unwrap_or_default();
                    return Err(BackendError::Auth(format!("{other}: {desc}")));
                }
            }
        }
    }

    /// Exchange a refresh token for a fresh access token.
    pub async fn refresh(&self, refresh_token: &str) -> Result<TokenCache, BackendError> {
        let resp = self
            .http
            .post(TOKEN_URL)
            .form(&[
                ("client_id", self.client_id.as_str()),
                ("client_secret", self.client_secret.as_str()),
                ("refresh_token", refresh_token),
                ("grant_type", "refresh_token"),
            ])
            .send()
            .await
            .map_err(|e| BackendError::Network(e.to_string()))?;

        if !resp.status().is_success() {
            let err: TokenError = resp
                .json()
                .await
                .map_err(|e| BackendError::Parse(e.to_string()))?;
            let desc = err.error_description.unwrap_or_default();
            return Err(BackendError::Auth(format!(
                "token refresh failed ({}): {desc}",
                err.error
            )));
        }

        let raw: TokenRaw = resp
            .json()
            .await
            .map_err(|e| BackendError::Parse(e.to_string()))?;
        into_cache(raw, Some(refresh_token))
    }

    /// Return a valid bearer access token, refreshing and persisting as needed.
    pub async fn access_token(&self, cache_path: &Path) -> Result<String, BackendError> {
        let cache = TokenCache::load(cache_path)?.ok_or_else(|| {
            BackendError::Auth(format!(
                "not signed in (no token cache at {}); run with --login",
                cache_path.display()
            ))
        })?;

        if cache.expires_at > Utc::now() + EXPIRY_MARGIN {
            return Ok(cache.access_token);
        }

        let refreshed = self.refresh(&cache.refresh_token).await?;
        refreshed.save(cache_path)?;
        Ok(refreshed.access_token)
    }
}

fn into_cache(raw: TokenRaw, prev_refresh: Option<&str>) -> Result<TokenCache, BackendError> {
    let refresh_token = raw
        .refresh_token
        .or_else(|| prev_refresh.map(str::to_string))
        .ok_or_else(|| {
            BackendError::Auth("token response contained no refresh token".to_string())
        })?;
    Ok(TokenCache {
        access_token: raw.access_token,
        refresh_token,
        expires_at: Utc::now() + Duration::seconds(raw.expires_in),
    })
}

/// A backend reading one Google calendar.
pub struct GoogleBackend {
    id: String,
    calendar_id: String,
    oauth: GoogleOAuth,
    cache_path: PathBuf,
    http: reqwest::Client,
}

impl GoogleBackend {
    pub fn new(
        id: impl Into<String>,
        client_id: impl Into<String>,
        client_secret: impl Into<String>,
        calendar_id: impl Into<String>,
        cache_path: impl Into<PathBuf>,
    ) -> Self {
        Self {
            id: id.into(),
            calendar_id: calendar_id.into(),
            oauth: GoogleOAuth::new(client_id, client_secret),
            cache_path: cache_path.into(),
            http: reqwest::Client::new(),
        }
    }

    pub fn oauth(&self) -> &GoogleOAuth {
        &self.oauth
    }

    pub fn cache_path(&self) -> &Path {
        &self.cache_path
    }
}

#[async_trait]
impl CalendarBackend for GoogleBackend {
    fn id(&self) -> &str {
        &self.id
    }

    async fn fetch_events(
        &self,
        window_start: DateTime<Utc>,
        window_end: DateTime<Utc>,
    ) -> Result<Vec<CalendarEvent>, BackendError> {
        let token = self.oauth.access_token(&self.cache_path).await?;

        let events_url = format!(
            "{EVENTS_BASE}/{}/events",
            urlencode(&self.calendar_id)
        );
        let time_min = window_start.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let time_max = window_end.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

        let mut out = Vec::new();
        let mut page_token: Option<String> = None;

        loop {
            let mut query: Vec<(&str, String)> = vec![
                ("timeMin", time_min.clone()),
                ("timeMax", time_max.clone()),
                ("singleEvents", "true".to_string()),
                ("orderBy", "startTime".to_string()),
                ("maxResults", "250".to_string()),
            ];
            if let Some(tok) = &page_token {
                query.push(("pageToken", tok.clone()));
            }

            let resp = self
                .http
                .get(&events_url)
                .bearer_auth(&token)
                .query(&query)
                .timeout(StdDuration::from_secs(30))
                .send()
                .await
                .map_err(|e| BackendError::Network(e.to_string()))?;

            let status = resp.status();
            let text = resp
                .text()
                .await
                .map_err(|e| BackendError::Network(e.to_string()))?;
            if status == reqwest::StatusCode::UNAUTHORIZED {
                return Err(BackendError::Auth(
                    "Google rejected the access token; run with --login to re-authenticate"
                        .to_string(),
                ));
            }
            if !status.is_success() {
                return Err(BackendError::Network(format!(
                    "Calendar API HTTP {status}: {text}"
                )));
            }

            let page = parse_page(&text)?;
            for raw in page.items {
                if let Some(event) = map_event(raw, &self.id) {
                    out.push(event);
                }
            }
            match page.next_page_token {
                Some(tok) => page_token = Some(tok),
                None => break,
            }
        }

        Ok(out)
    }
}

/// Minimal percent-encoding for a calendar id used as a path segment.
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[derive(Debug, Deserialize)]
struct EventsPage {
    #[serde(default)]
    items: Vec<GEvent>,
    #[serde(rename = "nextPageToken", default)]
    next_page_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GEvent {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    location: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(rename = "hangoutLink", default)]
    hangout_link: Option<String>,
    #[serde(rename = "conferenceData", default)]
    conference_data: Option<ConferenceData>,
    start: GTime,
    #[serde(default)]
    end: Option<GTime>,
    #[serde(default)]
    reminders: Option<GReminders>,
}

#[derive(Debug, Deserialize)]
struct GTime {
    #[serde(rename = "dateTime", default)]
    date_time: Option<String>,
    #[serde(default)]
    date: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GReminders {
    /// Present for completeness; an empty `overrides` already implies the
    /// account default reminder, which we map to `None`.
    #[serde(rename = "useDefault", default)]
    #[allow(dead_code)]
    use_default: bool,
    #[serde(default)]
    overrides: Vec<GReminderOverride>,
}

#[derive(Debug, Deserialize)]
struct GReminderOverride {
    #[serde(default)]
    minutes: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct ConferenceData {
    #[serde(rename = "entryPoints", default)]
    entry_points: Vec<EntryPoint>,
}

#[derive(Debug, Deserialize)]
struct EntryPoint {
    #[serde(default)]
    uri: Option<String>,
}

fn parse_page(text: &str) -> Result<EventsPage, BackendError> {
    serde_json::from_str(text).map_err(|e| BackendError::Parse(e.to_string()))
}

/// Map a Google event to the domain [`CalendarEvent`], or `None` if it is
/// cancelled or lacks a usable start time.
fn map_event(raw: GEvent, source_id: &str) -> Option<CalendarEvent> {
    if raw.status.as_deref() == Some("cancelled") {
        return None;
    }
    let id = raw.id?;
    let start = parse_gtime(&raw.start)?;
    let end = raw.end.as_ref().and_then(parse_gtime);

    let title = raw
        .summary
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "(no title)".to_string());

    let location = raw.location.filter(|s| !s.trim().is_empty());

    // Largest override lead time wins; `useDefault` (no overrides) means the
    // global default applies, represented as `None`.
    let reminder = raw.reminders.as_ref().and_then(|r| {
        r.overrides
            .iter()
            .filter_map(|o| o.minutes)
            .filter(|m| *m >= 0)
            .max()
            .map(|m| StdDuration::from_secs(m as u64 * 60))
    });

    // hangoutLink is the authoritative Google Meet link; otherwise scan
    // conferencing entry points, description, and location.
    let meeting = raw
        .hangout_link
        .filter(|u| !u.trim().is_empty())
        .map(|url| MeetingLink::from_known(MeetingProvider::GoogleMeet, url))
        .or_else(|| {
            let conf_uris: Vec<String> = raw
                .conference_data
                .map(|c| c.entry_points.into_iter().filter_map(|e| e.uri).collect())
                .unwrap_or_default();
            let description = raw.description.unwrap_or_default();
            let mut fields: Vec<&str> = conf_uris.iter().map(String::as_str).collect();
            fields.push(description.as_str());
            fields.push(location.as_deref().unwrap_or(""));
            MeetingLink::detect_in_fields(fields)
        });

    Some(CalendarEvent {
        id: format!("{source_id}:{id}"),
        source: source_id.to_string(),
        title,
        start,
        end,
        location,
        reminder,
        meeting,
    })
}

/// Convert a Google event time into UTC. Timed events carry an RFC 3339
/// `dateTime`; all-day events carry a `date` anchored at local midnight.
fn parse_gtime(t: &GTime) -> Option<DateTime<Utc>> {
    if let Some(dt) = &t.date_time {
        return DateTime::parse_from_rfc3339(dt)
            .ok()
            .map(|d| d.with_timezone(&Utc));
    }
    if let Some(date) = &t.date {
        let naive = NaiveDate::parse_from_str(date, "%Y-%m-%d")
            .ok()?
            .and_time(NaiveTime::from_hms_opt(0, 0, 0)?);
        return Local
            .from_local_datetime(&naive)
            .single()
            .map(|d| d.with_timezone(&Utc));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = r#"{
      "items": [
        {
          "id": "evt-meet",
          "status": "confirmed",
          "summary": "Design sync",
          "location": "Office",
          "description": "Agenda",
          "hangoutLink": "https://meet.google.com/abc-defg-hij",
          "start": { "dateTime": "2026-06-15T09:00:00+02:00" },
          "end": { "dateTime": "2026-06-15T10:00:00+02:00" },
          "reminders": { "useDefault": false, "overrides": [ { "method": "popup", "minutes": 10 }, { "method": "email", "minutes": 30 } ] }
        },
        {
          "id": "evt-allday",
          "status": "confirmed",
          "summary": "Holiday",
          "start": { "date": "2026-06-16" },
          "end": { "date": "2026-06-17" },
          "reminders": { "useDefault": true }
        },
        {
          "id": "evt-cancelled",
          "status": "cancelled",
          "start": { "dateTime": "2026-06-15T12:00:00Z" }
        },
        {
          "id": "evt-zoom",
          "status": "confirmed",
          "summary": "Vendor call",
          "description": "Join https://us02web.zoom.us/j/8412345678",
          "start": { "dateTime": "2026-06-15T14:00:00Z" }
        }
      ],
      "nextPageToken": "PAGE2"
    }"#;

    #[test]
    fn parses_page_and_next_token() {
        let page = parse_page(PAGE).unwrap();
        assert_eq!(page.items.len(), 4);
        assert_eq!(page.next_page_token.as_deref(), Some("PAGE2"));
    }

    #[test]
    fn maps_meet_event_with_largest_reminder() {
        let page = parse_page(PAGE).unwrap();
        let e = map_event(page.items.into_iter().next().unwrap(), "g").unwrap();
        assert_eq!(e.id, "g:evt-meet");
        assert_eq!(e.title, "Design sync");
        // 09:00 +02:00 == 07:00 UTC
        assert_eq!(e.start, Utc.with_ymd_and_hms(2026, 6, 15, 7, 0, 0).unwrap());
        assert_eq!(e.reminder, Some(StdDuration::from_secs(30 * 60)));
        let m = e.meeting.unwrap();
        assert_eq!(m.provider, MeetingProvider::GoogleMeet);
        assert_eq!(m.url, "https://meet.google.com/abc-defg-hij");
    }

    #[test]
    fn all_day_uses_default_reminder_none() {
        let page = parse_page(PAGE).unwrap();
        let e = map_event(page.items.into_iter().nth(1).unwrap(), "g").unwrap();
        assert_eq!(e.title, "Holiday");
        assert_eq!(e.reminder, None);
        assert!(e.meeting.is_none());
    }

    #[test]
    fn cancelled_event_skipped() {
        let page = parse_page(PAGE).unwrap();
        assert!(map_event(page.items.into_iter().nth(2).unwrap(), "g").is_none());
    }

    #[test]
    fn detects_zoom_from_description() {
        let page = parse_page(PAGE).unwrap();
        let e = map_event(page.items.into_iter().nth(3).unwrap(), "g").unwrap();
        let m = e.meeting.unwrap();
        assert_eq!(m.provider, MeetingProvider::Zoom);
        assert!(m.url.starts_with("https://us02web.zoom.us/j/"));
    }
}
