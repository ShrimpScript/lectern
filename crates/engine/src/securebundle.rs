//! Encrypted session bundles — the exact payload
//! format cross-device sync will ship, useful today for moving a session
//! between machines by hand: export on A, import on B, passphrase over any
//! channel you trust.
//!
//! Format (single text file, armor-safe):
//! line 1: `LECTERN-ENC1`
//! line 2: JSON header `{ "kdf": "scrypt", "log_n": 15, "r": 8, "p": 1, "salt": b64, "nonce": b64 }`
//! line 3: base64(XChaCha20-Poly1305 ciphertext of the session JSON)
use anyhow::{bail, Context, Result};
use base64::Engine as _;
use chacha20poly1305::aead::Aead;
use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce};

const MAGIC: &str = "LECTERN-ENC1";
const LOG_N: u8 = 15; // 2^15 — interactive-grade scrypt cost
const R: u32 = 8;
const P: u32 = 1;

fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

fn derive_key(passphrase: &str, salt: &[u8], log_n: u8, r: u32, p: u32) -> Result<[u8; 32]> {
    let params = scrypt::Params::new(log_n, r, p, 32).context("scrypt params")?;
    let mut key = [0u8; 32];
    scrypt::scrypt(passphrase.as_bytes(), salt, &params, &mut key).context("scrypt")?;
    Ok(key)
}

/// Seal arbitrary JSON text under a passphrase → armored bundle text.
pub fn seal(plaintext: &str, passphrase: &str) -> Result<String> {
    if passphrase.len() < 8 {
        bail!("passphrase must be at least 8 characters");
    }
    let mut salt = [0u8; 16];
    let mut nonce = [0u8; 24];
    getrandom_fill(&mut salt)?;
    getrandom_fill(&mut nonce)?;
    let key = derive_key(passphrase, &salt, LOG_N, R, P)?;
    let cipher = XChaCha20Poly1305::new((&key).into());
    let ct = cipher
        .encrypt(XNonce::from_slice(&nonce), plaintext.as_bytes())
        .map_err(|_| anyhow::anyhow!("encryption failed"))?;
    let header = serde_json::json!({
        "kdf": "scrypt", "log_n": LOG_N, "r": R, "p": P,
        "salt": b64().encode(salt), "nonce": b64().encode(nonce),
    });
    Ok(format!("{MAGIC}\n{header}\n{}\n", b64().encode(ct)))
}

/// Open an armored bundle with the passphrase → the original JSON text.
pub fn open(bundle: &str, passphrase: &str) -> Result<String> {
    let mut lines = bundle.lines();
    if lines.next().map(str::trim) != Some(MAGIC) {
        bail!("not a Lectern encrypted bundle (bad magic)");
    }
    let header: serde_json::Value =
        serde_json::from_str(lines.next().unwrap_or_default()).context("bundle header")?;
    let get_b64 = |k: &str| -> Result<Vec<u8>> {
        b64()
            .decode(header.get(k).and_then(|v| v.as_str()).unwrap_or_default())
            .with_context(|| format!("bundle header field {k}"))
    };
    let salt = get_b64("salt")?;
    let nonce = get_b64("nonce")?;
    let log_n = header.get("log_n").and_then(|v| v.as_u64()).unwrap_or(15) as u8;
    let r = header.get("r").and_then(|v| v.as_u64()).unwrap_or(8) as u32;
    let p = header.get("p").and_then(|v| v.as_u64()).unwrap_or(1) as u32;
    if log_n > 20 {
        bail!("bundle demands an unreasonable scrypt cost — refusing");
    }
    let key = derive_key(passphrase, &salt, log_n, r, p)?;
    let ct = b64()
        .decode(lines.next().unwrap_or_default().trim())
        .context("bundle body")?;
    let cipher = XChaCha20Poly1305::new((&key).into());
    let pt = cipher
        .decrypt(XNonce::from_slice(&nonce), ct.as_ref())
        .map_err(|_| anyhow::anyhow!("wrong passphrase or corrupted bundle"))?;
    String::from_utf8(pt).context("bundle plaintext utf8")
}

fn getrandom_fill(buf: &mut [u8]) -> Result<()> {
    use chacha20poly1305::aead::rand_core::RngCore;
    chacha20poly1305::aead::OsRng.fill_bytes(buf);
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn wrong_passphrase_refused() {
        let sealed = seal("secret payload", "right horse").unwrap();
        let err = open(&sealed, "wrong horse").unwrap_err().to_string();
        assert!(!err.contains("secret"), "error must not leak plaintext");
    }

    #[test]
    fn truncated_and_garbage_bodies_refused() {
        let sealed = seal("payload", "longpass1").unwrap();
        let cut = &sealed[..sealed.len() - 12];
        assert!(open(cut, "longpass1").is_err());
        assert!(open("LECTERN-ENC1\nnot-base64!!!", "longpass1").is_err());
        assert!(open("", "longpass1").is_err());
        assert!(open("random text entirely", "longpass1").is_err());
    }

    #[test]
    fn tampered_ciphertext_refused() {
        let sealed = seal("payload", "longpass1").unwrap();
        // flip one character deep in the body (past the header/salt lines)
        let mut b = sealed.into_bytes();
        let k = b.len() - 20;
        b[k] = if b[k] == b'A' { b'B' } else { b'A' };
        let tampered = String::from_utf8(b).unwrap();
        assert!(
            open(&tampered, "longpass1").is_err(),
            "AEAD must catch tampering"
        );
    }

    use super::*;

    #[test]
    fn roundtrip_and_wrong_passphrase() {
        let payload = r#"{"lectern_session":1,"session":{"title":"t"},"events":[{"type":"user","text":"hi"}]}"#;
        let sealed = seal(payload, "correct horse battery").unwrap();
        assert!(sealed.starts_with(MAGIC));
        assert_eq!(open(&sealed, "correct horse battery").unwrap(), payload);
        let err = open(&sealed, "wrong passphrase!").unwrap_err().to_string();
        assert!(err.contains("wrong passphrase"), "got: {err}");
    }

    #[test]
    fn refuses_garbage_and_short_passphrases() {
        assert!(seal("x", "short").is_err());
        assert!(open("not a bundle", "whatever123").is_err());
        let e = open(
            "LECTERN-ENC1\n{\"kdf\":\"scrypt\",\"log_n\":25,\"salt\":\"\",\"nonce\":\"\"}\nAAAA\n",
            "whatever123",
        )
        .unwrap_err()
        .to_string();
        assert!(e.contains("unreasonable"), "got: {e}");
    }
}
