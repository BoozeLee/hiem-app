import { invoke } from '@tauri-apps/api/core';

// ── GitHub App credential store (keyring) ─────────────────────────────────────

export interface KeyringEntry {
  label: string;
}

/** Check whether a GitHub App private key is stored in the OS keyring. */
export async function storeGithubAppKey(path: string): Promise<void> {
  await invoke('store_github_app_key', { path });
}

/** Returns `true` if a GitHub App private key exists in the OS keyring. */
export async function hasKeyringEntry(): Promise<boolean> {
  const list = (await invoke('list_keyring_entries', {})) as string[];
  return list.some((l) => l.startsWith('hiem:'));
}

// ── OAuth device flow ─────────────────────────────────────────────────────────

export async function getDeviceCode(): Promise<{
  device_code: string;
  user_code: string;
  interval: number;
  verification_uri: string;
  expires_in: number;
}> {
  return invoke('get_device_code', {}) as Promise<{
    device_code: string;
    user_code: string;
    interval: number;
    verification_uri: string;
    expires_in: number;
  }>;
}

export async function pollToken(
  deviceCode: string,
): Promise<{
  access_token: string | null;
  token_type: string | null;
  scope: string | null;
  error?: string | null;
}> {
  return invoke('poll_token', { device_code: deviceCode }) as Promise<{
    access_token: string | null;
    token_type: string | null;
    scope: string | null;
    error?: string | null;
  }>;
}

// ── GitHub API (bearer token) ─────────────────────────────────────────────────

export interface UserInfo {
  id: number;
  login: string;
  avatar_url: string;
}

export async function whoami(sessionId: string): Promise<UserInfo> {
  return invoke('whoami', { session_id: sessionId }) as Promise<UserInfo>;
}

export interface RepoInfo {
  id: number;
  full_name: string;
  name: string;
  private: boolean;
  owner: { login: string };
}

export async function getRepos(
  sessionId: string,
): Promise<RepoInfo[]> {
  return invoke('get_repos', { session_id: sessionId }) as Promise<RepoInfo[]>;
}

export interface PRInfo {
  id: number;
  number: number;
  title: string;
  state: string;
  author: { login: string };
  created_at: string;
}

export async function getPRs(
  sessionId: string,
  owner: string,
  repo: string,
): Promise<PRInfo[]> {
  return invoke('get_prs', { session_id: sessionId, owner, repo }) as Promise<PRInfo[]>;
}

export interface IssueInfo {
  id: number;
  number: number;
  title: string;
  state: string;
  author: { login: string };
  created_at: string;
}

export async function getIssues(
  sessionId: string,
  owner: string,
  repo: string,
): Promise<IssueInfo[]> {
  return invoke('get_issues', { session_id: sessionId, owner, repo }) as Promise<IssueInfo[]>;
}

export interface BranchInfo {
  name: string;
  commit: { sha: string };
}

export async function getBranches(
  sessionId: string,
  owner: string,
  repo: string,
): Promise<BranchInfo[]> {
  return invoke('get_branches', { session_id: sessionId, owner, repo }) as Promise<BranchInfo[]>;
}

// ── GitHub App — installation token via keyring ───────────────────────────────

/**
 * Use the stored GitHub App private key (keyring) to exchange a JWT for a
 * fresh `ghs_…` installation token, then verify by calling `GET /user`.
 *
 * Returns `{ loggedInAs, status }` on success.
 */
export async function signInWithInstallationToken(
  installationId: number,
): Promise<{ status: number; loggedInAs: string; login: string; avatarUrl: string }> {
  const result = (await invoke('whoami_with_installation_token', {
    installation_id: installationId,
  })) as {
    status: number;
    handle: string;
    login: string;
    expires_at_unix: number;
  };

  return {
    status: result.status,
    loggedInAs: result.handle,
    login: result.login,
    avatarUrl: '',
  };
}

// ── Chat ──────────────────────────────────────────────────────────────────────

export async function chat(sessionId: string, message: string): Promise<string> {
  return invoke('chat', { session_id: sessionId, message }) as Promise<string>;
}

// ── Browser / clipboard ───────────────────────────────────────────────────────

export async function openUrl(url: string): Promise<void> {
  await invoke('open_url', { url });
}

export async function copyToClipboard(text: string): Promise<void> {
  await invoke('copy_to_clipboard', { text });
}
