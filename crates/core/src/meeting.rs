//! Online-meeting link detection (Microsoft Teams, Google Meet, Zoom).

use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};

/// Supported online meeting providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MeetingProvider {
    Teams,
    GoogleMeet,
    Zoom,
}

impl MeetingProvider {
    pub fn display_name(&self) -> &'static str {
        match self {
            MeetingProvider::Teams => "Microsoft Teams",
            MeetingProvider::GoogleMeet => "Google Meet",
            MeetingProvider::Zoom => "Zoom",
        }
    }
}

/// A detected join link for an online meeting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeetingLink {
    pub provider: MeetingProvider,
    pub url: String,
}

// Ordered most-specific first. Each provider maps to a URL pattern.
static PATTERNS: Lazy<Vec<(MeetingProvider, Regex)>> = Lazy::new(|| {
    vec![
        (
            MeetingProvider::Teams,
            Regex::new(r#"https://teams\.microsoft\.com/l/meetup-join/[^\s<>"')]+"#).unwrap(),
        ),
        (
            MeetingProvider::Teams,
            Regex::new(r#"https://teams\.live\.com/meet/[^\s<>"')]+"#).unwrap(),
        ),
        (
            MeetingProvider::GoogleMeet,
            Regex::new(r#"https://meet\.google\.com/[a-z]{3}-[a-z]{4}-[a-z]{3}[^\s<>"')]*"#).unwrap(),
        ),
        (
            MeetingProvider::Zoom,
            Regex::new(r#"https://[A-Za-z0-9.-]*zoom\.us/(?:j|my|w)/[^\s<>"')]+"#).unwrap(),
        ),
    ]
});

impl MeetingLink {
    /// Wrap an already-trusted join URL whose provider is known from structured
    /// metadata (e.g., a Microsoft Graph `onlineMeeting.joinUrl`), without
    /// re-running link detection.
    pub fn from_known(provider: MeetingProvider, url: impl Into<String>) -> MeetingLink {
        MeetingLink {
            provider,
            url: url.into(),
        }
    }

    /// Scan free-form text (event body, location, or conferencing field) for the
    /// first recognized meeting link. Providers are checked in priority order
    /// (Teams, Google Meet, Zoom); the earliest match in the text wins for a
    /// given provider.
    pub fn detect(text: &str) -> Option<MeetingLink> {
        let mut best: Option<(usize, MeetingProvider, String)> = None;
        for (provider, re) in PATTERNS.iter() {
            if let Some(m) = re.find(text) {
                let candidate = (m.start(), *provider, m.as_str().to_string());
                match &best {
                    Some((pos, _, _)) if *pos <= candidate.0 => {}
                    _ => best = Some(candidate),
                }
            }
        }
        best.map(|(_, provider, url)| MeetingLink { provider, url })
    }

    /// Convenience: detect across several candidate fields, returning the first
    /// hit in field order.
    pub fn detect_in_fields<'a, I>(fields: I) -> Option<MeetingLink>
    where
        I: IntoIterator<Item = &'a str>,
    {
        fields.into_iter().find_map(MeetingLink::detect)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_teams() {
        let body = "Join here: https://teams.microsoft.com/l/meetup-join/19%3ameeting_abc/0 thanks";
        let link = MeetingLink::detect(body).unwrap();
        assert_eq!(link.provider, MeetingProvider::Teams);
        assert!(link.url.contains("meetup-join"));
    }

    #[test]
    fn detects_google_meet() {
        let body = "Video call: https://meet.google.com/abc-defg-hij";
        let link = MeetingLink::detect(body).unwrap();
        assert_eq!(link.provider, MeetingProvider::GoogleMeet);
        assert_eq!(link.url, "https://meet.google.com/abc-defg-hij");
    }

    #[test]
    fn detects_zoom() {
        let body = "Topic\nhttps://us02web.zoom.us/j/8412345678?pwd=Xy more text";
        let link = MeetingLink::detect(body).unwrap();
        assert_eq!(link.provider, MeetingProvider::Zoom);
        assert!(link.url.starts_with("https://us02web.zoom.us/j/"));
    }

    #[test]
    fn none_when_absent() {
        assert!(MeetingLink::detect("no links here").is_none());
    }

    #[test]
    fn earliest_match_wins() {
        let body = "https://meet.google.com/abc-defg-hij and https://us02web.zoom.us/j/123";
        let link = MeetingLink::detect(body).unwrap();
        assert_eq!(link.provider, MeetingProvider::GoogleMeet);
    }
}
