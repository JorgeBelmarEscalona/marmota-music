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
        .user_agent("MarmotaMusic/0.2 (https://github.com/marmota/marmota-music)")
        .build()?)
}

/// Title without the noise YouTube uploads add: "(Official Video)",
/// "ft. X", "[Remastered 2011]", "(con X)"…
pub fn clean_title(raw: &str) -> String {
    let mut t = String::new();
    let mut depth = 0i32;
    let mut chunk = String::new();
    for c in raw.chars() {
        match c {
            '(' | '[' => {
                if depth == 0 {
                    t.push_str(&chunk);
                    chunk.clear();
                }
                depth += 1;
                chunk.push(c);
            }
            ')' | ']' if depth > 0 => {
                depth -= 1;
                chunk.push(c);
                if depth == 0 {
                    let l = chunk.to_lowercase();
                    let noise = [
                        "feat", "ft.", "ft ", "with ", "con ", "official", "oficial", "video",
                        "vídeo", "audio", "lyric", "letra", "remaster", "visualizer", "live",
                        "en vivo", "hd", "4k", "explicit", "clean", "prod", "remix",
                    ];
                    if !noise.iter().any(|n| l.contains(n)) {
                        t.push_str(&chunk);
                    }
                    chunk.clear();
                }
            }
            _ => chunk.push(c),
        }
    }
    t.push_str(&chunk);
    for sep in [" ft. ", " ft ", " feat. ", " feat ", " Ft. ", " Feat. ", " FT ", " | ", " // "] {
        if let Some(i) = t.find(sep) {
            t.truncate(i);
        }
    }
    let t = t.trim().trim_matches(['-', '–', '|']).trim();
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// First credited artist: "Bad Bunny, Jowell & Randy" → "Bad Bunny".
pub fn main_artist(raw: &str) -> String {
    let mut a = raw.to_string();
    for sep in [",", " & ", " y ", " x ", " X ", " feat", " ft.", " and "] {
        if let Some(i) = a.find(sep) {
            a.truncate(i);
        }
    }
    a.trim().trim_end_matches(" - Topic").trim().to_string()
}

fn norm(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'ä' | 'â' => 'a',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' => 'o',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            'ñ' => 'n',
            c => c,
        })
        .filter(|c| c.is_alphanumeric())
        .collect()
}

pub fn similar(a: &str, b: &str) -> bool {
    let (a, b) = (norm(a), norm(b));
    !a.is_empty() && !b.is_empty() && (a.contains(&b) || b.contains(&a))
}

fn close_duration(want: f64, got: f64, tol: f64) -> bool {
    want <= 0.0 || got <= 0.0 || (want - got).abs() <= tol
}

#[derive(Deserialize)]
struct LrcItem {
    #[serde(rename = "trackName", default)]
    track: String,
    #[serde(rename = "artistName", default)]
    artist: String,
    #[serde(default)]
    duration: Option<f64>,
    #[serde(rename = "syncedLyrics")]
    synced: Option<String>,
    #[serde(rename = "plainLyrics")]
    plain: Option<String>,
}

struct Found {
    synced: Option<String>,
    plain: Option<String>,
}

fn lrclib(c: &reqwest::blocking::Client, title: &str, artist: &str, dur: f64) -> Found {
    let mut plain = None;
    // Exact lookup first: cheap and precise when the metadata is clean.
    let mut q: Vec<(&str, String)> = vec![("artist_name", artist.to_string()), ("track_name", title.to_string())];
    if dur > 0.0 {
        q.push(("duration", format!("{}", dur.round() as i64)));
    }
    for attempt in 0..2 {
        match c.get("https://lrclib.net/api/get").query(&q).send() {
            Ok(r) if r.status().is_success() => {
                if let Ok(item) = r.json::<LrcReply>() {
                    if item.synced.as_deref().is_some_and(|s| !s.trim().is_empty()) {
                        return Found { synced: item.synced, plain: item.plain };
                    }
                    plain = item.plain;
                }
                break;
            }
            Ok(r) if r.status().as_u16() >= 500 && attempt == 0 => {
                std::thread::sleep(Duration::from_millis(600));
            }
            _ => break,
        }
    }
    // Fuzzy search: prefer synced lyrics whose title, artist and length match.
    let query = format!("{title} {artist}");
    if let Ok(r) = c.get("https://lrclib.net/api/search").query(&[("q", query.as_str())]).send() {
        if let Ok(list) = r.json::<Vec<LrcItem>>() {
            let ok = |x: &LrcItem| similar(&x.track, title) && (artist.is_empty() || similar(&x.artist, artist));
            // Music videos run longer than the song (intros, outros), so
            // accept a looser match when no exact-length one exists.
            let synced = [6.0, 30.0].iter().find_map(|tol| {
                list.iter().find(|x| {
                    ok(x)
                        && close_duration(dur, x.duration.unwrap_or(0.0), *tol)
                        && x.synced.as_deref().is_some_and(|s| !s.trim().is_empty())
                })
            });
            if let Some(x) = synced {
                return Found { synced: x.synced.clone(), plain: x.plain.clone() };
            }
            if plain.is_none() {
                plain = list
                    .iter()
                    .find(|x| ok(x) && x.plain.as_deref().is_some_and(|s| !s.trim().is_empty()))
                    .and_then(|x| x.plain.clone());
            }
        }
    }
    Found { synced: None, plain }
}

/// NetEase Cloud Music has timed lyrics for a lot of what LRCLIB misses,
/// including most Latin and Asian catalogues.
fn netease(c: &reqwest::blocking::Client, title: &str, artist: &str, dur: f64) -> Option<String> {
    let query = format!("{title} {artist}");
    let v: serde_json::Value = c
        .get("https://music.163.com/api/search/get")
        .query(&[("s", query.as_str()), ("type", "1"), ("limit", "8")])
        .header("Referer", "https://music.163.com/")
        .send()
        .ok()?
        .json()
        .ok()?;
    let songs = v.pointer("/result/songs")?.as_array()?;
    let matches = |s: &&serde_json::Value, tol: f64| {
        let name = s.get("name").and_then(|n| n.as_str()).unwrap_or("");
        let artists: Vec<&str> = s
            .get("artists")
            .and_then(|a| a.as_array())
            .map(|a| a.iter().filter_map(|x| x.get("name")?.as_str()).collect())
            .unwrap_or_default();
        let ms = s.get("duration").and_then(|d| d.as_f64()).unwrap_or(0.0);
        similar(name, title)
            && (artist.is_empty() || artists.iter().any(|a| similar(a, artist)))
            && close_duration(dur, ms / 1000.0, tol)
    };
    let pick = songs
        .iter()
        .find(|s| matches(s, 6.0))
        .or_else(|| songs.iter().find(|s| matches(s, 30.0)))?;
    let id = pick.get("id")?.as_i64()?;
    let l: serde_json::Value = c
        .get("https://music.163.com/api/song/lyric")
        .query(&[("id", id.to_string().as_str()), ("lv", "1")])
        .header("Referer", "https://music.163.com/")
        .send()
        .ok()?
        .json()
        .ok()?;
    let text = l.pointer("/lrc/lyric")?.as_str()?;
    // Drop the credit lines NetEase prepends ("作词 : …", "作曲 : …").
    let cleaned: Vec<&str> = text
        .lines()
        .filter(|line| {
            let body = line.rsplit(']').next().unwrap_or("");
            !(body.contains(" : ") || body.contains('：'))
                || !body.chars().any(|ch| ('\u{4e00}'..='\u{9fff}').contains(&ch))
        })
        .collect();
    let lines = parse_lrc(&cleaned.join("\n"));
    if lines.len() < 4 || lines.iter().all(|l| l.t == 0.0) {
        return None;
    }
    Some(cleaned.join("\n"))
}

pub fn fetch(track: &Track) -> Result<Lyrics> {
    let c = client()?;
    // "Passenger | Holes": drop a leading artist name before the song title.
    let mut raw = track.title.as_str();
    for sep in [" | ", " - ", " – "] {
        if let Some((head, tail)) = raw.split_once(sep) {
            if similar(head, &main_artist(&track.artist)) {
                raw = tail;
                break;
            }
        }
    }
    let title = clean_title(raw);
    let artist = main_artist(&track.artist);
    let dur = track.duration;

    let first = lrclib(&c, &title, &artist, dur);
    if first.synced.is_some() {
        return build(first.synced, first.plain);
    }
    if let Some(s) = netease(&c, &title, &artist, dur) {
        return build(Some(s), first.plain);
    }
    // Uploads titled "Artist - Song" without a proper artist field.
    if let Some((a, t)) = track.title.split_once(" - ") {
        let alt = lrclib(&c, &clean_title(t), &main_artist(a), dur);
        if alt.synced.is_some() || (first.plain.is_none() && alt.plain.is_some()) {
            return build(alt.synced, alt.plain);
        }
    }
    build(None, first.plain).map_err(|_| anyhow!("no se encontró la letra"))
}
