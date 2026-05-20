#!/usr/bin/env python3
"""
HIEM GitHub App registration helper.

Usage:
  python3 scripts/register_github_app.py [--client-id]

Prints the manifest registration URL and -- if run interactively with a GitHub
access token available in $GITHUB_TOKEN or $(gh auth token) -- opens the URL
in a browser and prints the credentials to save in .env.

To get credentials without opening a browser, use the two-step flow:
  python3 scripts/register_github_app.py  # prints step-1 URL
  # open that URL in your browser, approve, GitHub redirects with a ?code=
  python3 scripts/register_github_app.py --code <CODE_FROM_REDIRECT>  # prints credentials
"""

import json
import os
import sys
import urllib.parse
import urllib.request
import subprocess
import webbrowser


MANIFEST = {
    "name": "HIEM",
    "url": "https://github.com/BoozeLee/hiem-app",
    "description": "HIEM — GitHub-authenticated chat with engineering agent",
    "request_oauth_on_install": True,
    "public": False,
    "default_permissions": {
        "contents": "read",
        "issues": "read",
        "pull_requests": "read",
        "metadata": "read",
        "members": "read",
    },
    "default_events": ["push", "pull_request", "issues", "issue_comment"],
}


def get_manifest_url() -> str:
    encoded = urllib.parse.quote(json.dumps(MANIFEST), safe="")
    return f"https://github.com/apps/new?manifest={encoded}"


def get_gh_token() -> str | None:
    # Try env, then gh cli
    token = os.environ.get("GITHUB_TOKEN") or os.environ.get("GH_TOKEN")
    if token:
        return token
    try:
        result = subprocess.run(
            ["gh", "auth", "token"], capture_output=True, text=True, check=True
        )
        return result.stdout.strip()
    except (subprocess.CalledProcessError, FileNotFoundError):
        return None


def exchange_code(code: str) -> dict:
    """POST /app-manifests/{code}/conversions → {client_id, client_secret, pem, ...}"""
    url = f"https://api.github.com/app-manifests/{urllib.parse.quote(code)}/conversions"
    req = urllib.request.Request(url, method="POST")
    req.add_header("Accept", "application/vnd.github+json")
    req.add_header("X-GitHub-Api-Version", "2022-11-28")

    token = get_gh_token()
    if token:
        req.add_header("Authorization", f"Bearer {token}")

    with urllib.request.urlopen(req) as resp:
        return json.loads(resp.read())


def main() -> None:
    if "--code" in sys.argv:
        idx = sys.argv.index("--code") + 1
        code = sys.argv[idx]
        print("Exchanging manifest code for credentials…")
        creds = exchange_code(code)
        print()
        print("=== HIEM GitHub App Credentials ===")
        print(f"Client ID:     {creds['client_id']}")
        print(f"Client Secret: {creds['client_secret']}")
        print(f"App Name:      {creds['name']}")
        print(f"HTML URL:      {creds['html_url']}")
        print()
        opts = creds.get("options", {})
        print(f"Is Public:     {opts.get('public', False)}")
        print(f"OAuth on Install: {opts.get('request_oauth_on_install', False)}")
        print(f"Device Flow:   {opts.get('allow_device_flow', False)}")
        print()
        print("→ Put Client ID and Client Secret into hiem-app/.env as:")
        print("  GH_CLIENT_ID=<above>")
        print("  GH_CLIENT_SECRET=<above>")
        return

    url = get_manifest_url()
    print("HIEM GitHub App Registration")
    print("=" * 44)
    print()
    print("Step 1 — Click this URL to create the GitHub App:")
    print(f"  {url}")
    print()
    print("Step 2 — In your browser:")
    print("  - Complete the registration form (pre-filled by the manifest)")
    print("  - Click 'Create GitHub App'")
    print()
    print("  GitHub will then redirect to a URL like:")
    print("  https://github.com/login/device?code=XXXXXXXXXX")
    print()
    print("Step 3 — Copy the ?code=… value from the URL bar, then run:")
    print("  scripts/register_github_app.py --code <CODE>")
    print()
    print("Step 4 — Put GH_CLIENT_ID / GH_CLIENT_SECRET into hiem-app/.env")
    print()

    # Try to auto-open the URL in browser
    token = get_gh_token()
    if token:
        webbrowser.open(url)
        print(f"(Opened in browser using gh auth token for {os.environ.get('GH_HOST', 'github.com')})")
    else:
        print("(No GitHub token found — open the URL manually in your browser)")


if __name__ == "__main__":
    main()
