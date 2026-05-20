use serde::{Deserialize, Serialize};
use tauri::command;
use crate::credentials;
use crate::installation;


#[derive(Debug, Serialize, Deserialize)]
pub struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    interval: i64,
    verification_uri: String,
    expires_in: i64,
}

#[derive(Debug, Serialize, Deserialize)]
struct DeviceCodeError {
    error: String,
    error_description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TokenResponse {
    access_token: Option<String>,
    token_type: Option<String>,
    scope: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UserInfo {
    id: i64,
    login: String,
    avatar_url: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RepoInfo {
    id: i64,
    full_name: String,
    name: String,
    private: bool,
    owner: OwnerInfo,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OwnerInfo {
    login: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PRInfo {
    id: i64,
    number: i64,
    title: String,
    state: String,
    author: AuthorInfo,
    created_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuthorInfo {
    login: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IssueInfo {
    id: i64,
    number: i64,
    title: String,
    state: String,
    author: AuthorInfo,
    created_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BranchInfo {
    name: String,
    commit: CommitInfo,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CommitInfo {
    sha: String,
}

#[command]
pub async fn get_device_code() -> Result<DeviceCodeResponse, String> {
    let client_id = std::env::var("GH_CLIENT_ID").unwrap_or_default();

    if client_id.is_empty() {
        eprintln!("[get_device_code] GH_CLIENT_ID is empty");
        return Err("GH_CLIENT_ID must be set in .env".to_string());
    }

    eprintln!("[get_device_code] Requesting device code for client_id={}", client_id);
    let response = reqwest::Client::new()
        .post("https://github.com/login/device/code")
        .header("Accept", "application/json")
        .form(&[("client_id", client_id.as_str())])
        .send()
        .await
        .map_err(|e| {
            eprintln!("[get_device_code] network error: {}", e);
            format!("Failed to get device code (network error): {}", e)
        })?;

    let status = response.status();
    let body_text = response.text().await.unwrap_or_default();
    eprintln!("[get_device_code] HTTP {} body={}", status, body_text);

    if !status.is_success() {
        if let Ok(err) = serde_json::from_str::<DeviceCodeError>(&body_text) {
            eprintln!("[get_device_code] GitHub API error: {} - {:?}", err.error, err.error_description);
            match err.error.as_str() {
                "device_flow_disabled" => {
                    return Err("Device flow is not enabled for this GitHub App. Go to https://github.com/settings/apps, select your app, and enable 'Device flow' in Beta features.".to_string());
                }
                "incorrect_client_credentials" => {
                    return Err("Invalid GH_CLIENT_ID. Check your credentials in .env".to_string());
                }
                _ => {
                    return Err(format!("GitHub error: {} - {:?}", err.error, err.error_description));
                }
            }
        }
        return Err(format!("GitHub API error (HTTP {}): {}", status, body_text));
    }

    let code: DeviceCodeResponse = serde_json::from_str(&body_text)
        .map_err(|e| {
            eprintln!("[get_device_code] parse error: {}", e);
            format!("Failed to parse device code response: {}", e)
        })?;

    eprintln!("[get_device_code] Got user_code={} verification_uri={} expires_in={} device_code_len={}",
        code.user_code, code.verification_uri, code.expires_in, code.device_code.len());

    Ok(code)
}


#[command]
pub async fn poll_token(device_code: String) -> Result<TokenResponse, String> {
    let client_id = std::env::var("GH_CLIENT_ID").unwrap_or_default();

    if client_id.is_empty() {
        eprintln!("[poll_token] GH_CLIENT_ID is empty");
        return Err("GH_CLIENT_ID must be set in .env".to_string());
    }

    eprintln!("[poll_token] polling with device_code={} client_id={} client_secret_set={}",
        device_code, client_id,
        std::env::var("GH_CLIENT_SECRET").is_ok());

    let client = reqwest::Client::new();
    let mut interval_secs: u64 = 5; // GitHub default; updated from response
    let mut attempt = 0u32;

    loop {
        attempt += 1;
        // Per RFC 8628 §3.5: wait at least `interval` seconds between polls.
        tokio::time::sleep(std::time::Duration::from_secs(interval_secs)).await;

        eprintln!("[poll_token] attempt={} interval={}s", attempt, interval_secs);

        let response = client
            .post("https://github.com/login/oauth/access_token")
            .header("Accept", "application/x-www-form-urlencoded")
            .form(&[
                ("client_id", client_id.as_str()),
                ("device_code", device_code.as_str()),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ])
            .send()
            .await
            .map_err(|e| {
                eprintln!("[poll_token] network error attempt={}: {}", attempt, e);
                format!("Polling failed (network error): {}", e)
            })?;

        let status = response.status();
        let body_text = response
            .text()
            .await
            .unwrap_or_else(|_| "<body read error>".to_string());
        eprintln!("[poll_token] HTTP {} body={}", status, body_text);

        // --- Hard errors (stop polling immediately) ---
        if status == 400 || status == 401 {
            if let Some(token_err) = parse_urlencoded_field(&body_text, "error") {
                eprintln!("[poll_token] error from GitHub: {}", token_err);
                match token_err.as_str() {
                    "authorization_pending" => {
                        // User hasn't entered code yet — keep polling at current interval.
                        continue;
                    }
                    "slow_down" => {
                        // Slow down: GitHub demands +5 s between polls.
                        interval_secs = interval_secs.saturating_add(5).max(5);
                        // GitHub may also include an updated `interval` field.
                        if let Some(updated) = body_text
                            .split('&')
                            .find(|p| p.starts_with("interval="))
                            .and_then(|p| p.split('=').nth(1))
                            .and_then(|v| v.parse::<u64>().ok())
                        {
                            interval_secs = interval_secs.max(updated);
                        }
                        continue;
                    }
                    "expired_token" => {
                        return Err(
                            "Device code expired. Please Login with GitHub again.".to_string()
                        );
                    }
                    "access_denied" => {
                        return Err("Access was denied by the user.".to_string());
                    }
                    "incorrect_client_credentials" => {
                        return Err("Incorrect client credentials. Check GH_CLIENT_ID / GH_CLIENT_SECRET in .env.".to_string());
                    }
                    "device_flow_disabled" => {
                        return Err("Device flow is not enabled for this GitHub App. Enable it in the app settings.".to_string());
                    }
                    "unsupported_grant_type" => {
                        return Err("Unsupported grant type.".to_string());
                    }
                    "incorrect_device_code" => {
                        return Err("Incorrect device code: request a new login.".to_string());
                    }
                    other => {
                        return Err(format!("Device flow API error: {}", other));
                    }
                }
            }
            // 401 without a known error → unrecoverable
            return Err(format!(
                "Authentication rejected (HTTP {}): {}",
                status, body_text
            ));
        }

        // --- x-www-form-urlencoded body: owned token response ---
        let token = parse_urlencoded_field(&body_text, "access_token").ok_or_else(|| {
            format!(
                "Missing access_token in response (HTTP {}, body: {})",
                status, body_text
            )
        })?;

        let token_type = parse_urlencoded_field(&body_text, "token_type");
        let scope = parse_urlencoded_field(&body_text, "scope");

        return Ok(TokenResponse {
            access_token: Some(token),
            token_type,
            scope,
        });
    }
}

#[command]
#[tracing::instrument(skip(session_id))]
pub async fn get_repos(session_id: String) -> Result<Vec<RepoInfo>, String> {
    let token = get_token(session_id).await?;
    let client = reqwest::Client::new();

    let response = client
        .get("https://api.github.com/user/repos")
        .header("Authorization", format!("Bearer {}", token))
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await
        .map_err(|e| format!("Failed to get repos: {}", e))?;

    let repos: Vec<RepoInfo> = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse repos: {}", e))?;

    Ok(repos)
}

#[command]
#[tracing::instrument(skip(session_id, owner, repo))]
pub async fn get_prs(
    session_id: String,
    owner: String,
    repo: String,
) -> Result<Vec<PRInfo>, String> {
    let token = get_token(session_id).await?;
    let client = reqwest::Client::new();

    let response = client
        .get(format!("https://api.github.com/repos/{}/{}", owner, repo))
        .query(&[("state", "open")])
        .header("Authorization", format!("Bearer {}", token))
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await
        .map_err(|e| format!("Failed to get PRs: {}", e))?;

    let prs: Vec<PRInfo> = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse PRs: {}", e))?;

    Ok(prs)
}

#[command]
#[tracing::instrument(skip(session_id, owner, repo))]
pub async fn get_issues(
    session_id: String,
    owner: String,
    repo: String,
) -> Result<Vec<IssueInfo>, String> {
    let token = get_token(session_id).await?;
    let client = reqwest::Client::new();

    let response = client
        .get(format!("https://api.github.com/repos/{}/{}", owner, repo))
        .query(&[("state", "open")])
        .header("Authorization", format!("Bearer {}", token))
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await
        .map_err(|e| format!("Failed to get issues: {}", e))?;

    let issues: Vec<IssueInfo> = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse issues: {}", e))?;

    Ok(issues)
}

#[command]
#[tracing::instrument(skip(session_id, owner, repo))]
pub async fn get_branches(
    session_id: String,
    owner: String,
    repo: String,
) -> Result<Vec<BranchInfo>, String> {
    let token = get_token(session_id).await?;
    let client = reqwest::Client::new();

    let response = client
        .get(format!("https://api.github.com/repos/{}/{}", owner, repo))
        .header("Authorization", format!("Bearer {}", token))
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await
        .map_err(|e| format!("Failed to get branches: {}", e))?;

    let branches: Vec<BranchInfo> = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse branches: {}", e))?;

    Ok(branches)
}

#[command]
pub async fn whoami(session_id: String) -> Result<UserInfo, String> {
    let token = get_token(session_id).await?;
    let client = reqwest::Client::new();

    let response = client
        .get("https://api.github.com/user")
        .header("Authorization", format!("Bearer {}", token))
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await
        .map_err(|e| format!("Failed to get user: {}", e))?;

    let user: UserInfo = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse user: {}", e))?;

    Ok(user)
}

#[command]
#[tracing::instrument(skip(message))]
pub async fn chat(session_id: String, message: String) -> Result<String, String> {
    use std::sync::Arc;
    use crate::agent::adapter::OllamaAdapter;
    use crate::agent::runtime::loop_control::AgentExecutor;
    use crate::agent::tools::ToolRegistry;
    use crate::agent::memory::AgentSession;
    use crate::agent::chain_format::spec::{ChainSpec, Role, Message};
    use crate::agent::trace::default_sinks;

    let adapter = Arc::new(OllamaAdapter::default());
    let registry = ToolRegistry::default();
    let executor = AgentExecutor::new(adapter, registry);

    let mut session = AgentSession::new(&session_id, "hiem_engineering");
    session.messages = vec![
        Message { role: Role::System, content: r#"You are HIEM's engineering agent. Use [TOOL: name]\n{json} format for tool calls. Be concise."#.to_string() },
        Message { role: Role::User, content: message.clone() },
    ];

    let spec = ChainSpec {
        chain_type: crate::agent::chain_format::spec::ChainType::Sequential,
        name: "hiem_engineering".to_string(),
        description: Some("HIEM engineering agent".to_string()),
        model: "ollama/llama3.2:3b".to_string(),
        temperature: 0.2,
        max_tokens: Some(4096),
        loop_budget: 6,
        token_budget: 4096,
        steps: vec![],
        tools: vec![],
        nodes: vec![],
        edges: vec![],
    };

    let sinks = default_sinks();

    executor.run(&spec, session, &sinks)
        .await
        .map_err(|e| format!("Agent execution failed: {}", e))
}

/// Check LangSmith tracing status
#[command]
pub fn langsmith_status() -> serde_json::Value {
    let api_key = std::env::var("LANGSMITH_API_KEY").ok();
    let project = std::env::var("LANGSMITH_PROJECT").ok();
    let endpoint = std::env::var("LANGSMITH_ENDPOINT").ok();
    let trace = std::env::var("HIEM_TRACE").unwrap_or_else(|_| "file".to_string());

    serde_json::json!({
        "enabled": api_key.is_some() && trace.contains("langsmith"),
        "api_key_set": api_key.is_some(),
        "project": project.unwrap_or_else(|| "hiem-app".to_string()),
        "endpoint": endpoint.unwrap_or_else(|| "https://api.smith.langchain.com".to_string()),
        "mode": trace,
    })
}

/// Retrieve the GitHub token from `gh` CLI credentials.
///
/// Sources checked in order:
/// 1. `GH_TOKEN` or `GITHUB_TOKEN` environment variable
/// 2. `gh auth token` command (reads from keyring or hosts.yml)
/// 3. `~/.config/gh/hosts.yml` (fallback for older gh versions)
#[command]
pub async fn get_gh_token() -> Result<TokenResponse, String> {
    // 1. Check environment variables first
    if let Ok(token) = std::env::var("GH_TOKEN") {
        if !token.is_empty() {
            return Ok(TokenResponse {
                access_token: Some(token),
                token_type: Some("bearer".to_string()),
                scope: None,
            });
        }
    }
    if let Ok(token) = std::env::var("GITHUB_TOKEN") {
        if !token.is_empty() {
            return Ok(TokenResponse {
                access_token: Some(token),
                token_type: Some("bearer".to_string()),
                scope: None,
            });
        }
    }

    // 2. Try `gh auth token` command
    let output = std::process::Command::new("gh")
        .args(["auth", "token"])
        .output()
        .map_err(|e| format!("Failed to run 'gh auth token': {}", e))?;

    if output.status.success() {
        let token = String::from_utf8_lossy(&output.stdout)
            .trim()
            .to_string();
        if !token.is_empty() && (token.starts_with("gho_") || token.starts_with("ghp_")) {
            return Ok(TokenResponse {
                access_token: Some(token),
                token_type: Some("bearer".to_string()),
                scope: None,
            });
        }
    }

    // 3. Fallback: read from ~/.config/gh/hosts.yml
    let hosts_path = dirs::home_dir()
        .ok_or_else(|| "Could not find home directory".to_string())?
        .join(".config/gh/hosts.yml");

    if hosts_path.exists() {
        let content = std::fs::read_to_string(&hosts_path)
            .map_err(|e| format!("Failed to read gh hosts file: {}", e))?;

        // Simple YAML parsing for oauth_token
        for line in content.lines() {
            if line.contains("oauth_token:") {
                let token = line
                    .split("oauth_token:")
                    .nth(1)
                    .map(|s| s.trim().trim_matches('"'))
                    .unwrap_or("")
                    .to_string();
                if !token.is_empty() {
                    return Ok(TokenResponse {
                        access_token: Some(token),
                        token_type: Some("bearer".to_string()),
                        scope: None,
                    });
                }
            }
        }
    }

    Err("No GitHub token found. Please run 'gh auth login' or set GH_TOKEN/GITHUB_TOKEN environment variable.".to_string())
}

/// Opens a URL in the user's system browser using `xdg-open`.
/// This wraps `window.open` which does not work inside a Tauri webview.
#[command]
pub async fn open_url(url: String) -> Result<(), String> {
    std::process::Command::new("xdg-open")
        .arg(&url)
        .spawn()
        .map_err(|e| format!("Failed to open browser: {}", e))?;
    Ok(())
}

/// Copies text to the system clipboard.
/// Uses `wl-copy` on Wayland, `xclip` on X11.  The text is sent via
/// stdin so it never appears in the process list.  Runs the child in a
/// blocking pool thread so the tokio async runtime is never stalled.
#[command]
pub async fn copy_to_clipboard(text: String) -> Result<(), String> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    // Try wl-copy first (Wayland), fall back to xclip (X11).
    let child = Command::new("wl-copy")
        .stdin(Stdio::piped())
        .spawn();

    let mut child = match child {
        Ok(c) => c,
        Err(_) => Command::new("xclip")
            .args(["-selection", "clipboard"])
            .stdin(Stdio::piped())
            .spawn()
            .map_err(|e| format!("No clipboard tool found (tried wl-copy, xclip): {}", e))?,
    };

    {
        let mut stdin = child.stdin.take().expect("child stdin");
        stdin
            .write_all(text.as_bytes())
            .map_err(|e| format!("Failed to write to clipboard: {}", e))?;
    }

    let status = child
        .wait()
        .map_err(|e| format!("clipboard process failed: {}", e))?;
    if !status.success() {
        return Err("clipboard command returned non-zero".to_string());
    }

    Ok(())
}

/// Import the GitHub App's RSA private key into the OS keyring.
/// Keyring service: `com.hiem.app`, entry: `github-app-pem`.
#[command]
pub fn store_github_app_key(path: String) -> Result<(), String> {
    let pem = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {}: {}", path, e))?;
    if !pem.contains("-----") {
        return Err("file does not look like a PEM private key".to_owned());
    }
    credentials::store_pem(&pem)
        .map_err(|e| format!("keyring write error: {}", e))
}

/// Return which hiem keyring entries exist (bearer-token purpose).
#[command]
pub fn list_keyring_entries() -> Result<Vec<String>, String> {
    let mut list = Vec::new();
    if credentials::has_keyring_entry() {
        list.push("hiem:github-app-pem".to_owned());
    }
    Ok(list)
}

/// Authenticate to GitHub using the stored GitHub App private key —
/// get an installation token, then call `GET /user`.
#[command]
pub async fn whoami_with_installation_token(
    installation_id: u64,
) -> Result<serde_json::Value, String> {
    use installation::request_installation_token;

    let token_data = request_installation_token(installation_id, None)
        .await
        .map_err(|e| {
            eprintln!("[whoami_with_installation_token] {}", e);
            e
        })?;

    // Resolve the login name so we can return a stable session handle
    let login = {
        let client = reqwest::Client::new();
        let resp = client
            .get("https://api.github.com/user")
            .bearer_auth(&token_data.value)
            .header("Accept", "application/vnd.github.v3+json")
            .header("X-GitHub-Api-Version", "2026-03-10")
            .send()
            .await
            .map_err(|e| format!("user API request failed: {}", e))?;
        #[derive(serde::Deserialize)]
        struct Whoami { login: String }
        resp.json::<Whoami>().await.map_err(|e| e.to_string())?.login
    };

    // ── Persist the raw `ghs_…` token in the OS keyring ────────────────────────
    // Keyed by the GitHub login so get_token() can look it up by the
    // `ghs_session_{login}` handle the frontend will receive.
    use crate::credentials;
    let _ = credentials::store_ghs_token(&login, &token_data.value);

    // ── Return a short stable handle to the frontend ───────────────────────────
    // Frontend stores "ghs_session_{login}" in localStorage.
    // Every subsequent Tauri command resolves this handle back to the real
    // ghs_… token via the OS keyring in get_token().
    let handle = format!("ghs_session_{}", login);

    Ok(serde_json::json!({
        "status":        200,
        "handle":        handle,
        "login":         login,
        "expires_at_unix": token_data.expires_at_unix,
    }))
}

/// Resolve a session ID to an actual GitHub bearer token.
///
/// OAuth (device-flow) tokens begin with `gho_` and are stored as-is.
///
/// GitHub App tokens handle a `ghs_session_{login}` handle.  The real `ghs_`
/// token is retrieved from the OS keyring and returned instead.  This allows
/// the frontend (which doesn't know the `ghs_…` token) to pass around short
/// handles that resolve transparently on every API call.
async fn get_token(session_id: String) -> Result<String, String> {
    if session_id.trim().is_empty() {
        return Err("No session ID (GitHub token) provided".to_string());
    }

    // --- OAuth tokens start with gho_, ghp_  ---
    if session_id.starts_with("gho_") || session_id.starts_with("ghp_") {
        return Ok(session_id);
    }

    // --- GitHub App installation-token handle: ghs_session_{login} ---
    if let Some(login) = session_id.strip_prefix("ghs_session_") {
        let token = match std::env::var("GH_APP_INST_TOKEN") {
            Ok(t) if !t.is_empty() => t,
            _ => {
                use crate::credentials;
                credentials::load_ghs_token(login)
                    .map_err(|e| format!("keyring error: {}", e))?
                    .ok_or_else(|| {
                        format!(
                            "No cached installation token for GitHub user '{}'. \
                             Re-authenticate in the app.",
                            login
                        )
                    })?
            }
        };
        return Ok(token);
    }

    // --- Fallback: pass through ---
    Ok(session_id)
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Parse a single key from a URL-encoded string such as
/// "access_token=gho_abc&token_type=bearer"
/// Handles "key=<value>" pairs. Tags value at next `&` or end of string.
/// Does **not** attempt percent-decoding (token values are always plain ASCII
/// from GitHub's device-flow responses).
fn parse_urlencoded_field(body: &str, key: &str) -> Option<String> {
    let needle = format!("{}=", key);
    let mut start = 0usize;
    loop {
        match body[start..].find(&needle) {
            None => return None,
            Some(idx) => {
                let value_start = start + idx + needle.len();
                let value_end = body[value_start..]
                    .find('&')
                    .map(|i| value_start + i)
                    .unwrap_or_else(|| body.len());
                if value_end >= value_start {
                    // Return the raw value; GitHub's device flow does not percent-
                    // encode token values, so no decoding is needed.
                    return Some(body[value_start..value_end].to_string());
                }
                start = value_end + 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_urlencoded_field() {
        assert_eq!(
            parse_urlencoded_field(
                "access_token=gho_abc%20def&token_type=bearer",
                "access_token"
            )
            .as_deref(),
            Some("gho_abc%20def")
        );
        assert_eq!(
            parse_urlencoded_field("token_type=bearer&access_token=xyz123", "access_token")
                .as_deref(),
            Some("xyz123")
        );
        assert!(parse_urlencoded_field("error=foo", "access_token").is_none());
        assert!(!parse_urlencoded_field("", "access_token").is_some());
    }

    #[test]
    fn test_device_code_response_parses_expires_in() {
        // Simulates the JSON returned by POST /login/device/code
        let body = serde_json::json!({
            "device_code": "d42d3b73b2a77761d79c978fa8f3a455c5dfa164",
            "user_code": "BF49-B03A",
            "interval": 5,
            "verification_uri": "https://github.com/login/device",
            "expires_in": 900
        })
        .to_string();

        let parsed: DeviceCodeResponse = serde_json::from_str(&body).expect("must parse");
        assert_eq!(parsed.user_code, "BF49-B03A");
        assert_eq!(parsed.verification_uri, "https://github.com/login/device");
        assert_eq!(parsed.interval, 5);
        assert_eq!(parsed.expires_in, 900);
        assert_eq!(parsed.device_code.len(), 40);
    }

    #[test]
    fn test_device_code_error_parses() {
        // Simulates a device_flow_disabled error from POST /login/device/code
        let body = serde_json::json!({
            "error": "device_flow_disabled",
            "error_description": "Device flow has not been enabled in the app's settings."
        })
        .to_string();

        let parsed: DeviceCodeError = serde_json::from_str(&body).expect("must parse");
        assert_eq!(parsed.error, "device_flow_disabled");
        assert!(parsed.error_description.is_some());
    }
}
