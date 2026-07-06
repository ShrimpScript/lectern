//! Cloud client — the OPTIONAL link to the Lectern web control plane. Handles
//! the OAuth 2.0 device-grant login, content-free usage telemetry, and E2E-encrypted
//! skills/memory sync. INVARIANT: the cloud only ever receives counts or ciphertext —
//! never source, prompts, or keys. See Lectern-Brain/03-Architecture/Sync, Auth & Entitlements.md.
use anyhow::{anyhow, bail, Result};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::PathBuf;

pub const DEFAULT_BASE_URL: &str = "https://getlectern.vercel.app";

/// Short-timeout HTTP agent so one-shot cloud calls never hang a run.
fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(5))
        .timeout(std::time::Duration::from_secs(10))
        .build()
}

/// Persisted login (data_dir/auth.json, 0600). Token is an engine token (lk_…).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Auth {
    pub base_url: String,
    pub token: String,
}

fn auth_path() -> PathBuf {
    crate::data_dir().join("auth.json")
}

pub fn load_auth() -> Option<Auth> {
    let s = std::fs::read_to_string(auth_path()).ok()?;
    serde_json::from_str(&s).ok()
}

pub fn save_auth(auth: &Auth) -> Result<()> {
    let path = auth_path();
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    std::fs::write(&path, serde_json::to_string_pretty(auth)?)?;
    restrict(&path);
    Ok(())
}

pub fn clear_auth() -> Result<()> {
    let _ = std::fs::remove_file(auth_path());
    Ok(())
}

#[cfg(unix)]
fn restrict(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}
#[cfg(not(unix))]
fn restrict(_path: &std::path::Path) {}

// ── Device authorization grant ───────────────────────────────────────────────
#[derive(Debug, Deserialize)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    #[serde(default)]
    pub verification_uri_complete: Option<String>,
    #[serde(default = "default_interval")]
    pub interval: u64,
    #[serde(default)]
    pub expires_in: u64,
}
fn default_interval() -> u64 {
    5
}

pub fn request_device_code(base_url: &str) -> Result<DeviceCode> {
    let resp = agent()
        .post(&format!("{base_url}/api/device/code"))
        .call()
        .map_err(|e| anyhow!("requesting device code: {e}"))?;
    Ok(resp.into_json()?)
}

/// Poll until the user approves (or the code expires). Blocks, sleeping `interval`.
pub fn poll_for_token(
    base_url: &str,
    device_code: &str,
    interval: u64,
    expires_in: u64,
) -> Result<String> {
    let interval = interval.max(1);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(expires_in.max(60));
    loop {
        if std::time::Instant::now() > deadline {
            bail!("device code expired before approval");
        }
        match agent()
            .post(&format!("{base_url}/api/device/token"))
            .send_json(serde_json::json!({ "device_code": device_code }))
        {
            Ok(resp) => {
                let v: serde_json::Value = resp.into_json()?;
                if let Some(tok) = v.get("access_token").and_then(|t| t.as_str()) {
                    return Ok(tok.to_string());
                }
            }
            Err(ureq::Error::Status(428, _)) => {} // authorization_pending
            Err(ureq::Error::Status(_, resp)) => {
                let v: serde_json::Value = resp.into_json().unwrap_or_default();
                let err = v.get("error").and_then(|e| e.as_str()).unwrap_or("error");
                if err == "authorization_pending" || err == "slow_down" {
                    // keep polling
                } else {
                    bail!("device authorization failed: {err}");
                }
            }
            Err(e) => bail!("polling token: {e}"),
        }
        std::thread::sleep(std::time::Duration::from_secs(interval));
    }
}

// ── Authenticated calls ──────────────────────────────────────────────────────

/// Humanize a cloud/HTTP failure — the cloud-side counterpart to
/// `backend::friendly_claude_error`. Auth problems say how to sign in, network
/// problems say what to check; anything else keeps the raw tail (where the
/// real error usually is).
pub fn friendly_cloud_error(op: &str, raw: &str) -> String {
    let low = raw.to_lowercase();
    let auth = [
        "401",
        "unauthorized",
        "403",
        "forbidden",
        "invalid token",
        "token expired",
    ];
    if auth.iter().any(|k| low.contains(k)) {
        return format!("{op}: not signed in — run `lectern login`, then retry.");
    }
    let net = [
        "connection",
        "connect",
        "timed out",
        "timeout",
        "dns",
        "resolve",
        "unreachable",
        "refused",
        "network",
    ];
    if net.iter().any(|k| low.contains(k)) {
        return format!(
            "{op}: can't reach the Lectern cloud — check your connection (sync retries later)."
        );
    }
    let r = raw.trim();
    if r.is_empty() {
        return format!("{op}: failed with no detail.");
    }
    let tail: String = if r.chars().count() > 300 {
        let skip = r.chars().count() - 300;
        format!("…{}", r.chars().skip(skip).collect::<String>())
    } else {
        r.to_string()
    };
    format!("{op}: {tail}")
}

pub fn get_entitlements(auth: &Auth) -> Result<serde_json::Value> {
    let resp = agent()
        .get(&format!("{}/api/entitlements", auth.base_url))
        .set("Authorization", &format!("Bearer {}", auth.token))
        .call()
        .map_err(|e| {
            anyhow!(
                "{}",
                friendly_cloud_error("fetching entitlements", &e.to_string())
            )
        })?;
    Ok(resp.into_json()?)
}

#[derive(Debug, Serialize)]
pub struct UsageRow {
    pub day: String,
    pub backend: String,
    pub sessions: u64,
    #[serde(rename = "tokensIn")]
    pub tokens_in: u64,
    #[serde(rename = "tokensOut")]
    pub tokens_out: u64,
    #[serde(rename = "costCents")]
    pub cost_cents: u64,
}

pub fn ingest_usage(auth: &Auth, rows: &[UsageRow]) -> Result<()> {
    agent()
        .post(&format!("{}/api/usage/ingest", auth.base_url))
        .set("Authorization", &format!("Bearer {}", auth.token))
        .send_json(serde_json::json!({ "rows": rows }))
        .map_err(|e| anyhow!("{}", friendly_cloud_error("usage ingest", &e.to_string())))?;
    Ok(())
}

pub fn push_blob(auth: &Auth, workspace_key: &str, ciphertext_b64: &str) -> Result<()> {
    let sha = sha256_hex(ciphertext_b64.as_bytes());
    agent()
        .request("PUT", &format!("{}/api/sync/blobs", auth.base_url))
        .set("Authorization", &format!("Bearer {}", auth.token))
        .send_json(serde_json::json!({
            "workspaceKey": workspace_key,
            "sha256": sha,
            "size": ciphertext_b64.len(),
            "ciphertext": ciphertext_b64,
        }))
        .map_err(|e| anyhow!("{}", friendly_cloud_error("pushing blob", &e.to_string())))?;
    crate::diag::log(
        "cloud",
        &format!("pushed blob ({} B ciphertext)", ciphertext_b64.len()),
    );
    Ok(())
}

/// Returns the latest ciphertext for a workspace, or None if nothing is synced.
pub fn pull_blob(auth: &Auth, workspace_key: &str) -> Result<Option<String>> {
    let url = format!(
        "{}/api/sync/blobs?download={}",
        auth.base_url,
        urlencode(workspace_key)
    );
    match agent()
        .get(&url)
        .set("Authorization", &format!("Bearer {}", auth.token))
        .call()
    {
        Ok(resp) => {
            let v: serde_json::Value = resp.into_json()?;
            let ct = v
                .get("ciphertext")
                .and_then(|c| c.as_str())
                .map(|s| s.to_string());
            crate::diag::log(
                "cloud",
                if ct.is_some() {
                    "pulled blob"
                } else {
                    "pull: no ciphertext in response"
                },
            );
            Ok(ct)
        }
        Err(ureq::Error::Status(404, _)) => {
            crate::diag::log("cloud", "pull: nothing synced (404)");
            Ok(None)
        }
        Err(e) => Err(anyhow!("pulling blob: {e}")),
    }
}

// ── E2E encryption (XChaCha20-Poly1305, local key) ───────────────────────────
// The key never leaves the machine; cross-device key sync is the documented next step.
fn key_path() -> PathBuf {
    crate::data_dir().join("sync.key")
}

fn load_or_create_key() -> Result<[u8; 32]> {
    let path = key_path();
    if let Ok(b) = std::fs::read(&path) {
        if b.len() == 32 {
            let mut k = [0u8; 32];
            k.copy_from_slice(&b);
            return Ok(k);
        }
    }
    let mut k = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut k);
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    let mut f = std::fs::File::create(&path)?;
    f.write_all(&k)?;
    restrict(&path);
    Ok(k)
}

pub fn encrypt(plaintext: &[u8]) -> Result<String> {
    let key = load_or_create_key()?;
    let cipher = XChaCha20Poly1305::new(Key::from_slice(&key));
    let mut nonce = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut nonce);
    let ct = cipher
        .encrypt(XNonce::from_slice(&nonce), plaintext)
        .map_err(|e| anyhow!("encrypt: {e}"))?;
    let mut out = nonce.to_vec();
    out.extend_from_slice(&ct);
    Ok(STANDARD.encode(out))
}

pub fn decrypt(b64: &str) -> Result<Vec<u8>> {
    let raw = STANDARD.decode(b64.trim())?;
    if raw.len() < 24 {
        bail!("ciphertext too short");
    }
    let (nonce, ct) = raw.split_at(24);
    let key = load_or_create_key()?;
    let cipher = XChaCha20Poly1305::new(Key::from_slice(&key));
    cipher
        .decrypt(XNonce::from_slice(nonce), ct)
        .map_err(|_| anyhow!("decrypt failed (wrong key or corrupt blob)"))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloud_failures_get_actionable_messages() {
        // Auth → sign-in hint.
        let m = friendly_cloud_error(
            "usage ingest",
            "https://getlectern.vercel.app: status 401 Unauthorized",
        );
        assert!(m.contains("lectern login"), "{m}");
        // Network → connectivity hint.
        let m = friendly_cloud_error("pushing blob", "Dns Failed: resolve getlectern.vercel.app");
        assert!(m.contains("check your connection"), "{m}");
        let m = friendly_cloud_error("fetching entitlements", "Network Error: connection refused");
        assert!(m.contains("check your connection"), "{m}");
        // Anything else keeps the op + raw tail; empty stays explicit.
        assert!(friendly_cloud_error("pushing blob", "status 500 oops").contains("500 oops"));
        assert!(friendly_cloud_error("pushing blob", "").contains("no detail"));
    }
}
