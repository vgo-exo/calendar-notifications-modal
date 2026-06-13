//! Core domain types and the pluggable calendar backend trait.
//!
//! This crate is intentionally free of I/O specifics so it can be unit-tested
//! and shared by the store, engine, backends and UI crates.

pub mod backend;
pub mod meeting;
pub mod model;

pub use backend::{BackendError, CalendarBackend};
pub use meeting::{MeetingLink, MeetingProvider};
pub use model::{CalendarEvent, EventId, ReminderState, SnoozeOption};
