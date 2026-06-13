//! Reminder engine: pure logic deciding which events are due, what snooze
//! options to offer, and when the daemon should next wake up.
//!
//! Everything here is deterministic given (`events`, `now`, `config`, and a
//! state lookup), which makes it straightforward to unit-test.

use std::time::Duration as StdDuration;

use chrono::{DateTime, Duration, Utc};
use cnm_core::model::{CalendarEvent, ReminderState, SnoozeOption};

/// Tunable parameters for the engine.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// Reminder lead time used when an event carries none of its own.
    pub global_default_reminder: StdDuration,
    /// Candidate "minutes before start" snooze values (multiples of 5).
    pub before_start_presets: Vec<i64>,
    /// Candidate "minutes from now" snooze values (multiples of 5).
    pub after_now_presets: Vec<i64>,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            global_default_reminder: StdDuration::from_secs(15 * 60),
            before_start_presets: vec![5, 10, 15, 30, 60],
            after_now_presets: vec![5, 10, 15, 30],
        }
    }
}

/// An event currently due to be shown, with everything the UI needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DueEvent {
    pub event: CalendarEvent,
    /// Context-aware snooze choices for this event at evaluation time.
    pub snooze_options: Vec<SnoozeOption>,
    /// Whether a "Join meeting" button should be offered.
    pub can_join: bool,
}

fn effective_state(state: &ReminderState, now: DateTime<Utc>) -> EffState {
    match state {
        ReminderState::Dismissed => EffState::Dismissed,
        ReminderState::Snoozed { until } => {
            if now >= *until {
                EffState::Active
            } else {
                EffState::SnoozedUntil(*until)
            }
        }
        ReminderState::Active => EffState::Active,
    }
}

enum EffState {
    Active,
    SnoozedUntil(DateTime<Utc>),
    Dismissed,
}

/// Generate the context-aware snooze options for an event at `now`.
///
/// * Before start: *"remind me again X minutes before start"* for every preset
///   `X` (a multiple of 5) that is strictly less than the minutes remaining
///   until start — guaranteeing the new reminder fires in the future.
/// * At/after start: *"remind me in X minutes"* for every preset.
pub fn snooze_options(
    event: &CalendarEvent,
    now: DateTime<Utc>,
    cfg: &EngineConfig,
) -> Vec<SnoozeOption> {
    let minutes_until_start = (event.start - now).num_minutes();
    if minutes_until_start > 0 {
        cfg.before_start_presets
            .iter()
            .copied()
            .filter(|&m| m >= 0 && m % 5 == 0 && m < minutes_until_start)
            .map(|minutes| SnoozeOption::BeforeStart { minutes })
            .collect()
    } else {
        cfg.after_now_presets
            .iter()
            .copied()
            .filter(|&m| m > 0 && m % 5 == 0)
            .map(|minutes| SnoozeOption::AfterNow { minutes })
            .collect()
    }
}

/// Compute the set of events that should currently be shown in the modal.
///
/// `state_of` returns the persisted [`ReminderState`] for an event id (callers
/// typically back this with the SQLite store; tests use an in-memory map).
pub fn compute_due<F>(
    events: &[CalendarEvent],
    now: DateTime<Utc>,
    cfg: &EngineConfig,
    state_of: F,
) -> Vec<DueEvent>
where
    F: Fn(&str) -> ReminderState,
{
    let mut due: Vec<DueEvent> = events
        .iter()
        .filter_map(|event| {
            let state = state_of(&event.id);
            match effective_state(&state, now) {
                EffState::Dismissed => None,
                EffState::SnoozedUntil(_) => None,
                EffState::Active => {
                    if now >= event.trigger_at(cfg.global_default_reminder) {
                        Some(DueEvent {
                            snooze_options: snooze_options(event, now, cfg),
                            can_join: event.meeting.is_some(),
                            event: event.clone(),
                        })
                    } else {
                        None
                    }
                }
            }
        })
        .collect();

    // Soonest-starting first, so the most imminent event is at the top.
    due.sort_by(|a, b| a.event.start.cmp(&b.event.start));
    due
}

/// Compute the next instant at which the due-set could change, so the daemon can
/// sleep until then instead of busy-polling. Returns `None` when nothing is
/// pending (no future triggers and no active snoozes).
pub fn next_wake<F>(
    events: &[CalendarEvent],
    now: DateTime<Utc>,
    cfg: &EngineConfig,
    state_of: F,
) -> Option<DateTime<Utc>>
where
    F: Fn(&str) -> ReminderState,
{
    let mut soonest: Option<DateTime<Utc>> = None;
    let mut consider = |t: DateTime<Utc>| {
        if t > now {
            soonest = Some(match soonest {
                Some(s) if s <= t => s,
                _ => t,
            });
        }
    };

    for event in events {
        match effective_state(&state_of(&event.id), now) {
            EffState::Dismissed => {}
            EffState::SnoozedUntil(until) => consider(until),
            EffState::Active => consider(event.trigger_at(cfg.global_default_reminder)),
        }
    }
    soonest
}

/// Resolve a chosen snooze option to the absolute wake instant to persist.
pub fn resolve_snooze(
    option: SnoozeOption,
    event: &CalendarEvent,
    now: DateTime<Utc>,
) -> DateTime<Utc> {
    option.wake_at(event.start, now)
}

// Internal helper kept public-in-crate for symmetry with chrono usage.
#[allow(dead_code)]
fn minutes(n: i64) -> Duration {
    Duration::minutes(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn ev(id: &str, start: DateTime<Utc>, reminder_min: Option<i64>) -> CalendarEvent {
        CalendarEvent {
            id: id.to_string(),
            source: "test".into(),
            title: format!("Event {id}"),
            start,
            end: None,
            location: None,
            reminder: reminder_min.map(|m| StdDuration::from_secs((m * 60) as u64)),
            meeting: None,
        }
    }

    fn cfg() -> EngineConfig {
        EngineConfig::default()
    }

    #[test]
    fn active_event_due_when_within_threshold() {
        let now = Utc::now();
        let e = ev("a", now + Duration::minutes(10), Some(15));
        let due = compute_due(&[e], now, &cfg(), |_| ReminderState::Active);
        assert_eq!(due.len(), 1);
    }

    #[test]
    fn not_due_before_threshold() {
        let now = Utc::now();
        let e = ev("a", now + Duration::minutes(30), Some(15));
        let due = compute_due(&[e], now, &cfg(), |_| ReminderState::Active);
        assert!(due.is_empty());
    }

    #[test]
    fn global_default_applies_without_event_reminder() {
        let now = Utc::now();
        // No per-event reminder -> global default is 15 min.
        let e = ev("a", now + Duration::minutes(12), None);
        let due = compute_due(&[e], now, &cfg(), |_| ReminderState::Active);
        assert_eq!(due.len(), 1);
    }

    #[test]
    fn dismissed_never_shown() {
        let now = Utc::now();
        let e = ev("a", now + Duration::minutes(5), Some(15));
        let due = compute_due(&[e], now, &cfg(), |_| ReminderState::Dismissed);
        assert!(due.is_empty());
    }

    #[test]
    fn snoozed_hidden_until_expiry() {
        let now = Utc::now();
        let e = ev("a", now + Duration::minutes(5), Some(15));
        let until = now + Duration::minutes(3);
        let due = compute_due(&[e.clone()], now, &cfg(), |_| ReminderState::Snoozed { until });
        assert!(due.is_empty());

        // After expiry it reappears.
        let later = until + Duration::seconds(1);
        let due2 = compute_due(&[e], later, &cfg(), |_| ReminderState::Snoozed { until });
        assert_eq!(due2.len(), 1);
    }

    #[test]
    fn before_start_snooze_options_are_future_only() {
        let now = Utc::now();
        let e = ev("a", now + Duration::minutes(22), Some(60));
        let opts = snooze_options(&e, now, &cfg());
        // presets 5,10,15,30,60 filtered to < 22 -> 5,10,15
        assert_eq!(
            opts,
            vec![
                SnoozeOption::BeforeStart { minutes: 5 },
                SnoozeOption::BeforeStart { minutes: 10 },
                SnoozeOption::BeforeStart { minutes: 15 },
            ]
        );
    }

    #[test]
    fn before_start_includes_zero_minute_preset() {
        let now = Utc::now();
        let e = ev("a", now + Duration::minutes(22), Some(60));
        let cfg = EngineConfig {
            before_start_presets: vec![0, 5, 10, 15, 30, 60],
            ..EngineConfig::default()
        };
        let opts = snooze_options(&e, now, &cfg);
        // presets 0,5,10,15,30,60 filtered to < 22 -> 0,5,10,15
        assert_eq!(
            opts,
            vec![
                SnoozeOption::BeforeStart { minutes: 0 },
                SnoozeOption::BeforeStart { minutes: 5 },
                SnoozeOption::BeforeStart { minutes: 10 },
                SnoozeOption::BeforeStart { minutes: 15 },
            ]
        );
    }

    #[test]
    fn after_start_offers_relative_snooze() {
        let now = Utc::now();
        let e = ev("a", now - Duration::minutes(2), Some(15));
        let opts = snooze_options(&e, now, &cfg());
        assert_eq!(
            opts,
            vec![
                SnoozeOption::AfterNow { minutes: 5 },
                SnoozeOption::AfterNow { minutes: 10 },
                SnoozeOption::AfterNow { minutes: 15 },
                SnoozeOption::AfterNow { minutes: 30 },
            ]
        );
    }

    #[test]
    fn resolve_snooze_before_start() {
        let now = Utc::now();
        let start = now + Duration::minutes(20);
        let e = ev("a", start, Some(15));
        let wake = resolve_snooze(SnoozeOption::BeforeStart { minutes: 5 }, &e, now);
        assert_eq!(wake, start - Duration::minutes(5));
    }

    #[test]
    fn resolve_snooze_after_now() {
        let now = Utc::now();
        let e = ev("a", now - Duration::minutes(1), Some(15));
        let wake = resolve_snooze(SnoozeOption::AfterNow { minutes: 10 }, &e, now);
        assert_eq!(wake, now + Duration::minutes(10));
    }

    #[test]
    fn next_wake_picks_soonest_trigger_or_snooze() {
        let now = Utc::now();
        let mut states: HashMap<String, ReminderState> = HashMap::new();
        states.insert("snoozed".into(), ReminderState::Snoozed { until: now + Duration::minutes(4) });

        let e_future = ev("future", now + Duration::minutes(30), Some(15)); // trigger in 15m
        let e_snoozed = ev("snoozed", now + Duration::minutes(50), Some(15));

        let events = vec![e_future, e_snoozed];
        let wake = next_wake(&events, now, &cfg(), |id| {
            states.get(id).cloned().unwrap_or(ReminderState::Active)
        });
        // Snooze expiry (4m) is sooner than the future trigger (15m).
        assert_eq!(wake, Some(now + Duration::minutes(4)));
    }

    #[test]
    fn due_sorted_by_start() {
        let now = Utc::now();
        let late = ev("late", now + Duration::minutes(5), Some(15));
        let early = ev("early", now - Duration::minutes(1), Some(15));
        let due = compute_due(&[late, early], now, &cfg(), |_| ReminderState::Active);
        assert_eq!(due[0].event.id, "early");
        assert_eq!(due[1].event.id, "late");
    }

    #[test]
    fn can_join_reflects_meeting_presence() {
        use cnm_core::meeting::{MeetingLink, MeetingProvider};
        let now = Utc::now();
        let mut e = ev("a", now, Some(15));
        e.meeting = Some(MeetingLink {
            provider: MeetingProvider::Zoom,
            url: "https://us02web.zoom.us/j/1".into(),
        });
        let due = compute_due(&[e], now, &cfg(), |_| ReminderState::Active);
        assert!(due[0].can_join);
    }
}
