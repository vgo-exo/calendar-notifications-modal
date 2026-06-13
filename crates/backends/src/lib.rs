//! Calendar backend implementations.
//!
//! [`ics::IcsBackend`] reads iCalendar data from a local file or HTTP(S) URL.
//! [`msgraph::MsGraphBackend`] reads a Microsoft 365 / Outlook calendar via the
//! Microsoft Graph API. [`google::GoogleBackend`] reads a Google Calendar via
//! the Calendar API. The Graph and Google backends authenticate with the OAuth2
//! device-code flow. Additional providers (CalDAV) can plug in behind the same
//! [`cnm_core::CalendarBackend`] trait.

#[cfg(feature = "ics")]
pub mod ics;

#[cfg(feature = "ics")]
pub use ics::IcsBackend;

#[cfg(any(feature = "msgraph", feature = "google"))]
pub mod auth;

#[cfg(feature = "msgraph")]
pub mod msgraph;

#[cfg(feature = "msgraph")]
pub use msgraph::MsGraphBackend;

#[cfg(feature = "google")]
pub mod google;

#[cfg(feature = "google")]
pub use google::GoogleBackend;
