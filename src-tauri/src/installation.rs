//! Exchange a GitHub App JWT for a short-lived installation access token.
//!
//! Per GitHub: <https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/authenticating-as-a-github-app-installation>

use chrono::{DateTime, Utc};
use reqwest::Client;

use crate::jwt_auth::generate_jwt;
use crate::credentials;

// ────────────────────────────────────────────────────────────────────────────
// Types
// ────────────────────────────────────────────────────────────────────────────

/// Raw JSON from `POST /app/installations/{id}/access_tokens`.
#[derive(serde::Deserialize, Debug)]
struct TokenResponseJson {
    token: String,
    expires_at: String,
    #[allow(dead_code)]
    permissions: Option<serde_json::Map<String, serde_json::Value>>,
    #[allow(dead_code)]
    repositories: Option<Vec<serde_json::Value>>,
}

/// Parsed, ready-to-use installation token.
#[derive(Debug, Clone)]
pub struct InstallationToken {
    /// Bearer token (prefix `ghs_`).
    pub value: String,
    /// Unix-seconds when this token expires.
    pub expires_at_unix: u64,
}

// ────────────────────────────────────────────────────────────────────────────
// HTTP / env helpers
// ────────────────────────────────────────────────────────────────────────────

fn client_id_from_env() -> Result<String, String> {
    std::env::var("GH_CLIENT_ID")
        .map_err(|_| "GH_CLIENT_ID not set in .env".to_owned())
}

fn api_client() -> Client {
    reqwest::Client::builder()
        .user_agent("hiem-app/0.1 (+github.com/boozelee/hiem)")
        .build()
        .unwrap_or_default()
}

#[inline]
fn now_unix() -> u64 {
    Utc::now().timestamp() as u64
}

// ────────────────────────────────────────────────────────────────────────────
// Core
// ────────────────────────────────────────────────────────────────────────────

/// Request a new installation access token.
pub async fn request_installation_token(
    installation_id: u64,
    repositories: Option<&[&str]>,
) -> Result<InstallationToken, String> {
    let pem = credentials::load_pem()?
        .ok_or("No GitHub App private key found. Store it first via store_github_app_key.")?;
    let jwt = generate_jwt(&pem, &client_id_from_env()?)?;

    let client = api_client();

    let mut body = serde_json::Map::new();
    if let Some(repos) = repositories {
        if !repos.is_empty() {
            body.insert(
                "repositories".to_string(),
                serde_json::Value::Array(
                    repos.iter().map(|r| serde_json::Value::String((*r).to_string())).collect()
                ),
            );
        }
    }

    let resp = client
        .post(format!(
            "https://api.github.com/app/installations/{}/access_tokens",
            installation_id
        ))
        .bearer_auth(&jwt)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2026-03-10")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("GitHub network error: {}", e))?;

    let status = resp.status();
    eprintln!(
        "[installation] POST /installations/{}/access_tokens → {}",
        installation_id, status
    );

    if !status.is_success() {
        let err_body = resp.text().await.unwrap_or_default();
        eprintln!("[installation] error body: {}", err_body);
        return Err(format!(
            "GitHub API error {} — check installation ID and app permissions: {}",
            status, err_body
        ));
    }

    let data: TokenResponseJson = resp
        .json()
        .await
        .map_err(|e| format!("failed to deserialise token response: {}", e))?;

    if data.token.is_empty() {
        return Err("GitHub returned an empty token".to_owned());
    }

    let expires_at_unix = parse_rfc3339(&data.expires_at)
        .unwrap_or_else(|e| {
            eprintln!(
                "[installation] warn: could not parse expires_at={:?}: {} — \
                 defaulting to 1 h from now",
                data.expires_at, e
            );
            now_unix() + 3600
        });

    Ok(InstallationToken {
        value: data.token,
        expires_at_unix,
    })
}

// ────────────────────────────────────────────────────────────────────────────
// Keyring state helpers
// ────────────────────────────────────────────────────────────────────────────

/// Whether a GitHub App private key is stored in the OS keyring.
pub fn has_keyring_credentials() -> bool {
    credentials::has_keyring_entry()
}

/// Remove the private key from the OS keyring.
#[allow(dead_code)]
pub fn clear_keyring_credentials() -> Result<(), String> {
    credentials::delete_pem()
}

// ────────────────────────────────────────────────────────────────────────────
// Expiry helpers
// ────────────────────────────────────────────────────────────────────────────

/// Seconds remaining before `expires_at_unix`, or `None` if already past.
#[inline]
#[allow(dead_code)] // used by downstream guards once session-aware TTL wiring is done
pub fn secs_until_expiry(expires_at_unix: u64) -> Option<i64> {
    let now = now_unix() as i64;
    let exp = expires_at_unix as i64;
    (exp > now).then_some(exp - now)
}

/// `true` if `expires_at_unix` is fewer than 5 minutes away.
#[allow(dead_code)] // called by refresh-guard after session-aware TTL wiring
pub fn should_refresh(expires_at_unix: u64) -> bool {
    secs_until_expiry(expires_at_unix)
        .map(|s| s < 300)
        .unwrap_or(true)
}

// ────────────────────────────────────────────────────────────────────────────
// RFC 3339 → unix-seconds
// ────────────────────────────────────────────────────────────────────────────

/// Parse a GitHub-style RFC 3339 datetime string (e.g. `"2024-01-02T15:04:05Z"`)
/// into unix-seconds since the Unix epoch.
fn parse_rfc3339(s: &str) -> Result<u64, String> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc).timestamp() as u64)
        .map_err(|e| format!("invalid RFC 3339 '{}': {}", s, e))
}

// ────────────────────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339_z_roundtrip() {
        assert_eq!(
            parse_rfc3339("2024-06-15T12:30:00Z").unwrap(),
            parse_rfc3339("2024-06-15T12:30:00Z").unwrap(),
        );
    }

    #[test]
    fn rfc3339_utc_offset_is_same_as_z() {
        let z = parse_rfc3339("2024-06-15T12:30:00Z").unwrap();
        let off = parse_rfc3339("2024-06-15T12:30:00+00:00").unwrap();
        assert_eq!(z, off);
    }

    #[test]
    fn should_refresh_five_min_rule() {
        assert!(should_refresh(now_unix().saturating_sub(1)));
        assert!(should_refresh(now_unix() + 299));
        assert!(!should_refresh(now_unix() + 600));
    }

    #[test]
    fn secs_until_expiry_past_returns_none() {
        assert_eq!(secs_until_expiry(0), None);
    }

    #[test]
    fn secs_until_expiry_future_succeeds() {
        assert_eq!(secs_until_expiry(now_unix() + 120), Some(120));
    }
}
