//! Apple Music catalog search (Apple Music API). Needs a MusicKit key from
//! an Apple Developer account: the key file (.p8), its Key ID and the Team
//! ID sign a developer token (ES256 JWT) that no user sign-in is needed for.
//!
//! Songs found here play from YouTube Music, like Spotify songs without a
//! Spotify device; playing them through Apple Music itself is a later step.

use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use base64::Engine;
use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use p256::pkcs8::DecodePrivateKey;
use serde_json::{json, Value};

use crate::model::Track;

/// Signed tokens may last up to six months; a day is plenty and keeps a
/// revoked key from working for long.
const TOKEN_LIFE: Duration = Duration::from_secs(24 * 3600);

static TOKEN: Mutex<Option<(String, Instant)>> = Mutex::new(None);

pub struct Keys {
    pub team_id: String,
    pub key_id: String,
    pub key_file: String,
}

pub fn keys() -> Option<Keys> {
    let v = crate::paths::config();
    let get = |k: &str| v.get(k).and_then(|x| x.as_str()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    Some(Keys { team_id: get("apple_team_id")?, key_id: get("apple_key_id")?, key_file: get("apple_key_file")? })
}

pub fn configured() -> bool {
    keys().is_some_and(|k| std::path::Path::new(&k.key_file).is_file())
}

/// Two-letter Apple Music country (storefront): the config key
/// `apple_storefront`, else the region of the system language, else "us".
pub fn storefront() -> String {
    if let Some(s) = crate::paths::config().get("apple_storefront").and_then(|v| v.as_str()) {
        let s = s.trim().to_ascii_lowercase();
        if s.len() == 2 {
            return s;
        }
    }
    locale_region().unwrap_or_else(|| "us".into())
}

/// "es_CL.UTF-8" → "cl".
pub fn locale_region() -> Option<String> {
    let lang = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))?;
    let region = lang.split(['.', '@']).next()?.split(['_', '-']).nth(1)?;
    (region.len() == 2).then(|| region.to_ascii_lowercase())
}

/// Developer token for the Apple Music API.
fn sign(keys: &Keys) -> Result<String> {
    let pem = std::fs::read_to_string(&keys.key_file)
        .with_context(|| format!("No pude leer la clave {}", keys.key_file))?;
    let key = SigningKey::from_pkcs8_pem(pem.trim())
        .map_err(|_| anyhow!("El archivo .p8 no es una clave MusicKit válida"))?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    let header = json!({ "alg": "ES256", "kid": keys.key_id });
    let claims = json!({ "iss": keys.team_id, "iat": now, "exp": now + TOKEN_LIFE.as_secs() });
    let body = format!("{}.{}", B64.encode(header.to_string()), B64.encode(claims.to_string()));
    // JWS wants the raw r‖s signature, which is what to_bytes() gives.
    let sig: Signature = key.sign(body.as_bytes());
    Ok(format!("{body}.{}", B64.encode(sig.to_bytes())))
}

fn token() -> Result<String> {
    if let Ok(guard) = TOKEN.lock() {
        if let Some((t, until)) = guard.as_ref() {
            if Instant::now() < *until {
                return Ok(t.clone());
            }
        }
    }
    let keys = keys().ok_or_else(|| anyhow!("Apple Music no está configurado"))?;
    let t = sign(&keys)?;
    if let Ok(mut guard) = TOKEN.lock() {
        *guard = Some((t.clone(), Instant::now() + TOKEN_LIFE - Duration::from_secs(600)));
    }
    Ok(t)
}

/// Forget the cached token (after the keys change).
pub fn reset() {
    if let Ok(mut guard) = TOKEN.lock() {
        *guard = None;
    }
}

/// Square artwork URL from Apple's "{w}x{h}" template.
fn artwork(attrs: &Value, size: u32) -> String {
    attrs
        .pointer("/artwork/url")
        .and_then(|u| u.as_str())
        .map(|u| u.replace("{w}", &size.to_string()).replace("{h}", &size.to_string()))
        .unwrap_or_default()
}

pub fn search(query: &str) -> Result<Vec<Track>> {
    let tok = token()?;
    let c = reqwest::blocking::Client::builder().timeout(Duration::from_secs(10)).build()?;
    let url = format!("https://api.music.apple.com/v1/catalog/{}/search", storefront());
    let resp = c
        .get(url)
        .bearer_auth(&tok)
        .query(&[("term", query), ("types", "songs"), ("limit", "20")])
        .send()?;
    let status = resp.status();
    if status.as_u16() == 401 || status.as_u16() == 403 {
        reset();
        return Err(anyhow!("Apple rechazó la clave ({status}): revisa Team ID, Key ID y que la clave tenga MusicKit"));
    }
    if !status.is_success() {
        let body = resp.text().unwrap_or_default();
        let msg = serde_json::from_str::<Value>(&body)
            .ok()
            .and_then(|v| v.pointer("/errors/0/detail").and_then(|m| m.as_str()).map(str::to_string))
            .unwrap_or_default();
        return Err(anyhow!("Apple Music respondió {status} {msg}"));
    }
    let v: Value = resp.json()?;
    let items = v.pointer("/results/songs/data").and_then(|d| d.as_array()).cloned().unwrap_or_default();
    let mut out = Vec::new();
    for item in &items {
        let Some(id) = item.get("id").and_then(|i| i.as_str()) else { continue };
        let Some(attrs) = item.get("attributes") else { continue };
        let text = |k: &str| attrs.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
        out.push(Track {
            id: format!("am_{id}"),
            title: text("name"),
            artist: text("artistName"),
            album: text("albumName"),
            duration: attrs.get("durationInMillis").and_then(|d| d.as_f64()).unwrap_or(0.0) / 1000.0,
            art: artwork(attrs, 600),
            source: "apple".into(),
            video: false,
            quality: 3,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use p256::ecdsa::signature::Verifier;
    use p256::ecdsa::VerifyingKey;

    #[test]
    fn signs_a_verifiable_es256_token() {
        let Ok(file) = std::env::var("MM_TEST_P8") else { return };
        let keys = Keys { team_id: "TEAM123456".into(), key_id: "KEY1234567".into(), key_file: file.clone() };
        let jwt = sign(&keys).unwrap();
        let parts: Vec<&str> = jwt.split('.').collect();
        assert_eq!(parts.len(), 3);
        let header: Value = serde_json::from_slice(&B64.decode(parts[0]).unwrap()).unwrap();
        assert_eq!(header["alg"], "ES256");
        assert_eq!(header["kid"], "KEY1234567");
        let claims: Value = serde_json::from_slice(&B64.decode(parts[1]).unwrap()).unwrap();
        assert_eq!(claims["iss"], "TEAM123456");
        let key = SigningKey::from_pkcs8_pem(&std::fs::read_to_string(file).unwrap()).unwrap();
        let sig = Signature::from_slice(&B64.decode(parts[2]).unwrap()).unwrap();
        VerifyingKey::from(&key).verify(format!("{}.{}", parts[0], parts[1]).as_bytes(), &sig).unwrap();
    }

    #[test]
    fn region_from_locale() {
        std::env::set_var("LC_ALL", "es_CL.UTF-8");
        assert_eq!(locale_region().as_deref(), Some("cl"));
    }
}
