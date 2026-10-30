use std::process::Command;
use std::time::Duration;

use anyhow::{anyhow, Result};
use serde_json::Value;

use crate::model::Track;

fn clean_title(raw: &str) -> String {
    let mut t = raw.to_string();
    for junk in [
        "(Official Music Video)",
        "[Official Music Video]",
        "(Official Video)",
        "[Official Video]",
        "(Official Audio)",
        "[Official Audio]",
        "(Lyric Video)",
        "[Lyric Video]",
        "(Audio)",
        "[Audio]",
        "(Visualizer)",
        "[Visualizer]",
        "(Lyrics)",
        "[Lyrics]",
        "(HD)",
        "[HD]",
        "(4K)",
        "[4K]",
    ] {
        t = t.replace(junk, "");
    }
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn parse_entry(v: &Value) -> Option<Track> {
    let id = v.get("id")?.as_str()?.to_string();
    if id.is_empty() {
        return None;
    }
    if v.get("ie_key").and_then(|k| k.as_str()) == Some("YoutubeTab") {
        return None;
    }
    let raw_title = v.get("title").and_then(|t| t.as_str()).unwrap_or("Sin título");
    let uploader = v
        .get("uploader")
        .or_else(|| v.get("channel"))
        .or_else(|| v.get("artist"))
        .and_then(|a| a.as_str())
        .unwrap_or("");

    let mut title = clean_title(raw_title);
    let mut artist = uploader.to_string();

    if let Some(stripped) = uploader.strip_suffix(" - Topic") {
        artist = stripped.to_string();
    } else if artist.is_empty() || artist == "YouTube" {
        if let Some((a, t)) = raw_title.split_once(" - ") {
            artist = a.trim().to_string();
            title = clean_title(t);
        }
    }

    // "Artist - Title" uploads repeat the artist in the title.
    if !artist.is_empty() {
        for sep in [" - ", " – ", " — "] {
            let prefix = format!("{artist}{sep}");
            if let Some(rest) = title.strip_prefix(prefix.as_str()) {
                title = rest.trim().to_string();
                break;
            }
        }
    }

    let duration = v.get("duration").and_then(|d| d.as_f64()).unwrap_or(0.0);
    if duration > 3600.0 {
        return None;
    }

    let art = v
        .get("thumbnails")
        .and_then(|t| t.as_array())
        .and_then(|arr| {
            arr.iter()
                .filter_map(|th| {
                    let url = th.get("url")?.as_str()?.to_string();
                    let w = th.get("width").and_then(|w| w.as_u64()).unwrap_or(0);
                    Some((w, url))
                })
                .max_by_key(|(w, _)| *w)
                .map(|(_, url)| url)
        })
        .unwrap_or_else(|| format!("https://i.ytimg.com/vi/{id}/maxresdefault.jpg"));

    Some(Track {
        id,
        title,
        artist,
        album: String::new(),
        duration,
        art,
        source: "youtube".into(),
    })
}

fn run_ytdlp(args: &[&str]) -> Result<Value> {
    let out = Command::new("yt-dlp")
        .args(args)
        .output()
        .map_err(|e| anyhow!("yt-dlp no está instalado: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(anyhow!("yt-dlp: {}", err.lines().last().unwrap_or("error")));
    }
    let value: Value = serde_json::from_slice(&out.stdout)?;
    Ok(value)
}

pub fn search(query: &str, limit: usize) -> Result<Vec<Track>> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let spec = format!("ytsearch{}:{q}", limit.clamp(1, 30));
    let value = run_ytdlp(&[
        "--flat-playlist",
        "-J",
        "--no-warnings",
        "--ignore-errors",
        "--socket-timeout",
        "15",
        &spec,
    ])?;
    let mut tracks: Vec<Track> = Vec::new();
    if let Some(entries) = value.get("entries").and_then(|e| e.as_array()) {
        for entry in entries {
            if let Some(t) = parse_entry(entry) {
                if !tracks.iter().any(|x| x.id == t.id) {
                    tracks.push(t);
                }
            }
        }
        if tracks.is_empty() {
            if let Some(t) = parse_entry(&value) {
                tracks.push(t);
            }
        }
    }
    Ok(tracks)
}

#[allow(dead_code)]
pub fn playlist(url: &str) -> Result<Vec<Track>> {
    let value = run_ytdlp(&[
        "--flat-playlist",
        "-J",
        "--no-warnings",
        "--ignore-errors",
        "--socket-timeout",
        "15",
        url,
    ])?;
    let mut tracks = Vec::new();
    if let Some(entries) = value.get("entries").and_then(|e| e.as_array()) {
        for entry in entries {
            if let Some(t) = parse_entry(entry) {
                tracks.push(t);
            }
        }
    }
    Ok(tracks)
}

/// Download cover art, falling back through YouTube's thumbnail sizes.
pub fn download_cover(id: &str, url: &str) -> Result<std::path::PathBuf> {
    let dest = crate::paths::art_path(id);
    if dest.exists() {
        return Ok(dest);
    }
    std::fs::create_dir_all(dest.parent().unwrap_or(std::path::Path::new("/tmp")))?;
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(12))
        .user_agent("MarmotaMusic/0.1")
        .build()?;

    let mut candidates = vec![url.to_string()];
    if url.contains("maxresdefault") {
        candidates.push(format!("https://i.ytimg.com/vi/{id}/hqdefault.jpg"));
        candidates.push(format!("https://i.ytimg.com/vi/{id}/mqdefault.jpg"));
    }
    candidates.push(format!("https://i.ytimg.com/vi/{id}/hqdefault.jpg"));

    for candidate in candidates {
        if candidate.is_empty() {
            continue;
        }
        if let Ok(resp) = client.get(&candidate).send() {
            if resp.status().is_success() {
                if let Ok(bytes) = resp.bytes() {
                    if bytes.len() > 1024 {
                        std::fs::write(&dest, &bytes)?;
                        return Ok(dest);
                    }
                }
            }
        }
    }
    Err(anyhow!("sin portada"))
}
