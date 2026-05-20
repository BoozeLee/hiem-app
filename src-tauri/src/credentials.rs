//! Secure credential storage for the HIEM GitHub App private key and cached
//! installation tokens.
//!
//! Keyring layout:
//!   Service                Username              Secret
//!   ─────────────────────  ────────────────────  ────────────
//!   `com.hiem.app`         `github-app-pem`       RSA PEM key
//!   `com.hiem.app`         `ghs_token:{login}`   ghs_ token (per login)

// ────────────────────────────────────────────────────────────────────────────
// PEM (private key)
// ────────────────────────────────────────────────────────────────────────────

/// Load the GitHub App RSA private key PEM from the OS keyring.
pub fn load_pem() -> Result<Option<String>, String> {
    let entry = keyring::Entry::new(SERVICE, KEYRING_PEM_USER);
    match entry {
        Ok(entry) => match entry.get_password() {
            Ok(pem) => Ok(Some(pem)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(format!("keyring read: {}", e)),
        },
        Err(e) => Err(format!("keyring open: {}", e)),
    }
}

/// Store / overwrite the GitHub App RSA private key PEM in the OS keyring.
pub fn store_pem(pem: &str) -> Result<(), String> {
    let entry = keyring::Entry::new(SERVICE, KEYRING_PEM_USER)
        .map_err(|e| format!("keyring open: {}", e))?;
    entry.set_password(pem).map_err(|e| format!("keyring write: {}", e))
}

pub fn delete_pem() -> Result<(), String> {
    let entry = keyring::Entry::new(SERVICE, KEYRING_PEM_USER)
        .map_err(|e| format!("keyring open: {}", e))?;
    entry.delete_credential().map_err(|e| format!("keyring delete: {}", e))
}

// ────────────────────────────────────────────────────────────────────────────
// ghs_ installation tokens (per GitHub login)
// ────────────────────────────────────────────────────────────────────────────

/// Store a `ghs_…` installation token for a specific GitHub login.
pub fn store_ghs_token(login: &str, token: &str) -> Result<(), String> {
    let username = format!("{}:{}", KEYRING_GHS_PREFIX, login);
    let entry = keyring::Entry::new(SERVICE, &username)
        .map_err(|e| format!("keyring open: {}", e))?;
    entry.set_password(token).map_err(|e| format!("keyring write: {}", e))
}

/// Load the `ghs_…` installation token for a specific GitHub login.
pub fn load_ghs_token(login: &str) -> Result<Option<String>, String> {
    let username = format!("{}:{}", KEYRING_GHS_PREFIX, login);
    let entry = match keyring::Entry::new(SERVICE, &username) {
        Ok(e) => e,
        Err(e) => return Err(format!("keyring open: {}", e)),
    };
    match entry.get_password() {
        Ok(token) => Ok(Some(token)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("keyring read: {}", e)),
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Helpers
// ────────────────────────────────────────────────────────────────────────────

pub fn has_keyring_entry() -> bool {
    load_pem().ok().flatten().is_some()
}

const SERVICE: &str = "com.hiem.app";
const KEYRING_PEM_USER: &str = "github-app-pem";
const KEYRING_GHS_PREFIX: &str = "ghs_token";
