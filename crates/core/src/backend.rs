//! The pluggable calendar backend trait implemented by each provider.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use thiserror::Error;

use crate::model::CalendarEvent;

/// Errors a backend can surface while fetching events.
#[derive(Debug, Error)]
pub enum BackendError {
    #[error("network error: {0}")]
    Network(String),
    #[error("authentication error: {0}")]
    Auth(String),
    #[error("parse error: {0}")]
    Parse(String),
    #[error("configuration error: {0}")]
    Config(String),
    #[error("{0}")]
    Other(String),
}

/// A source of calendar events (ICS, CalDAV, Google, Microsoft Graph, ...).
///
/// Implementations must be cheap to keep alive for the lifetime of the daemon
/// and safe to call repeatedly from the polling loop.
#[async_trait]
pub trait CalendarBackend: Send + Sync {
    /// Stable identifier for this backend/account (used as `CalendarEvent::source`).
    fn id(&self) -> &str;

    /// Fetch all event occurrences that start within `[window_start, window_end]`.
    ///
    /// The engine queries a rolling window large enough to cover the maximum
    /// reminder lead time plus the poll interval.
    async fn fetch_events(
        &self,
        window_start: DateTime<Utc>,
        window_end: DateTime<Utc>,
    ) -> Result<Vec<CalendarEvent>, BackendError>;
}
