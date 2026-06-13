//! iCalendar (`.ics`) backend: reads events from a local file or HTTP(S) URL.

use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration as StdDuration;

use async_trait::async_trait;
use chrono::{DateTime, Local, NaiveTime, TimeZone, Utc};
use cnm_core::backend::{BackendError, CalendarBackend};
use cnm_core::meeting::MeetingLink;
use cnm_core::model::CalendarEvent;
use icalendar::{
    Calendar, CalendarDateTime, Component, DatePerhapsTime, EventLike, Related,
};

/// Where an [`IcsBackend`] reads its data from.
#[derive(Debug, Clone)]
pub enum IcsSource {
    /// A local filesystem path to an `.ics` file.
    File(PathBuf),
    /// An HTTP(S) URL serving iCalendar data (e.g. a published calendar).
    Url(String),
}

/// A backend backed by a single iCalendar source.
pub struct IcsBackend {
    id: String,
    source: IcsSource,
}

impl IcsBackend {
    pub fn new(id: impl Into<String>, source: IcsSource) -> Self {
        Self {
            id: id.into(),
            source,
        }
    }

    pub fn from_file(id: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        Self::new(id, IcsSource::File(path.into()))
    }

    pub fn from_url(id: impl Into<String>, url: impl Into<String>) -> Self {
        Self::new(id, IcsSource::Url(url.into()))
    }

    async fn load_text(&self) -> Result<String, BackendError> {
        match &self.source {
            IcsSource::File(path) => {
                std::fs::read_to_string(path).map_err(|e| BackendError::Other(e.to_string()))
            }
            IcsSource::Url(url) => {
                let resp = reqwest::get(url)
                    .await
                    .map_err(|e| BackendError::Network(e.to_string()))?;
                if !resp.status().is_success() {
                    return Err(BackendError::Network(format!("HTTP {}", resp.status())));
                }
                resp.text()
                    .await
                    .map_err(|e| BackendError::Network(e.to_string()))
            }
        }
    }
}

#[async_trait]
impl CalendarBackend for IcsBackend {
    fn id(&self) -> &str {
        &self.id
    }

    async fn fetch_events(
        &self,
        window_start: DateTime<Utc>,
        window_end: DateTime<Utc>,
    ) -> Result<Vec<CalendarEvent>, BackendError> {
        let text = self.load_text().await?;
        parse_ics(&text, &self.id, window_start, window_end)
    }
}

/// Parse iCalendar `text` into events whose start falls in `[window_start, window_end]`.
///
/// Pure and synchronous so it can be unit-tested without any I/O.
pub fn parse_ics(
    text: &str,
    source_id: &str,
    window_start: DateTime<Utc>,
    window_end: DateTime<Utc>,
) -> Result<Vec<CalendarEvent>, BackendError> {
    let calendar = Calendar::from_str(text).map_err(BackendError::Parse)?;

    let mut out = Vec::new();
    for component in &calendar.components {
        let Some(event) = component.as_event() else {
            continue;
        };

        let Some(start) = event.get_start().and_then(date_perhaps_to_utc) else {
            continue; // events without a usable start can't be reminded on
        };
        if start < window_start || start > window_end {
            continue;
        }

        let end = event.get_end().and_then(date_perhaps_to_utc);
        let title = event
            .get_summary()
            .map(str::to_string)
            .unwrap_or_else(|| "(no title)".to_string());
        let location = event.get_location().map(str::to_string);
        let description = event.get_description().unwrap_or("");
        let conference = event
            .property_value("X-GOOGLE-CONFERENCE")
            .or_else(|| event.property_value("URL"))
            .unwrap_or("");

        let meeting = MeetingLink::detect_in_fields([
            description,
            location.as_deref().unwrap_or(""),
            conference,
        ]);

        let reminder = extract_reminder(event);

        let uid = event
            .get_uid()
            .map(str::to_string)
            .unwrap_or_else(|| format!("{:x}", start.timestamp()));
        let id = format!("{source_id}:{uid}:{}", start.timestamp());

        out.push(CalendarEvent {
            id,
            source: source_id.to_string(),
            title,
            start,
            end,
            location,
            reminder,
            meeting,
        });
    }

    Ok(out)
}

/// Extract a "minutes before start" reminder lead time from the event's VALARMs.
///
/// Honours `TRIGGER` durations that are relative to the event start (the common
/// case, e.g. `-PT15M`). End-relative and absolute-time triggers are ignored, in
/// which case the engine's global default applies. When multiple alarms exist,
/// the earliest (largest lead time) wins.
fn extract_reminder(event: &icalendar::Event) -> Option<StdDuration> {
    let mut best: Option<StdDuration> = None;
    for sub in event.components() {
        if !sub.component_kind().eq_ignore_ascii_case("VALARM") {
            continue;
        }
        let Some(prop) = sub.properties().get("TRIGGER") else {
            continue;
        };
        // Skip absolute-time triggers (VALUE=DATE-TIME); only durations are leads.
        if prop.value_type() == Some(icalendar::ValueType::DateTime) {
            continue;
        }
        // RELATED defaults to START when unspecified; ignore END-relative triggers.
        let related = prop.get_param_as("RELATED", |s| Related::from_str(s).ok());
        let related_to_start = matches!(related, None | Some(Related::Start));
        if !related_to_start {
            continue;
        }
        // Negative durations (e.g. "-PT15M") mean "before start" -> the lead time.
        if let Some((negative, dur)) = parse_ical_duration(prop.value()) {
            if negative {
                best = Some(match best {
                    Some(prev) if prev >= dur => prev,
                    _ => dur,
                });
            }
        }
    }
    best
}

/// Parse an RFC 5545 / ISO 8601 duration such as `-PT15M`, `PT1H30M`, `-P1DT2H`
/// or `P2W` into `(is_negative, magnitude)`. Returns `None` on malformed input.
fn parse_ical_duration(s: &str) -> Option<(bool, StdDuration)> {
    let s = s.trim();
    let (negative, rest) = match s.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    let rest = rest.strip_prefix('P')?;

    let mut total: u64 = 0;
    let mut num = String::new();
    let mut in_time = false;
    for ch in rest.chars() {
        match ch {
            'T' => in_time = true,
            '0'..='9' => num.push(ch),
            unit => {
                let value: u64 = num.parse().ok()?;
                num.clear();
                let seconds = match (in_time, unit) {
                    (false, 'W') => value.checked_mul(7 * 24 * 3600)?,
                    (false, 'D') => value.checked_mul(24 * 3600)?,
                    (true, 'H') => value.checked_mul(3600)?,
                    (true, 'M') => value.checked_mul(60)?,
                    (true, 'S') => value,
                    _ => return None,
                };
                total = total.checked_add(seconds)?;
            }
        }
    }
    if !num.is_empty() {
        return None; // trailing number without a unit
    }
    Some((negative, StdDuration::from_secs(total)))
}

/// Convert an iCalendar date/time into UTC.
///
/// * UTC and zoned date-times convert directly (zoned via the bundled tz db).
/// * Floating (zoneless) date-times are interpreted in the local timezone.
/// * Date-only values (all-day events) are anchored at local midnight.
fn date_perhaps_to_utc(dpt: DatePerhapsTime) -> Option<DateTime<Utc>> {
    match dpt {
        DatePerhapsTime::DateTime(cdt) => match cdt {
            CalendarDateTime::Floating(naive) => Local
                .from_local_datetime(&naive)
                .single()
                .map(|dt| dt.with_timezone(&Utc)),
            other => other.try_into_utc(),
        },
        DatePerhapsTime::Date(date) => {
            let naive = date.and_time(NaiveTime::from_hms_opt(0, 0, 0)?);
            Local
                .from_local_datetime(&naive)
                .single()
                .map(|dt| dt.with_timezone(&Utc))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn window() -> (DateTime<Utc>, DateTime<Utc>) {
        (
            Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            Utc.with_ymd_and_hms(2026, 12, 31, 0, 0, 0).unwrap(),
        )
    }

    const SAMPLE: &str = "\
BEGIN:VCALENDAR\r
VERSION:2.0\r
PRODID:-//test//EN\r
BEGIN:VEVENT\r
UID:evt-1@example.com\r
SUMMARY:Standup\r
DTSTART:20260615T090000Z\r
DTEND:20260615T091500Z\r
LOCATION:https://meet.google.com/abc-defg-hij\r
DESCRIPTION:Daily sync\r
BEGIN:VALARM\r
ACTION:DISPLAY\r
TRIGGER:-PT15M\r
DESCRIPTION:Reminder\r
END:VALARM\r
END:VEVENT\r
END:VCALENDAR\r
";

    #[test]
    fn parses_basic_event() {
        let (ws, we) = window();
        let events = parse_ics(SAMPLE, "acct", ws, we).unwrap();
        assert_eq!(events.len(), 1);
        let e = &events[0];
        assert_eq!(e.title, "Standup");
        assert_eq!(e.source, "acct");
        assert_eq!(e.start, Utc.with_ymd_and_hms(2026, 6, 15, 9, 0, 0).unwrap());
        assert_eq!(e.end, Some(Utc.with_ymd_and_hms(2026, 6, 15, 9, 15, 0).unwrap()));
        assert!(e.id.starts_with("acct:evt-1@example.com:"));
    }

    #[test]
    fn extracts_reminder_from_valarm() {
        let (ws, we) = window();
        let e = &parse_ics(SAMPLE, "acct", ws, we).unwrap()[0];
        assert_eq!(e.reminder, Some(StdDuration::from_secs(15 * 60)));
    }

    #[test]
    fn detects_meeting_link_from_location() {
        let (ws, we) = window();
        let e = &parse_ics(SAMPLE, "acct", ws, we).unwrap()[0];
        let meeting = e.meeting.as_ref().unwrap();
        assert_eq!(meeting.url, "https://meet.google.com/abc-defg-hij");
    }

    #[test]
    fn filters_outside_window() {
        let ws = Utc.with_ymd_and_hms(2026, 7, 1, 0, 0, 0).unwrap();
        let we = Utc.with_ymd_and_hms(2026, 8, 1, 0, 0, 0).unwrap();
        // Event is in June, window is July -> excluded.
        let events = parse_ics(SAMPLE, "acct", ws, we).unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn no_reminder_without_valarm() {        let ics = "\
BEGIN:VCALENDAR\r
VERSION:2.0\r
PRODID:-//test//EN\r
BEGIN:VEVENT\r
UID:evt-2\r
SUMMARY:No alarm\r
DTSTART:20260615T100000Z\r
END:VEVENT\r
END:VCALENDAR\r
";
        let (ws, we) = window();
        let e = &parse_ics(ics, "acct", ws, we).unwrap()[0];
        assert_eq!(e.reminder, None);
        assert!(e.meeting.is_none());
        // stable id derived from uid + start
        assert!(e.id.contains("evt-2"));
        let _ = Duration::minutes(1); // keep chrono::Duration import used
    }

    #[test]
    fn parse_duration_variants() {
        assert_eq!(
            parse_ical_duration("-PT15M"),
            Some((true, StdDuration::from_secs(15 * 60)))
        );
        assert_eq!(
            parse_ical_duration("PT1H30M"),
            Some((false, StdDuration::from_secs(90 * 60)))
        );
        assert_eq!(
            parse_ical_duration("-P1DT2H"),
            Some((true, StdDuration::from_secs(26 * 3600)))
        );
        assert_eq!(
            parse_ical_duration("P2W"),
            Some((false, StdDuration::from_secs(14 * 24 * 3600)))
        );
        assert_eq!(parse_ical_duration("garbage"), None);
        assert_eq!(parse_ical_duration("PT15"), None);
    }
}
