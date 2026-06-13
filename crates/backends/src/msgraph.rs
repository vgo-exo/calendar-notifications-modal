//! Microsoft Graph backend: reads a real Microsoft 365 / Outlook calendar.
//!
//! Events are fetched from the [`calendarView`] endpoint, which expands
//! recurrences server-side, so no local RRULE handling is needed. Authentication
//! uses the OAuth2 device-code flow (see [`crate::auth`]); the daemon only reads
//! a cached refresh token here and refreshes access tokens as required.
//!
//! [`calendarView`]: https://learn.microsoft.com/graph/api/calendar-list-calendarview

use std::path::PathBuf;
use std::time::Duration as StdDuration;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use cnm_core::backend::{BackendError, CalendarBackend};
use cnm_core::meeting::{MeetingLink, MeetingProvider};
use cnm_core::model::CalendarEvent;
use serde::Deserialize;

use crate::auth::OAuthClient;

const GRAPH_BASE: &str = "https://graph.microsoft.com/v1.0/me/calendarView";
const SELECT: &str = "id,subject,start,end,location,onlineMeeting,bodyPreview,body,isReminderOn,reminderMinutesBeforeStart";
const PAGE_SIZE: &str = "50";

/// A backend reading one user's calendar over Microsoft Graph.
pub struct MsGraphBackend {
    id: String,
    oauth: OAuthClient,
    cache_path: PathBuf,
    http: reqwest::Client,
}

impl MsGraphBackend {
    pub fn new(
        id: impl Into<String>,
        tenant: impl Into<String>,
        client_id: impl Into<String>,
        cache_path: impl Into<PathBuf>,
    ) -> Self {
        let id = id.into();
        Self {
            oauth: OAuthClient::new(tenant, client_id),
            cache_path: cache_path.into(),
            http: reqwest::Client::new(),
            id,
        }
    }

    /// Access the OAuth client (used by the `--login` CLI flow).
    pub fn oauth(&self) -> &OAuthClient {
        &self.oauth
    }

    /// Path of this backend's token cache.
    pub fn cache_path(&self) -> &std::path::Path {
        &self.cache_path
    }
}

#[async_trait]
impl CalendarBackend for MsGraphBackend {
    fn id(&self) -> &str {
        &self.id
    }

    async fn fetch_events(
        &self,
        window_start: DateTime<Utc>,
        window_end: DateTime<Utc>,
    ) -> Result<Vec<CalendarEvent>, BackendError> {
        let token = self.oauth.access_token(&self.cache_path).await?;

        let mut out = Vec::new();
        // First page: build the query explicitly. Subsequent pages follow the
        // server-provided @odata.nextLink verbatim.
        let mut next: Option<String> = None;
        let mut first = true;

        loop {
            let request = if let Some(url) = next.take() {
                self.http.get(url)
            } else if first {
                self.http.get(GRAPH_BASE).query(&[
                    ("startDateTime", window_start.format("%Y-%m-%dT%H:%M:%SZ").to_string()),
                    ("endDateTime", window_end.format("%Y-%m-%dT%H:%M:%SZ").to_string()),
                    ("$select", SELECT.to_string()),
                    ("$top", PAGE_SIZE.to_string()),
                    ("$orderby", "start/dateTime".to_string()),
                ])
            } else {
                break;
            };
            first = false;

            let resp = request
                .bearer_auth(&token)
                .header("Prefer", "outlook.timezone=\"UTC\"")
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
                    "Microsoft Graph rejected the access token; run with --login to re-authenticate"
                        .to_string(),
                ));
            }
            if !status.is_success() {
                return Err(BackendError::Network(format!("Graph HTTP {status}: {text}")));
            }

            let page = parse_page(&text)?;
            for raw in page.value {
                if let Some(event) = map_event(raw, &self.id) {
                    out.push(event);
                }
            }
            match page.next_link {
                Some(link) => next = Some(link),
                None => break,
            }
        }

        Ok(out)
    }
}

/// One page of the calendarView response.
#[derive(Debug, Deserialize)]
struct GraphPage {
    #[serde(default)]
    value: Vec<GraphEvent>,
    #[serde(rename = "@odata.nextLink", default)]
    next_link: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GraphEvent {
    id: String,
    #[serde(default)]
    subject: Option<String>,
    start: GraphDateTime,
    #[serde(default)]
    end: Option<GraphDateTime>,
    #[serde(default)]
    location: Option<GraphLocation>,
    #[serde(rename = "onlineMeeting", default)]
    online_meeting: Option<GraphOnlineMeeting>,
    #[serde(rename = "bodyPreview", default)]
    body_preview: Option<String>,
    #[serde(default)]
    body: Option<GraphBody>,
    #[serde(rename = "isReminderOn", default)]
    is_reminder_on: bool,
    #[serde(rename = "reminderMinutesBeforeStart", default)]
    reminder_minutes_before_start: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct GraphDateTime {
    #[serde(rename = "dateTime")]
    date_time: String,
    #[serde(rename = "timeZone", default)]
    time_zone: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GraphLocation {
    #[serde(rename = "displayName", default)]
    display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GraphOnlineMeeting {
    #[serde(rename = "joinUrl", default)]
    join_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GraphBody {
    #[serde(default)]
    content: Option<String>,
}

/// Parse a calendarView JSON page.
fn parse_page(text: &str) -> Result<GraphPage, BackendError> {
    serde_json::from_str(text).map_err(|e| BackendError::Parse(e.to_string()))
}

/// Map a Graph event to the domain [`CalendarEvent`], or `None` if it lacks a
/// usable start time.
fn map_event(raw: GraphEvent, source_id: &str) -> Option<CalendarEvent> {
    let start = parse_graph_datetime(&raw.start)?;
    let end = raw.end.as_ref().and_then(parse_graph_datetime);

    let title = raw
        .subject
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "(no title)".to_string());

    let location = raw
        .location
        .and_then(|l| l.display_name)
        .filter(|s| !s.trim().is_empty());

    let reminder = match (raw.is_reminder_on, raw.reminder_minutes_before_start) {
        (true, Some(mins)) if mins >= 0 => Some(StdDuration::from_secs(mins as u64 * 60)),
        _ => None,
    };

    // A Teams meeting is the authoritative join link when present; otherwise scan
    // the body / preview / location for any recognised provider.
    let meeting = raw
        .online_meeting
        .and_then(|m| m.join_url)
        .filter(|u| !u.trim().is_empty())
        .map(|url| MeetingLink::from_known(MeetingProvider::Teams, url))
        .or_else(|| {
            let body = raw.body.and_then(|b| b.content).unwrap_or_default();
            let preview = raw.body_preview.unwrap_or_default();
            MeetingLink::detect_in_fields([
                body.as_str(),
                preview.as_str(),
                location.as_deref().unwrap_or(""),
            ])
        });

    Some(CalendarEvent {
        id: format!("{source_id}:{}", raw.id),
        source: source_id.to_string(),
        title,
        start,
        end,
        location,
        reminder,
        meeting,
    })
}

/// Convert a Graph date/time into UTC.
///
/// With the `Prefer: outlook.timezone="UTC"` request header the values come back
/// in UTC, but without a trailing `Z`, e.g. `2026-06-15T09:00:00.0000000`. We
/// parse the naive value and anchor it to UTC (only when the declared zone is
/// UTC or unspecified; anything else is logged and still treated as UTC since we
/// asked for UTC).
fn parse_graph_datetime(dt: &GraphDateTime) -> Option<DateTime<Utc>> {
    // Fast path: a fully-qualified RFC 3339 timestamp.
    if let Ok(parsed) = DateTime::parse_from_rfc3339(&dt.date_time) {
        return Some(parsed.with_timezone(&Utc));
    }

    let naive = NaiveDateTime::parse_from_str(&dt.date_time, "%Y-%m-%dT%H:%M:%S%.f")
        .or_else(|_| NaiveDateTime::parse_from_str(&dt.date_time, "%Y-%m-%dT%H:%M:%S"))
        .ok()?;

    if let Some(tz) = &dt.time_zone {
        if !tz.eq_ignore_ascii_case("UTC") {
            tracing::warn!(
                "Graph returned non-UTC timezone '{tz}'; treating '{}' as UTC",
                dt.date_time
            );
        }
    }
    Some(Utc.from_utc_datetime(&naive))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = r#"{
      "value": [
        {
          "id": "evt-teams",
          "subject": "Sprint Review",
          "isReminderOn": true,
          "reminderMinutesBeforeStart": 15,
          "bodyPreview": "Review",
          "body": { "contentType": "html", "content": "<html>join</html>" },
          "start": { "dateTime": "2026-06-15T09:00:00.0000000", "timeZone": "UTC" },
          "end": { "dateTime": "2026-06-15T10:00:00.0000000", "timeZone": "UTC" },
          "location": { "displayName": "Room 1" },
          "onlineMeeting": { "joinUrl": "https://teams.microsoft.com/l/meetup-join/19%3ameeting_abc/0" }
        },
        {
          "id": "evt-zoom",
          "subject": "External sync",
          "isReminderOn": false,
          "start": { "dateTime": "2026-06-15T11:00:00.0000000", "timeZone": "UTC" },
          "end": { "dateTime": "2026-06-15T11:30:00.0000000", "timeZone": "UTC" },
          "body": { "contentType": "text", "content": "Join https://us02web.zoom.us/j/8412345678" }
        },
        {
          "id": "evt-plain",
          "subject": "",
          "start": { "dateTime": "2026-06-15T12:00:00.0000000", "timeZone": "UTC" }
        }
      ],
      "@odata.nextLink": "https://graph.microsoft.com/v1.0/me/calendarView?$skip=50"
    }"#;

    #[test]
    fn parses_next_link() {
        let page = parse_page(PAGE).unwrap();
        assert_eq!(page.value.len(), 3);
        assert!(page.next_link.unwrap().contains("$skip=50"));
    }

    #[test]
    fn maps_teams_meeting_and_reminder() {
        let page = parse_page(PAGE).unwrap();
        let e = map_event(page.value.into_iter().next().unwrap(), "acct").unwrap();
        assert_eq!(e.id, "acct:evt-teams");
        assert_eq!(e.source, "acct");
        assert_eq!(e.title, "Sprint Review");
        assert_eq!(e.start, Utc.with_ymd_and_hms(2026, 6, 15, 9, 0, 0).unwrap());
        assert_eq!(e.end, Some(Utc.with_ymd_and_hms(2026, 6, 15, 10, 0, 0).unwrap()));
        assert_eq!(e.location.as_deref(), Some("Room 1"));
        assert_eq!(e.reminder, Some(StdDuration::from_secs(15 * 60)));
        let m = e.meeting.unwrap();
        assert_eq!(m.provider, MeetingProvider::Teams);
        assert!(m.url.contains("meetup-join"));
    }

    #[test]
    fn detects_link_from_body_when_no_online_meeting() {
        let page = parse_page(PAGE).unwrap();
        let zoom = page.value.into_iter().nth(1).unwrap();
        let e = map_event(zoom, "acct").unwrap();
        assert_eq!(e.reminder, None);
        let m = e.meeting.unwrap();
        assert_eq!(m.provider, MeetingProvider::Zoom);
        assert!(m.url.starts_with("https://us02web.zoom.us/j/"));
    }

    #[test]
    fn untitled_event_gets_placeholder_and_no_end() {
        let page = parse_page(PAGE).unwrap();
        let plain = page.value.into_iter().nth(2).unwrap();
        let e = map_event(plain, "acct").unwrap();
        assert_eq!(e.title, "(no title)");
        assert!(e.end.is_none());
        assert!(e.meeting.is_none());
        assert!(e.reminder.is_none());
    }

    #[test]
    fn parses_rfc3339_datetime() {
        let dt = GraphDateTime {
            date_time: "2026-06-15T09:00:00Z".into(),
            time_zone: None,
        };
        assert_eq!(
            parse_graph_datetime(&dt),
            Some(Utc.with_ymd_and_hms(2026, 6, 15, 9, 0, 0).unwrap())
        );
    }
}
