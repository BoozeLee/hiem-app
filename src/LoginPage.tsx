import { useState, useEffect, useRef } from "react";
import {
  getDeviceCode,
  pollToken,
  storeGithubAppKey,
  hasKeyringEntry,
  signInWithInstallationToken,
  copyToClipboard,
} from "./tauri-api";

// ─────────────────────────────────────────────────────────────────────────────

type AuthMode = "oauth" | "github-app";

export default function LoginPage({
  onLogin,
}: {
  onLogin: (id: string) => void;
}) {
  const [mode, setMode] = useState<AuthMode>("oauth");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [status, setStatus] = useState("");

  // ── OAuth state ────────────────────────────────────────────────────────────
  const [userCode, setUserCode] = useState("");
  const [verificationUri, setVerificationUri] = useState("");
  const [expiresIn, setExpiresIn] = useState(0);
  const pollTimerRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const expiryTimerRef = useRef<ReturnType<typeof setInterval> | null>(null);

  // ── GitHub App (keyring) state ─────────────────────────────────────────────
  const [pemPath, setPemPath] = useState("");
  const [instId, setInstId] = useState("");
  const [keyringHasKey, setKeyringHasKey] = useState(false);
  const clipboardAttempts = useRef(0);

  // ─────────────────────────────────────────────────────────────────────────
  // Countdown timer
  // ─────────────────────────────────────────────────────────────────────────
  useEffect(() => {
    if (expiresIn <= 0) return;
    const t = setInterval(() => setExpiresIn((p) => Math.max(0, p - 1)), 1000);
    return () => clearInterval(t);
  }, [expiresIn]);

  const formatTime = (s: number) => {
    const m = Math.floor(s / 60);
    const sec = s % 60;
    return `${m}:${sec.toString().padStart(2, "0")}`;
  };

  // ─────────────────────────────────────────────────────────────────────────
  // Check keyring on mount / when switching to GitHub App mode
  // ─────────────────────────────────────────────────────────────────────────
  const checkKeyring = async () => {
    const ok = await hasKeyringEntry();
    setKeyringHasKey(ok);
  };

  useEffect(() => {
    if (mode === "github-app") checkKeyring();
  }, [mode]);

  // ─────────────────────────────────────────────────────────────────────────
  // OAuth device-flow: copy helper (same as before)
  // ─────────────────────────────────────────────────────────────────────────
  const tryCopy = async (text: string): Promise<boolean> => {
    clipboardAttempts.current += 1;
    try {
      await copyToClipboard(text);
      return true;
    } catch {
      /* fall through */
    }
    try {
      await navigator.clipboard.writeText(text);
      return true;
    } catch {
      /* fall through */
    }
    return false;
  };

  // ─────────────────────────────────────────────────────────────────────────
  // OAuth login
  // ─────────────────────────────────────────────────────────────────────────
  const handleOAuthLogin = async () => {
    setLoading(true);
    setError("");
    setStatus("");
    setUserCode("");
    setExpiresIn(0);
    clipboardAttempts.current = 0;
    try {
      const deviceCode = await getDeviceCode();
      setUserCode(deviceCode.user_code || "");
      setVerificationUri(
        deviceCode.verification_uri || "https://github.com/login/device",
      );
      setExpiresIn(deviceCode.expires_in || 900);

      const copied = await tryCopy(deviceCode.user_code);
      setStatus(copied ? "✓ Code copied — paste in browser" : "Type the code in the browser");

      // Open browser
      (async () => {
        try {
          await copyToClipboard(deviceCode.verification_uri);
        } catch {
          void window.open(deviceCode.verification_uri, "_blank");
        }
      })();

      pollTimerRef.current = setInterval(async () => {
        try {
          const result = await pollToken(deviceCode.device_code);
          if (pollTimerRef.current) {
            clearInterval(pollTimerRef.current);
            pollTimerRef.current = null;
          }
          if (expiryTimerRef.current) {
            clearInterval(expiryTimerRef.current);
            expiryTimerRef.current = null;
          }

          if (result.access_token) {
            setStatus("✓ Authorized! Setting up…");
            setLoading(false);
            setTimeout(() => {
              localStorage.setItem("hiem_session_id", result.access_token!);
              onLogin(result.access_token!);
            }, 600);
          } else if (result.error) {
            setError(result.error);
            setLoading(false);
          }
        } catch (e: unknown) {
          if (pollTimerRef.current) {
            clearInterval(pollTimerRef.current);
            pollTimerRef.current = null;
          }
          const msg =
            typeof e === "object" && e !== null && "message" in e
              ? String((e as { message: string }).message)
              : String(e);
          setError(msg || "Polling failed.");
          setLoading(false);
        }
      }, (deviceCode.interval || 5) * 1000);
    } catch (e: unknown) {
      const msg =
        typeof e === "object" && e !== null && "message" in e
          ? String((e as { message: string }).message)
          : String(e);
      setError(msg || "Login failed.");
      setLoading(false);
    }
  };

  // ─────────────────────────────────────────────────────────────────────────
  // GitHub App login: store PEM → request installation token
  // ─────────────────────────────────────────────────────────────────────────
  const handleStorePemAndLogin = async () => {
    if (!pemPath.trim()) {
      setError("Enter the path to your GitHub App private key PEM file.");
      return;
    }
    if (!instId.trim() || isNaN(Number(instId))) {
      setError("Enter a valid GitHub App Installation ID (numeric).");
      return;
    }

    setLoading(true);
    setError("");
    setStatus("Storing private key in keyring…");
    try {
      await storeGithubAppKey(pemPath.trim());
      setStatus("✓ Key stored. Requesting installation token…");
      await checkKeyring();

      const result = await signInWithInstallationToken(Number(instId));
      if (result.status === 200 && result.login) {
        setStatus(`✓ Authenticated as ${result.login}!`);
        setLoading(false);
        setTimeout(() => {
          localStorage.setItem("hiem_session_id", result.loggedInAs);
          onLogin(result.loggedInAs);
        }, 600);
      } else {
        setError(
          `GitHub returned HTTP ${result.status}. ` +
            "Check your installation ID and that the app is installed."
        );
        setLoading(false);
      }
    } catch (e: unknown) {
      const msg =
        typeof e === "object" && e !== null && "message" in e
          ? String((e as { message: string }).message)
          : String(e);
      setError(msg || "GitHub App sign-in failed.");
      setLoading(false);
    }
  };

  // ─────────────────────────────────────────────────────────────────────────
  // Cleanup
  // ─────────────────────────────────────────────────────────────────────────
  useEffect(() => {
    return () => {
      if (pollTimerRef.current) clearInterval(pollTimerRef.current);
      if (expiryTimerRef.current) clearInterval(expiryTimerRef.current);
    };
  }, []);

  // ─────────────────────────────────────────────────────────────────────────
  // Render
  // ─────────────────────────────────────────────────────────────────────────
  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "center",
        height: "100vh",
        background: "#0d1117",
        color: "#e6edf3",
        gap: 0,
      }}
    >
      <div style={{ textAlign: "center", maxWidth: 440, width: "100%" }}>
        <h1 style={{ fontSize: 32, fontWeight: 700, marginBottom: 6 }}>
          🚀 HIEM
        </h1>
        <p style={{ color: "#8b949e", fontSize: 14, marginBottom: 24 }}>
          High-fidelity Engineering that Moves
        </p>

        {/* ── Mode toggle ────────────────────────────────────────────────── */}
        <div
          style={{
            display: "flex",
            gap: 0,
            background: "#161b22",
            borderRadius: 10,
            padding: 4,
            border: "1px solid #30363d",
            marginBottom: 28,
          }}
        >
          {([
            ["oauth", "OAuth Login"],
            ["github-app", "GitHub App"],
          ] as [AuthMode, string][]).map(([m, label]) => (
            <button
              key={m}
              onClick={() => {
                setMode(m);
                setError("");
                setStatus("");
              }}
              style={{
                flex: 1,
                padding: "8px 0",
                border: "none",
                borderRadius: 7,
                fontSize: 13,
                fontWeight: 600,
                cursor: "pointer",
                background: mode === m ? "#1f6feb" : "transparent",
                color: mode === m ? "#ffffff" : "#8b949e",
                transition: "all 0.15s ease",
              }}
            >
              {label}
            </button>
          ))}
        </div>

        {/* ================================================================== */}
        {/* OAuth Device Flow                                                    */}
        {/* ================================================================== */}
        {mode === "oauth" && (
          !loading ? (
            <div>
              <button
                onClick={handleOAuthLogin}
                style={{
                  padding: "12px 32px",
                  borderRadius: 8,
                  border: "none",
                  fontSize: 15,
                  fontWeight: 600,
                  cursor: "pointer",
                  background: "#1f6feb",
                  color: "#ffffff",
                  width: "100%",
                  animation: "bouncer 0.6s ease",
                }}
              >
                Login with GitHub
              </button>
              <p style={{ marginTop: 12, fontSize: 12, color: "#484f58" }}>
                OAuth device flow · opens your browser automatically
              </p>
            </div>
          ) : (
            <div>
              <div
                style={{
                  padding: "20px 24px",
                  borderRadius: 12,
                  background: "#161b22",
                  border: "1px solid #30363d",
                  marginBottom: 16,
                }}
              >
                <p style={{ margin: "0 0 12px 0", fontSize: 13, color: "#8b949e" }}>
                  A browser window should have opened. If not,{" "}
                  <a
                    href={verificationUri}
                    target="_blank"
                    rel="noopener noreferrer"
                    style={{ color: "#58a6ff", textDecoration: "none" }}
                  >
                    click here
                  </a>
                  .
                </p>

                {userCode && (
                  <div style={{ marginBottom: 12 }}>
                    <p
                      style={{
                        margin: "0 0 6px 0",
                        fontSize: 12,
                        color: "#8b949e",
                        textTransform: "uppercase",
                        letterSpacing: "0.05em",
                      }}
                    >
                      Enter this code at{" "}
                      <strong>github.com/login/device</strong>
                    </p>
                    <div
                      style={{
                        fontSize: 36,
                        fontWeight: 700,
                        letterSpacing: "0.15em",
                        color: "#e6edf3",
                        fontFamily: "monospace",
                        padding: "12px 16px",
                        background: "#0d1117",
                        borderRadius: 8,
                        border: "1px solid #30363d",
                      }}
                    >
                      {userCode}
                    </div>
                  </div>
                )}

                {expiresIn > 0 && (
                  <p
                    style={{
                      margin: 0,
                      fontSize: 12,
                      color: expiresIn < 60 ? "#f85149" : "#8b949e",
                    }}
                  >
                    Code expires in {formatTime(expiresIn)}
                  </p>
                )}
              </div>

              <button
                onClick={() => {
                  setLoading(false);
                  setUserCode("");
                  if (pollTimerRef.current) {
                    clearInterval(pollTimerRef.current);
                    pollTimerRef.current = null;
                  }
                  if (expiryTimerRef.current) {
                    clearInterval(expiryTimerRef.current);
                    expiryTimerRef.current = null;
                  }
                }}
                style={{
                  background: "none",
                  border: "none",
                  color: "#8b949e",
                  fontSize: 12,
                  cursor: "pointer",
                  textDecoration: "underline",
                }}
              >
                Cancel
              </button>
            </div>
          )
        )}

        {/* ================================================================== */}
        {/* GitHub App / keyring path                                            */}
        {/* ================================================================== */}
        {mode === "github-app" && (
          <div style={{ textAlign: "left" }}>
            {/* PEM file path */}
            <p
              style={{
                fontSize: 12,
                color: "#8b949e",
                marginBottom: 6,
              }}
            >
              Private key PEM file
            </p>
            <input
              value={pemPath}
              onChange={(e) => setPemPath(e.target.value)}
              placeholder="/path/to/github-app-key.pem"
              disabled={loading}
              style={{
                width: "100%",
                padding: "10px 14px",
                borderRadius: 8,
                border: "1px solid #30363d",
                background: "#0d1117",
                color: "#e6edf3",
                fontSize: 13,
                outline: "none",
                marginBottom: 16,
                fontFamily: "monospace",
                boxSizing: "border-box",
              }}
            />

            {/* Installation ID */}
            <p
              style={{
                fontSize: 12,
                color: "#8b949e",
                marginBottom: 6,
              }}
            >
              Installation ID
            </p>
            <input
              value={instId}
              onChange={(e) => setInstId(e.target.value)}
              placeholder="e.g. 18342712"
              disabled={loading}
              style={{
                width: "100%",
                padding: "10px 14px",
                borderRadius: 8,
                border: "1px solid #30363d",
                background: "#0d1117",
                color: "#e6edf3",
                fontSize: 13,
                outline: "none",
                marginBottom: 16,
                fontFamily: "monospace",
                boxSizing: "border-box",
              }}
            />

            <button
              onClick={handleStorePemAndLogin}
              disabled={loading}
              style={{
                width: "100%",
                padding: "12px 0",
                borderRadius: 8,
                border: "none",
                fontSize: 15,
                fontWeight: 600,
                cursor: loading ? "default" : "pointer",
                background: loading ? "#21262d" : "#238636",
                color: loading ? "#484f58" : "#ffffff",
                transition: "background 0.15s",
              }}
            >
              {loading ? "Authenticating…" : "Sign in with GitHub App"}
            </button>

            {keyringHasKey && (
              <p style={{ marginTop: 10, fontSize: 12, color: "#3fb950" }}>
                ✓ Private key found in keyring (stored previously)
              </p>
            )}
            <p style={{ marginTop: 8, fontSize: 11, color: "#484f58" }}>
              {keyringHasKey
                ? "Leave the PEM path blank to reuse the stored key, or enter a new path to rotate it."
                : "The PEM file path is required on first use. The key is stored in the OS keyring."}
            </p>
          </div>
        )}

        {/* ── Status / error ──────────────────────────────────────────────── */}
        {status && (
          <p style={{ marginTop: 16, fontSize: 13, color: "#58a6ff", wordBreak: "break-word", maxWidth: 400 }}>
            {status}
          </p>
        )}
        {error && (
          <p
            style={{
              marginTop: 12,
              fontSize: 13,
              color: "#f85149",
              wordBreak: "break-word",
              maxWidth: 400,
            }}
          >
            {error}
          </p>
        )}
      </div>

      <style>{`
        @keyframes bouncer {
          0%, 100% { transform: translateY(0); }
          20%  { transform: translateY(-4px); }
          40%  { transform: translateY(0); }
          60%  { transform: translateY(-6px); }
          80%  { transform: translateY(0); }
        }
      `}</style>
    </div>
  );
}
