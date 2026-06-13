//! Persistent reminder-state storage backed by SQLite (rusqlite).
//!
//! Stores per-occurrence reminder state (active / snoozed-until / dismissed) so
//! that dismissals and snoozes survive daemon restarts. The database lives under
//! `$XDG_DATA_HOME/calendar-notifications-modal/state.db` by default.

use std::path::{Path, PathBuf};

use chrono::{DateTime, TimeZone, Utc};
use cnm_core::model::ReminderState;
use rusqlite::{Connection, OptionalExtension};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("could not determine data directory")]
    NoDataDir,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Handle to the reminder-state database.
pub struct Store {
    conn: Connection,
}

impl Store {
    /// Open (creating if needed) the database at the default XDG data location.
    pub fn open_default() -> Result<Self, StoreError> {
        let dir = default_data_dir().ok_or(StoreError::NoDataDir)?;
        std::fs::create_dir_all(&dir)?;
        Self::open(dir.join("state.db"))
    }

    /// Open (creating if needed) the database at `path`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let conn = Connection::open(path)?;
        Self::from_connection(conn)
    }

    /// Open an in-memory database (useful for tests).
    pub fn open_in_memory() -> Result<Self, StoreError> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(conn: Connection) -> Result<Self, StoreError> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS reminder_state (
                event_id     TEXT PRIMARY KEY,
                status       TEXT NOT NULL,          -- 'active' | 'snoozed' | 'dismissed'
                snooze_until INTEGER,                -- unix seconds, when status='snoozed'
                last_shown   INTEGER,                -- unix seconds
                updated_at   INTEGER NOT NULL
            );
            "#,
        )?;
        Ok(Self { conn })
    }

    /// Fetch the stored state for an event, defaulting to `Active` when unknown.
    pub fn get(&self, event_id: &str) -> Result<ReminderState, StoreError> {
        let row = self
            .conn
            .query_row(
                "SELECT status, snooze_until FROM reminder_state WHERE event_id = ?1",
                [event_id],
                |r| {
                    let status: String = r.get(0)?;
                    let snooze_until: Option<i64> = r.get(1)?;
                    Ok((status, snooze_until))
                },
            )
            .optional()?;

        Ok(match row {
            None => ReminderState::Active,
            Some((status, snooze_until)) => match status.as_str() {
                "dismissed" => ReminderState::Dismissed,
                "snoozed" => match snooze_until.and_then(|s| Utc.timestamp_opt(s, 0).single()) {
                    Some(until) => ReminderState::Snoozed { until },
                    None => ReminderState::Active,
                },
                _ => ReminderState::Active,
            },
        })
    }

    /// Persist `state` for `event_id`.
    pub fn set(&self, event_id: &str, state: &ReminderState) -> Result<(), StoreError> {
        let now = Utc::now().timestamp();
        let (status, snooze_until): (&str, Option<i64>) = match state {
            ReminderState::Active => ("active", None),
            ReminderState::Dismissed => ("dismissed", None),
            ReminderState::Snoozed { until } => ("snoozed", Some(until.timestamp())),
        };
        self.conn.execute(
            r#"
            INSERT INTO reminder_state (event_id, status, snooze_until, updated_at)
            VALUES (?1, ?2, ?3, ?4)
            ON CONFLICT(event_id) DO UPDATE SET
                status = excluded.status,
                snooze_until = excluded.snooze_until,
                updated_at = excluded.updated_at
            "#,
            rusqlite::params![event_id, status, snooze_until, now],
        )?;
        Ok(())
    }

    /// Convenience: mark an event dismissed.
    pub fn dismiss(&self, event_id: &str) -> Result<(), StoreError> {
        self.set(event_id, &ReminderState::Dismissed)
    }

    /// Convenience: snooze an event until `until`.
    pub fn snooze(&self, event_id: &str, until: DateTime<Utc>) -> Result<(), StoreError> {
        self.set(event_id, &ReminderState::Snoozed { until })
    }

    /// Record that an event was shown to the user at `when`.
    pub fn mark_shown(&self, event_id: &str, when: DateTime<Utc>) -> Result<(), StoreError> {
        let now = Utc::now().timestamp();
        self.conn.execute(
            r#"
            INSERT INTO reminder_state (event_id, status, last_shown, updated_at)
            VALUES (?1, 'active', ?2, ?3)
            ON CONFLICT(event_id) DO UPDATE SET
                last_shown = excluded.last_shown,
                updated_at = excluded.updated_at
            "#,
            rusqlite::params![event_id, when.timestamp(), now],
        )?;
        Ok(())
    }

    /// Delete state rows that have not been updated since `older_than`, to keep
    /// the database from growing unbounded with stale past events.
    pub fn purge_older_than(&self, older_than: DateTime<Utc>) -> Result<usize, StoreError> {
        let n = self.conn.execute(
            "DELETE FROM reminder_state WHERE updated_at < ?1",
            [older_than.timestamp()],
        )?;
        Ok(n)
    }
}

/// Default data directory for the application.
pub fn default_data_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "calendar-notifications-modal")
        .map(|p| p.data_dir().to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn unknown_defaults_to_active() {
        let s = Store::open_in_memory().unwrap();
        assert_eq!(s.get("nope").unwrap(), ReminderState::Active);
    }

    #[test]
    fn dismiss_roundtrip() {
        let s = Store::open_in_memory().unwrap();
        s.dismiss("e1").unwrap();
        assert_eq!(s.get("e1").unwrap(), ReminderState::Dismissed);
    }

    #[test]
    fn snooze_roundtrip() {
        let s = Store::open_in_memory().unwrap();
        let until = Utc::now() + Duration::minutes(10);
        // truncate to seconds to match storage granularity
        let until = Utc.timestamp_opt(until.timestamp(), 0).single().unwrap();
        s.snooze("e2", until).unwrap();
        assert_eq!(s.get("e2").unwrap(), ReminderState::Snoozed { until });
    }

    #[test]
    fn set_overwrites() {
        let s = Store::open_in_memory().unwrap();
        s.dismiss("e3").unwrap();
        s.set("e3", &ReminderState::Active).unwrap();
        assert_eq!(s.get("e3").unwrap(), ReminderState::Active);
    }

    #[test]
    fn purge_removes_old_rows() {
        let s = Store::open_in_memory().unwrap();
        s.dismiss("old").unwrap();
        // Everything updated "now"; purging with a future cutoff removes it.
        let removed = s.purge_older_than(Utc::now() + Duration::days(1)).unwrap();
        assert_eq!(removed, 1);
        assert_eq!(s.get("old").unwrap(), ReminderState::Active);
    }
}
