//! Spotify Web API search (client-credentials flow). Needs a free app from
//! developer.spotify.com; its id and secret live in the config file.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use serde_json::Value;

use crate::model::Track;

static TOKEN: Mutex<Option<(String, Instant)>> = Mutex::new(None);

fn credentials() -> Option<(String, String)> {
    let v = crate::paths::config();
    let id = v.get("spotify_client_id")?.as_str()?.trim().to_string();
    let secret = v.get("spotify_client_secret")?.as_str()?.trim().to_string();
    if id.is_empty() || secret.is_empty() {
        None
    } else {
        Some((id, secret))
    }
}

pub fn configured() -> bool {
    credentials().is_some()
}

fn client() -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?)
}

fn token(c: &reqwest::blocking::Client) -> Result<String> {
    if let Ok(guard) = TOKEN.lock() {
        if let Some((t, until)) = guard.as_ref() {
            if Instant::now() < *until {
                return Ok(t.clone());
            }
        }
    }
    let (id, secret) = credentials().ok_or_else(|| anyhow!("Spotify no está configurado"))?;
    let resp = c
        .post("https://accounts.spotify.com/api/token")
        .basic_auth(id, Some(secret))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body("grant_type=client_credentials")
        .send()?;
    if !resp.status().is_success() {
        return Err(anyhow!("Spotify rechazó las credenciales ({})", resp.status()));
    }
    let v: Value = resp.json()?;
    let t = v.get("access_token").and_then(|t| t.as_str()).ok_or_else(|| anyhow!("sin token"))?;
    let secs = v.get("expires_in").and_then(|e| e.as_u64()).unwrap_or(3600);
    if let Ok(mut guard) = TOKEN.lock() {
        *guard = Some((t.to_string(), Instant::now() + Duration::from_secs(secs.saturating_sub(60))));
    }
    Ok(t.to_string())
}

pub fn search(query: &str) -> Result<Vec<Track>> {
    let c = client()?;
    let tok = token(&c)?;
    // Apps in development mode may ask for at most 10 results per request,
    // so fetch two pages.
    let mut items: Vec<Value> = Vec::new();
    for offset in ["0", "10"] {
        let resp = c
            .get("https://api.spotify.com/v1/search")
            .bearer_auth(&tok)
            .query(&[("q", query), ("type", "track"), ("limit", "10"), ("offset", offset), ("market", "CL")])
            .send()?;
        if !resp.status().is_success() {
            if !items.is_empty() {
                break;
            }
            let status = resp.status();
            let body = resp.text().unwrap_or_default();
            let msg = serde_json::from_str::<Value>(&body)
                .ok()
                .and_then(|v| v.pointer("/error/message").and_then(|m| m.as_str()).map(str::to_string))
                .unwrap_or_default();
            return Err(anyhow!("Spotify respondió {status} {msg}"));
        }
        let v: Value = resp.json()?;
        let page = v.pointer("/tracks/items").and_then(|i| i.as_array()).cloned().unwrap_or_default();
        let done = page.len() < 10;
        items.extend(page);
        if done {
            break;
        }
    }
    let mut out = Vec::new();
    for item in &items {
        let Some(id) = item.get("id").and_then(|i| i.as_str()) else { continue };
        let artist = item
            .get("artists")
            .and_then(|a| a.as_array())
            .map(|a| a.iter().filter_map(|x| x.get("name")?.as_str()).collect::<Vec<_>>().join(", "))
            .unwrap_or_default();
        out.push(Track {
            id: format!("sp_{id}"),
            title: item.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string(),
            artist,
            album: item.pointer("/album/name").and_then(|n| n.as_str()).unwrap_or("").to_string(),
            duration: item.get("duration_ms").and_then(|d| d.as_f64()).unwrap_or(0.0) / 1000.0,
            art: item
                .pointer("/album/images/0/url")
                .and_then(|u| u.as_str())
                .unwrap_or("")
                .to_string(),
            source: "spotify".into(),
            video: false,
            quality: 3,
        });
    }
    Ok(out)
}
