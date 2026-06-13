//! OAuth2 **device authorization grant** for Microsoft identity platform, plus a
//! small on-disk token cache used by the Microsoft Graph backend.
//!
//! Flow:
//! 1. [`OAuthClient::begin_device_code`] starts the grant and returns a
//!    verification URL + user code to show the user.
//! 2. [`OAuthClient::poll_for_token`] polls the token endpoint until the user
//!    finishes signing in (honouring `authorization_pending` / `slow_down`).
//! 3. The resulting [`TokenCache`] is persisted (`0600`) and later refreshed
//!    automatically via [`OAuthClient::access_token`].
//!
//! No client secret is involved: this targets a *public client* Azure app with
//! "allow public client flows" enabled.

use std::path::{Path, PathBuf};
use std::time::Duration as StdDuration;

use chrono::{DateTime, Duration, Utc};
use cnm_core::backend::BackendError;
use serde::{Deserialize, Serialize};

/// Delegated scopes requested for reading the signed-in user's calendar.
/// `offline_access` is what yields a refresh token.
pub const SCOPES: &str = "offline_access Calendars.Read User.Read";

/// Refresh the access token when it is within this margin of expiring.
const EXPIRY_MARGIN: Duration = Duration::seconds(60);

/// Persisted OAuth tokens for a single account.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenCache {
    pub access_token: String,
    pub refresh_token: String,
    /// Absolute instant at which `access_token` expires.
    pub expires_at: DateTime<Utc>,
}

impl TokenCache {
    /// Whether the access token is still valid (with a safety margin).
    pub fn is_fresh(&self, now: DateTime<Utc>) -> bool {
        self.expires_at > now + EXPIRY_MARGIN
    }

    /// Load a cache from `path`, returning `None` if the file does not exist.
    pub fn load(path: &Path) -> Result<Option<TokenCache>, BackendError> {
        match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text)
                .map(Some)
                .map_err(|e| BackendError::Auth(format!("corrupt token cache: {e}"))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(BackendError::Auth(format!("reading token cache: {e}"))),
        }
    }

    /// Persist this cache to `path` with owner-only (`0600`) permissions.
    pub fn save(&self, path: &Path) -> Result<(), BackendError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| BackendError::Auth(format!("creating token dir: {e}")))?;
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| BackendError::Auth(format!("serializing tokens: {e}")))?;
        std::fs::write(path, text)
            .map_err(|e| BackendError::Auth(format!("writing token cache: {e}")))?;
        set_owner_only(path)?;
        Ok(())
    }
}

#[cfg(unix)]
fn set_owner_only(path: &Path) -> Result<(), BackendError> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|e| BackendError::Auth(format!("setting token file permissions: {e}")))
}

#[cfg(not(unix))]
fn set_owner_only(_path: &Path) -> Result<(), BackendError> {
    Ok(())
}

/// Details returned when starting a device-code grant; show these to the user.
#[derive(Debug, Clone)]
pub struct DeviceCodePrompt {
    /// Where the user should sign in (e.g. `https://microsoft.com/devicelogin`).
    pub verification_uri: String,
    /// Short code the user types at the verification URL.
    pub user_code: String,
    /// Opaque code the daemon polls with (not shown to the user).
    pub device_code: String,
    /// Seconds to wait between polls.
    pub interval: u64,
    /// Seconds until the device code expires.
    pub expires_in: u64,
    /// Full human-readable instruction message from the server, if any.
    pub message: Option<String>,
}

/// A configured OAuth client bound to a tenant + public-client application id.
pub struct OAuthClient {
    tenant: String,
    client_id: String,
    http: reqwest::Client,
}

#[derive(Deserialize)]
struct DeviceCodeRaw {
    device_code: String,
    user_code: String,
    verification_uri: String,
    #[serde(default = "default_interval")]
    interval: u64,
    #[serde(default)]
    expires_in: u64,
    #[serde(default)]
    message: Option<String>,
}

fn default_interval() -> u64 {
    5
}

#[derive(Deserialize)]
struct TokenRaw {
    access_token: String,
    /// Microsoft rotates refresh tokens; absent on some refresh responses, in
    /// which case the previous refresh token is reused.
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

impl OAuthClient {
    pub fn new(tenant: impl Into<String>, client_id: impl Into<String>) -> Self {
        Self {
            tenant: tenant.into(),
            client_id: client_id.into(),
            http: reqwest::Client::new(),
        }
    }

    fn authority(&self, leaf: &str) -> String {
        format!(
            "https://login.microsoftonline.com/{}/oauth2/v2.0/{leaf}",
            self.tenant
        )
    }

    /// Start the device authorization grant.
    pub async fn begin_device_code(&self) -> Result<DeviceCodePrompt, BackendError> {
        let resp = self
            .http
            .post(self.authority("devicecode"))
            .form(&[("client_id", self.client_id.as_str()), ("scope", SCOPES)])
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
            verification_uri: raw.verification_uri,
            user_code: raw.user_code,
            device_code: raw.device_code,
            interval: raw.interval.max(1),
            expires_in: raw.expires_in,
            message: raw.message,
        })
    }

    /// Poll the token endpoint until the user completes sign-in, then return a
    /// populated [`TokenCache`].
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
                .post(self.authority("token"))
                .form(&[
                    (
                        "grant_type",
                        "urn:ietf:params:oauth:grant-type:device_code",
                    ),
                    ("client_id", self.client_id.as_str()),
                    ("device_code", prompt.device_code.as_str()),
                ])
                .send()
                .await
                .map_err(|e| BackendError::Network(e.to_string()))?;

            if resp.status().is_success() {
                let raw: TokenRaw = resp
                    .json()
                    .await
                    .map_err(|e| BackendError::Parse(e.to_string()))?;
                return Ok(into_cache(raw, None)?);
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
            .post(self.authority("token"))
            .form(&[
                ("grant_type", "refresh_token"),
                ("client_id", self.client_id.as_str()),
                ("refresh_token", refresh_token),
                ("scope", SCOPES),
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
        // Reuse the prior refresh token if the server didn't rotate it.
        into_cache(raw, Some(refresh_token))
    }

    /// Return a valid bearer access token for the account whose tokens live at
    /// `cache_path`, refreshing and persisting transparently when needed.
    pub async fn access_token(&self, cache_path: &Path) -> Result<String, BackendError> {
        let cache = TokenCache::load(cache_path)?.ok_or_else(|| {
            BackendError::Auth(format!(
                "not signed in (no token cache at {}); run with --login",
                cache_path.display()
            ))
        })?;

        if cache.is_fresh(Utc::now()) {
            return Ok(cache.access_token);
        }

        let refreshed = self.refresh(&cache.refresh_token).await?;
        refreshed.save(cache_path)?;
        Ok(refreshed.access_token)
    }
}

/// Build a [`TokenCache`] from a raw token response, falling back to
/// `prev_refresh` when the response omits a (rotated) refresh token.
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

/// Per-backend token cache path under the user's data dir, namespaced by
/// `provider` (e.g. `msgraph`, `google`) and `backend_id`.
pub fn cache_path(provider: &str, backend_id: &str) -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "calendar-notifications-modal")
        .map(|p| p.data_dir().join(format!("{provider}-{backend_id}.json")))
}

/// Default per-backend Microsoft Graph token cache path under the user's data dir.
pub fn default_cache_path(backend_id: &str) -> Option<PathBuf> {
    cache_path("msgraph", backend_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_cache_roundtrip_and_perms() {
        let dir = std::env::temp_dir().join(format!("cnm-auth-test-{}", std::process::id()));
        let path = dir.join("msgraph-test.json");
        let cache = TokenCache {
            access_token: "at".into(),
            refresh_token: "rt".into(),
            expires_at: Utc::now() + Duration::seconds(3600),
        };
        cache.save(&path).unwrap();

        let loaded = TokenCache::load(&path).unwrap().unwrap();
        assert_eq!(loaded.access_token, "at");
        assert_eq!(loaded.refresh_token, "rt");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_missing_is_none() {
        let path = std::env::temp_dir().join("cnm-definitely-missing-token.json");
        let _ = std::fs::remove_file(&path);
        assert!(TokenCache::load(&path).unwrap().is_none());
    }

    #[test]
    fn freshness_respects_margin() {
        let now = Utc::now();
        let almost = TokenCache {
            access_token: "a".into(),
            refresh_token: "r".into(),
            expires_at: now + Duration::seconds(30),
        };
        assert!(!almost.is_fresh(now));
        let good = TokenCache {
            expires_at: now + Duration::seconds(3600),
            ..almost.clone()
        };
        assert!(good.is_fresh(now));
    }

    #[test]
    fn refresh_token_reused_when_absent() {
        let raw = TokenRaw {
            access_token: "new-at".into(),
            refresh_token: None,
            expires_in: 3600,
        };
        let cache = into_cache(raw, Some("old-rt")).unwrap();
        assert_eq!(cache.refresh_token, "old-rt");
        assert_eq!(cache.access_token, "new-at");
    }
}
