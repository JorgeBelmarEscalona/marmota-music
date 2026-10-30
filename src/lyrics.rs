use std::time::Duration;

use anyhow::{anyhow, Result};
use serde::Deserialize;

use crate::model::{LyricLine, Track};

pub struct Lyrics {
    pub lines: Vec<LyricLine>,
    pub synced: bool,
    pub synced_text: Option<String>,
    pub plain_text: String,
}

#[derive(Deserialize)]
struct LrcReply {
    #[serde(rename = "syncedLyrics")]
    synced: Option<String>,
    #[serde(rename = "plainLyrics")]
    plain: Option<String>,
}

fn parse_timestamp(tag: &str) -> Option<f64> {
    let (m, rest) = tag.split_once(':')?;
    let minutes: f64 = m.trim().parse().ok()?;
    let seconds: f64 = rest.trim().parse().ok()?;
    Some(minutes * 60.0 + seconds)
}

pub fn parse_lrc(text: &str) -> Vec<LyricLine> {
    let mut out = Vec::new();
    for raw in text.lines() {
        let mut rest = raw.trim();
        let mut stamps = Vec::new();
        while let Some(after) = rest.strip_prefix('[') {
            let Some(end) = after.find(']') else { break };
            let tag = &after[..end];
            if let Some(t) = parse_timestamp(tag) {
                stamps.push(t);
            }
            rest = after[end + 1..].trim_start();
        }
        let body = rest.trim();
        if body.is_empty() || stamps.is_empty() {
            continue;
        }
        for t in stamps {
            out.push(LyricLine {
                t,
                text: body.to_string(),
            });
        }
    }
    out.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap_or(std::cmp::Ordering::Equal));
    out
}

pub fn parse_plain(text: &str) -> Vec<LyricLine> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .enumerate()
        .map(|(i, l)| LyricLine {
            t: i as f64 * 4.0,
            text: l.trim().to_string(),
        })
        .collect()
}

fn build(synced: Option<String>, plain: Option<String>) -> Result<Lyrics> {
    if let Some(s) = synced.as_deref().filter(|s| !s.trim().is_empty()) {
        return Ok(Lyrics {
            lines: parse_lrc(s),
            synced: true,
            synced_text: Some(s.to_string()),
            plain_text: plain.unwrap_or_default(),
        });
    }
    if let Some(p) = plain.as_deref().filter(|s| !s.trim().is_empty()) {
        return Ok(Lyrics {
            lines: parse_plain(p),
            synced: false,
            synced_text: None,
            plain_text: p.to_string(),
        });
    }
    Err(anyhow!("letra vacía"))
}

fn client() -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(9))
        .user_agent("MarmotaMusic/0.1 (liquid glass notch)")
        .build()?)
}

pub fn fetch(track: &Track) -> Result<Lyrics> {
    let c = client()?;
    let mut attempt = 0;
    loop {
        attempt += 1;
        let resp = c
            .get("https://lrclib.net/api/get")
            .query(&[
                ("artist_name", track.artist.as_str()),
                ("track_name", track.title.as_str()),
                ("album_name", track.album.as_str()),
                ("duration", &format!("{}", track.duration.round() as i64)),
            ])
            .send();
        match resp {
            Ok(r) if r.status().is_success() => {
                if let Ok(reply) = r.json::<LrcReply>() {
                    return build(reply.synced, reply.plain);
                }
                return Err(anyhow!("respuesta inválida"));
            }
            Ok(r) if r.status().as_u16() == 503 && attempt < 3 => {
                std::thread::sleep(Duration::from_millis(700));
            }
            Ok(_) => break,
            Err(_) if attempt < 3 => {
                std::thread::sleep(Duration::from_millis(700));
            }
            Err(_) => return Err(anyhow!("sin conexión con LRCLIB")),
        }
    }

    let resp = c
        .get("https://lrclib.net/api/search")
        .query(&[
            ("track_name", track.title.as_str()),
            ("artist_name", track.artist.as_str()),
        ])
        .send()?;
    if resp.status().is_success() {
        let list: Vec<LrcReply> = resp.json().unwrap_or_default();
        for item in list {
            if let Ok(found) = build(item.synced, item.plain) {
                return Ok(found);
            }
        }
    }
    Err(anyhow!("no se encontró la letra"))
}
