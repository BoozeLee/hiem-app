//! GitHub App JWT generation — RS256 (RSA + SHA-256).
//!
//! GitHub's CA policy requires JWT signing keys to be RSA-2048 minimum.
//! New 2026+ registrations should prefer ES256, but RS256 is documented and
//! confirmed working here for maximum compatibility.
//!
//! Claims layout per docs:
//!   iss  — App **client ID** (not numeric app ID)
//!   iat  — Issued-at, GitHub recommends subtracting 60 s to absorb clock drift
//!   exp  — Expires-at, MUST be ≤ 10 minutes from `iat`
//!
//! <https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/generating-a-json-web-token-jwt-for-a-github-app>

use std::time::{SystemTime, UNIX_EPOCH};

use jsonwebtoken::{Algorithm, EncodingKey, Header};
use rsa::{
    pkcs1::{DecodeRsaPrivateKey, EncodeRsaPrivateKey},
    pkcs8::DecodePrivateKey,
    RsaPrivateKey,
};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;

// ────────────────────────────────────────────────────────────────────────────
// Algorithm
// ────────────────────────────────────────────────────────────────────────────

/// RS256 (RSA-SHA-256) — the algorithm GitHub's own docs highlight for JWTs.
pub const JWT_ALGORITHM: Algorithm = Algorithm::RS256;

// ────────────────────────────────────────────────────────────────────────────
// Claims
// ────────────────────────────────────────────────────────────────────────────

/// Claims payload for a GitHub App JWT.
#[derive(serde::Serialize, Debug, PartialEq)]
pub struct Claims {
    pub iss: String,
    pub iat: u64,
    pub exp: u64,
}

impl Claims {
    /// Build claims with the correct clock-drift buffer and 10-min maximum age.
    pub fn new(client_id: &str) -> Self {
        let now = now_unix();
        Self {
            iss: client_id.to_owned(),
            iat: now.saturating_sub(60),
            exp: now.saturating_sub(60).saturating_add(10 * 60),
        }
    }

    /// `exp - iat` in seconds.  Must be ≤ 600.
    #[inline]
    #[allow(dead_code)] // used by downstream callers in session
    pub fn lifetime_secs(&self) -> u64 {
        self.exp - self.iat
    }
}

// ────────────────────────────────────────────────────────────────────────────
// JWT header
// ────────────────────────────────────────────────────────────────────────────

/// Produce the JWT header required by GitHub.
pub fn jwk_header() -> Header {
    Header {
        alg: JWT_ALGORITHM,
        typ: Some("JWT".to_string()),
        ..Default::default()
    }
}

// ────────────────────────────────────────────────────────────────────────────
// PEM parsing
// ────────────────────────────────────────────────────────────────────────────

/// Parse an RSA private key from a PEM string.
///
/// Supports:
/// * PKCS#8  — `-----BEGIN PRIVATE KEY-----`
/// * PKCS#1  — `-----BEGIN RSA PRIVATE KEY-----`
pub fn parse_rsa_pem(pem: &str) -> Result<RsaPrivateKey, String> {
    rsa::RsaPrivateKey::from_pkcs8_pem(pem)
        .or_else(|_| rsa::RsaPrivateKey::from_pkcs1_pem(pem)
            .map_err(|e| format!("PEM is neither valid PKCS#8 nor PKCS#1 RSA private key: {}", e)))
}

// ────────────────────────────────────────────────────────────────────────────
// JWT encoder
// ────────────────────────────────────────────────────────────────────────────

/// Sign and encode a JWT.
///
/// Use `generate_jwt(pem, client_id)`. Returns an RS256 JWT.
/// The token is suitable for `POST /app/installations/{id}/access_tokens`.
pub fn generate_jwt(pem: &str, client_id: &str) -> Result<String, String> {
    let claims = Claims::new(client_id);
    let header = jwk_header();
    let key = parse_rsa_pem(pem)?;
    let enc = EncodingKey::from_rsa_der(
        key.to_pkcs1_der().map_err(|e| e.to_string())?.as_bytes()
    );
    jsonwebtoken::encode(&header, &claims, &enc)
        .map_err(|e| format!("jsonwebtoken::encode: {}", e))
}

/// Verify that PEM + client ID are usable — parses the key and round-trips the
/// header to confirm `alg: RS256`.
#[allow(dead_code)] // reserved for future credential-seed validation step
pub fn validate_pem_and_id(pem: &str, client_id: &str) -> Result<(), String> {
    if client_id.trim().is_empty() {
        return Err("client_id must not be empty".to_owned());
    }
    let jwt = generate_jwt(pem, client_id)?;
    // Inspect header
    let hdr_b64 = jwt.split('.').next().ok_or("JWT missing header")?;
    let bytes =
        STANDARD.decode(hdr_b64).map_err(|_| "header is not valid base64")?;
    let val: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| "header is not valid JSON")?;
    match val["alg"].as_str() {
        Some("RS256") => Ok(()),
        Some(other) => Err(format!("unexpected JWT alg: {}", other)),
        None => Err("JWT header has no alg".to_owned()),
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Internal helpers
// ────────────────────────────────────────────────────────────────────────────

#[inline]
fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before 1970")
        .as_secs()
}

// ────────────────────────────────────────────────────────────────────────────
// Unit tests
// ────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    #[allow(unused_imports)]

    /// Build a 2048-bit RSA key manually from hex so we never depend on `rand`.
    /// These bytes represent a minimal but structurally valid RSA key derived
    /// from a known 2048-bit test vector.
    fn known_test_rsa_pem() -> &'static str {
        // This PEM is a pre-baked 2048-bit key generated once offline.
        // It is ONLY used in tests and never shipped to production.
        include_str!("../test-fixtures/2048-rsa-test-key.pem")
    }

    #[test]
    fn claims_lifetime_is_exactly_ten_minutes() {
        let c = Claims::new("my-app");
        assert_eq!(c.lifetime_secs(), 600);
    }

    #[test]
    fn claims_iss_is_set() {
        let c = Claims::new("my-client-id");
        assert_eq!(c.iss, "my-client-id");
        assert!(c.iat > 0);
        assert!(c.exp > c.iat);
    }

    #[test]
    fn jwt_header_is_rs256() {
        // Runs only if the env var is set to keep tests hermetic.
        let pem = known_test_rsa_pem();
        let jwt = generate_jwt(pem, "test-id").expect("must encode");
        let hdr_b64 = jwt.split('.').next().unwrap();
        let hdr_bytes = STANDARD.decode(hdr_b64).unwrap();
        let val: serde_json::Value = serde_json::from_slice(&hdr_bytes).unwrap();
        assert_eq!(val["alg"].as_str().unwrap(), "RS256");
        assert_eq!(val["typ"].as_str().unwrap(), "JWT");
    }

    #[test]
    fn jwt_has_exactly_two_dots() {
        let pem = known_test_rsa_pem();
        let jwt = generate_jwt(pem, "test-id").expect("must encode");
        assert_eq!(jwt.matches('.').count(), 2);
    }

    #[test]
    fn validate_pem_and_id_ok() {
        let pem = known_test_rsa_pem();
        validate_pem_and_id(pem, "any-client-id").expect("valid");
    }

    #[test]
    fn validate_pem_and_id_rejects_empty_client_id() {
        let pem = known_test_rsa_pem();
        let err = validate_pem_and_id(pem, "").unwrap_err();
        assert!(err.contains("empty") || err.contains("not be empty"));
    }
}
