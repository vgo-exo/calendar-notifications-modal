//! Core data model: calendar events, reminder state, and snooze options.

use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::meeting::MeetingLink;

/// Stable, unique identifier for a single (possibly recurring) event instance.
///
/// Backends are responsible for producing an id that is stable across polls for
/// the same occurrence (e.g. `"{account}:{uid}:{instance_start}"`), so reminder
/// state can be persisted and matched reliably.
pub type EventId = String;

/// A calendar event occurrence eligible to be shown in the reminder modal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalendarEvent {
    /// Stable unique id for this occurrence.
    pub id: EventId,
    /// Identifier of the backend/account this event came from.
    pub source: String,
    /// Human-readable title.
    pub title: String,
    /// Start time (UTC).
    pub start: DateTime<Utc>,
    /// End time (UTC), if known.
    pub end: Option<DateTime<Utc>>,
    /// Free-form location text, if any.
    pub location: Option<String>,
    /// Reminder lead time before `start`. `None` means the event has no
    /// reminder of its own and the global default should be applied.
    pub reminder: Option<Duration>,
    /// Detected online meeting link, if any.
    pub meeting: Option<MeetingLink>,
}

impl CalendarEvent {
    /// Effective reminder lead time, falling back to `global_default` when the
    /// event carries no reminder of its own.
    pub fn effective_reminder(&self, global_default: Duration) -> Duration {
        self.reminder.unwrap_or(global_default)
    }

    /// The earliest instant at which this event becomes eligible to be shown,
    /// i.e. `start - effective_reminder`.
    pub fn trigger_at(&self, global_default: Duration) -> DateTime<Utc> {
        let lead = chrono::Duration::from_std(self.effective_reminder(global_default))
            .unwrap_or_else(|_| chrono::Duration::zero());
        self.start - lead
    }
}

/// Persisted reminder state for a single event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReminderState {
    /// Eligible to be shown as soon as its trigger time is reached.
    Active,
    /// Hidden until `until`.
    Snoozed { until: DateTime<Utc> },
    /// Permanently dismissed for this occurrence.
    Dismissed,
}

impl Default for ReminderState {
    fn default() -> Self {
        ReminderState::Active
    }
}

/// A snooze choice offered to the user for a given event.
///
/// The dropdown is context-aware:
/// * Before the event starts we offer *"remind me again X minutes before
///   start"* (`BeforeStart`).
/// * Once the start time is in the past we offer *"remind me in X minutes"*
///   (`AfterNow`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SnoozeOption {
    /// Remind again `minutes` before the event start.
    BeforeStart { minutes: i64 },
    /// Remind again `minutes` from now.
    AfterNow { minutes: i64 },
}

impl SnoozeOption {
    /// Resolve this option to an absolute wake instant.
    pub fn wake_at(&self, event_start: DateTime<Utc>, now: DateTime<Utc>) -> DateTime<Utc> {
        match *self {
            SnoozeOption::BeforeStart { minutes } => event_start - chrono::Duration::minutes(minutes),
            SnoozeOption::AfterNow { minutes } => now + chrono::Duration::minutes(minutes),
        }
    }

    /// Human-readable label for the dropdown entry.
    pub fn label(&self) -> String {
        match *self {
            SnoozeOption::BeforeStart { minutes } => {
                format!("Remind me {minutes} minutes before start")
            }
            SnoozeOption::AfterNow { minutes } => format!("Remind me in {minutes} minutes"),
        }
    }
}
