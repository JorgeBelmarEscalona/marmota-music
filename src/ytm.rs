//! Tiny YouTube Music (InnerTube) client: search, home shelves, albums and
//! playlists, and the "radio" that keeps the queue going.
//!
//! The responses are deeply nested and change shape often, so instead of
//! following exact paths this walks the JSON for the renderers that describe
//! a song or a collection and reads them defensively.

use std::time::Duration;

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use crate::model::{CollKind, Collection, Track};

const BASE: &str = "https://music.youtube.com/youtubei/v1/";
const CLIENT_VERSION: &str = "1.20260801.01.00";

const ITEM_KEYS: [&str; 3] = [
    "musicResponsiveListItemRenderer",
    "musicTwoRowItemRenderer",
    "playlistPanelVideoRenderer",
];

fn call(endpoint: &str, mut body: Value) -> Result<Value> {
    body["context"] = json!({
        "client": {
            "clientName": "WEB_REMIX",
            "clientVersion": CLIENT_VERSION,
            "hl": "es",
            "gl": "CL",
        }
    });
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0 Safari/537.36")
        .build()?;
    let resp = client
        .post(format!("{BASE}{endpoint}?prettyPrint=false"))
        .header("Origin", "https://music.youtube.com")
        .header("Referer", "https://music.youtube.com/")
        .json(&body)
        .send()?;
    if !resp.status().is_success() {
        return Err(anyhow!("YouTube Music respondió {}", resp.status()));
    }
    Ok(resp.json()?)
}

// ------------------------------------------------------------------ walking

fn walk<'a>(v: &'a Value, keys: &[&'a str], out: &mut Vec<(&'a str, &'a Value)>) {
    match v {
        Value::Object(map) => {
            for (k, val) in map {
                if let Some(key) = keys.iter().find(|x| **x == k.as_str()) {
                    out.push((key, val));
                } else {
                    walk(val, keys, out);
                }
            }
        }
        Value::Array(arr) => {
            for val in arr {
                walk(val, keys, out);
            }
        }
        _ => {}
    }
}

fn find<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    match v {
        Value::Object(map) => {
            if let Some(x) = map.get(key) {
                return Some(x);
            }
            map.values().find_map(|val| find(val, key))
        }
        Value::Array(arr) => arr.iter().find_map(|val| find(val, key)),
        _ => None,
    }
}

fn runs(v: Option<&Value>) -> Vec<&Value> {
    v.and_then(|t| t.get("runs"))
        .and_then(|r| r.as_array())
        .map(|a| a.iter().collect())
        .unwrap_or_default()
}

fn run_text(r: &Value) -> &str {
    r.get("text").and_then(|t| t.as_str()).unwrap_or("")
}

fn join(rs: &[&Value]) -> String {
    rs.iter().map(|r| run_text(r)).collect()
}

fn page_type(v: &Value) -> Option<&str> {
    v.get("navigationEndpoint")?
        .get("browseEndpoint")?
        .get("browseEndpointContextSupportedConfigs")?
        .get("browseEndpointContextMusicConfig")?
        .get("pageType")?
        .as_str()
}

fn browse_id(v: &Value) -> Option<&str> {
    v.get("navigationEndpoint")?
        .get("browseEndpoint")?
        .get("browseId")?
        .as_str()
}

fn best_thumb(v: &Value) -> String {
    let Some(list) = find(v, "thumbnails").and_then(|t| t.as_array()) else {
        return String::new();
    };
    list.iter()
        .filter_map(|t| {
            let url = t.get("url")?.as_str()?;
            let w = t.get("width").and_then(|w| w.as_u64()).unwrap_or(0);
            Some((w, url))
        })
        .max_by_key(|(w, _)| *w)
        .map(|(_, u)| big_art(u))
        .unwrap_or_default()
}

/// googleusercontent thumbnails carry their size in the URL; ask for a cover
/// sized for the big artwork instead of the 60 px list icon.
pub fn big_art(url: &str) -> String {
    sized(url, 544)
}

pub fn small_art(url: &str) -> String {
    if url.contains("i.ytimg.com/vi/") {
        if let Some(id) = url.split("/vi/").nth(1).and_then(|r| r.split('/').next()) {
            return format!("https://i.ytimg.com/vi/{id}/mqdefault.jpg");
        }
    }
    sized(url, 120)
}

fn sized(url: &str, px: u32) -> String {
    if url.contains("googleusercontent.com") || url.contains("ggpht.com") {
        if let Some(i) = url.rfind('=') {
            return format!("{}=w{px}-h{px}-l90-rj", &url[..i]);
        }
    }
    url.to_string()
}

fn parse_duration(s: &str) -> Option<f64> {
    let parts: Vec<&str> = s.trim().split(':').collect();
    if parts.len() < 2 || parts.len() > 3 {
        return None;
    }
    let mut total = 0.0;
    for p in parts {
        if p.is_empty() || !p.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        total = total * 60.0 + p.parse::<f64>().ok()?;
    }
    Some(total)
}

const TYPE_WORDS: [&str; 14] = [
    "Canción", "Video", "Vídeo", "Álbum", "Sencillo", "EP", "Lista de reproducción",
    "Artista", "Episodio", "Podcast", "Song", "Album", "Single", "Playlist",
];

fn is_stat(seg: &str) -> bool {
    let l = seg.to_lowercase();
    l.contains("reproducci")
        || l.contains("visualizaci")
        || l.contains("usuarios")
        || l.contains("views")
        || l.contains("plays")
        || l.contains("canciones")
        || parse_duration(seg).is_some()
        || (seg.len() == 4 && seg.chars().all(|c| c.is_ascii_digit()))
}

/// Subtitle segments separated by " • " minus the item type, stats, years
/// and durations.
fn meta_segments(sub: &str) -> Vec<String> {
    sub.split(" • ")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && !TYPE_WORDS.contains(&s.as_str()) && !is_stat(s))
        .collect()
}

enum Item {
    Song(Track),
    Coll(Collection),
}

fn parse_item(kind: &str, r: &Value) -> Option<Item> {
    let (title, sub): (String, Vec<&Value>) = match kind {
        "musicResponsiveListItemRenderer" => {
            let cols: Vec<Vec<&Value>> = r
                .get("flexColumns")
                .and_then(|c| c.as_array())
                .map(|a| {
                    a.iter()
                        .map(|c| runs(c.get("musicResponsiveListItemFlexColumnRenderer").and_then(|x| x.get("text"))))
                        .collect()
                })
                .unwrap_or_default();
            let fixed: Vec<&Value> = r
                .get("fixedColumns")
                .and_then(|c| c.as_array())
                .map(|a| {
                    a.iter()
                        .flat_map(|c| runs(c.get("musicResponsiveListItemFixedColumnRenderer").and_then(|x| x.get("text"))))
                        .collect()
                })
                .unwrap_or_default();
            let title = cols.first().map(|c| join(c)).unwrap_or_default();
            let mut sub: Vec<&Value> = Vec::new();
            for (i, c) in cols.iter().enumerate().skip(1) {
                if i > 1 && !c.is_empty() {
                    sub.push(&*SEP);
                }
                sub.extend(c.iter().copied());
            }
            if !fixed.is_empty() {
                sub.push(&*SEP);
                sub.extend(fixed);
            }
            (title, sub)
        }
        "musicTwoRowItemRenderer" => (join(&runs(r.get("title"))), runs(r.get("subtitle"))),
        _ => {
            let mut sub = runs(r.get("longBylineText"));
            let len = runs(r.get("lengthText"));
            if !len.is_empty() {
                sub.push(&*SEP);
                sub.extend(len);
            }
            (join(&runs(r.get("title"))), sub)
        }
    };
    if title.trim().is_empty() {
        return None;
    }
    let sub_text = join(&sub);
    let art = best_thumb(r.get("thumbnail").or_else(|| r.get("thumbnailRenderer")).unwrap_or(r));

    // Collections: albums and playlists link to a browse page.
    let mut ptype = page_type(r).map(str::to_string);
    let mut bid = browse_id(r).map(str::to_string);
    if ptype.is_none() {
        for t in runs(r.get("title")) {
            if let (Some(p), Some(b)) = (page_type(t), browse_id(t)) {
                ptype = Some(p.to_string());
                bid = Some(b.to_string());
            }
        }
    }
    if let (Some(p), Some(b)) = (ptype.as_deref(), bid.clone()) {
        let kind = match p {
            "MUSIC_PAGE_TYPE_ALBUM" => CollKind::Album,
            "MUSIC_PAGE_TYPE_PLAYLIST" => CollKind::Playlist,
            _ => return None, // artists, podcasts, channels
        };
        let segs = meta_segments(&sub_text);
        return Some(Item::Coll(Collection {
            id: b,
            title,
            subtitle: if segs.is_empty() { sub_text.clone() } else { segs.join(" · ") },
            art,
            kind,
        }));
    }

    let video_id = r
        .get("videoId")
        .or_else(|| r.get("playlistItemData").and_then(|p| p.get("videoId")))
        .or_else(|| find(r, "videoId"))?
        .as_str()?
        .to_string();

    let artists: Vec<&str> = sub
        .iter()
        .filter(|x| {
            matches!(
                page_type(x),
                Some("MUSIC_PAGE_TYPE_ARTIST") | Some("MUSIC_PAGE_TYPE_USER_CHANNEL")
            )
        })
        .map(|x| run_text(x))
        .collect();
    let album = sub
        .iter()
        .find(|x| page_type(x) == Some("MUSIC_PAGE_TYPE_ALBUM"))
        .map(|x| run_text(x).to_string())
        .unwrap_or_default();
    let duration = sub
        .iter()
        .find_map(|x| parse_duration(run_text(x)))
        .unwrap_or(0.0);
    let artist = if !artists.is_empty() {
        artists.join(", ")
    } else {
        meta_segments(&sub_text)
            .into_iter()
            .find(|s| *s != album)
            .unwrap_or_default()
    };

    Some(Item::Song(Track {
        id: video_id.clone(),
        title,
        artist,
        album,
        duration,
        art: if art.is_empty() {
            format!("https://i.ytimg.com/vi/{video_id}/hqdefault.jpg")
        } else {
            art
        },
        source: "youtube".into(),
    }))
}

static SEP: std::sync::LazyLock<Value> = std::sync::LazyLock::new(|| json!({ "text": " • " }));

fn items(v: &Value) -> (Vec<Track>, Vec<Collection>) {
    let mut found = Vec::new();
    walk(v, &ITEM_KEYS, &mut found);
    let mut tracks: Vec<Track> = Vec::new();
    let mut colls: Vec<Collection> = Vec::new();
    for (k, r) in found {
        match parse_item(k, r) {
            Some(Item::Song(t)) => {
                if !tracks.iter().any(|x| x.id == t.id) {
                    tracks.push(t);
                }
            }
            Some(Item::Coll(c)) => {
                if !colls.iter().any(|x| x.id == c.id) {
                    colls.push(c);
                }
            }
            None => {}
        }
    }
    (tracks, colls)
}

// ---------------------------------------------------------------- public API

pub fn search(query: &str) -> Result<(Vec<Track>, Vec<Collection>)> {
    let q = query.trim();
    if q.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let all = call("search", json!({ "query": q }))?;
    let (mut tracks, mut colls) = items(&all);
    // Songs inside the "top result" card belong to the card's artist, which
    // their own subtitles leave out.
    let mut cards = Vec::new();
    walk(&all, &["musicCardShelfRenderer"], &mut cards);
    for (_, card) in cards {
        let owner = join(&runs(card.get("title")));
        if owner.is_empty() {
            continue;
        }
        for t in items(card).0 {
            if let Some(x) = tracks.iter_mut().find(|x| x.id == t.id && x.artist.is_empty()) {
                x.artist = owner.clone();
            }
        }
    }
    // The mixed results only carry a handful of songs; top up from the
    // songs-only filter so the list is worth scrolling.
    if let Ok(songs) = call("search", json!({ "query": q, "params": "EgWKAQIIAWoKEAkQBRAKEAMQBA%3D%3D" })) {
        for t in items(&songs).0 {
            match tracks.iter_mut().find(|x| x.id == t.id) {
                // The "top result" shelf often omits the artist and album.
                Some(x) => {
                    if x.artist.is_empty() {
                        x.artist = t.artist;
                    }
                    if x.album.is_empty() {
                        x.album = t.album;
                    }
                    if x.duration <= 0.0 {
                        x.duration = t.duration;
                    }
                }
                None => tracks.push(t),
            }
        }
    }
    if colls.iter().filter(|c| c.kind == CollKind::Playlist).count() < 4 {
        if let Ok(pl) = call("search", json!({ "query": q, "params": "EgeKAQQoAEABagoQAxAEEAkQChAF" })) {
            for c in items(&pl).1 {
                if !colls.iter().any(|x| x.id == c.id) {
                    colls.push(c);
                }
            }
        }
    }
    tracks.truncate(40);
    colls.truncate(16);
    Ok((tracks, colls))
}

/// Shelves from the YouTube Music home page: featured playlists, albums and
/// "quick picks" songs.
pub fn home() -> Result<(Vec<Collection>, Vec<Track>)> {
    let v = call("browse", json!({ "browseId": "FEmusic_home" }))?;
    let (tracks, colls) = items(&v);
    Ok((colls, tracks))
}

fn header_info(v: &Value) -> (String, String, String) {
    let mut found = Vec::new();
    walk(
        v,
        &[
            "musicResponsiveHeaderRenderer",
            "musicDetailHeaderRenderer",
            "musicEditablePlaylistDetailHeaderRenderer",
        ],
        &mut found,
    );
    let Some((_, h)) = found.first() else {
        return Default::default();
    };
    let title = join(&runs(h.get("title")));
    let artist = {
        let s = join(&runs(h.get("straplineTextOne")));
        if s.is_empty() {
            meta_segments(&join(&runs(h.get("subtitle")))).join(", ")
        } else {
            s
        }
    };
    let art = best_thumb(h.get("thumbnail").unwrap_or(h));
    (title, artist, art)
}

fn continuation(v: &Value) -> Option<String> {
    if let Some(t) = find(v, "continuationCommand").and_then(|c| c.get("token")) {
        return t.as_str().map(str::to_string);
    }
    find(v, "nextContinuationData")
        .and_then(|c| c.get("continuation"))
        .and_then(|t| t.as_str())
        .map(str::to_string)
}

/// Tracks of an album (`MPREb_…`) or playlist (`VL…`), plus its title.
pub fn collection(browse_id: &str) -> Result<(String, Vec<Track>)> {
    if let Some(mix) = browse_id.strip_prefix("RDAMVM") {
        return Ok((String::new(), radio(mix)?));
    }
    let first = call("browse", json!({ "browseId": browse_id }))?;
    let (title, artist, art) = header_info(&first);
    let is_album = browse_id.starts_with("MPRE");
    // Albums list related releases after the tracks; keep only the main list.
    let main = first
        .pointer("/contents/twoColumnBrowseResultsRenderer")
        .map(|two| {
            if is_album {
                two.get("tabs").unwrap_or(two)
            } else {
                two.get("secondaryContents").unwrap_or(two)
            }
        })
        .unwrap_or(&first);
    let (mut tracks, _) = items(main);
    if tracks.is_empty() {
        tracks = items(&first).0;
    }

    let mut token = continuation(main);
    let mut pages = 0;
    while let Some(t) = token.take() {
        pages += 1;
        if pages > 6 {
            break;
        }
        let Ok(more) = call("browse", json!({ "continuation": t })) else { break };
        let (extra, _) = items(&more);
        if extra.is_empty() {
            break;
        }
        for x in extra {
            if !tracks.iter().any(|y| y.id == x.id) {
                tracks.push(x);
            }
        }
        token = continuation(&more);
    }

    for t in tracks.iter_mut() {
        if is_album {
            if t.artist.is_empty() {
                t.artist = artist.clone();
            }
            if t.album.is_empty() {
                t.album = title.clone();
            }
            if !art.is_empty() && t.art.contains("i.ytimg.com") {
                t.art = art.clone();
            }
        }
    }
    Ok((title, tracks))
}

/// YouTube Music's "radio" for a song: what plays next when the queue ends.
pub fn radio(video_id: &str) -> Result<Vec<Track>> {
    let v = call(
        "next",
        json!({
            "videoId": video_id,
            "playlistId": format!("RDAMVM{video_id}"),
            "isAudioOnly": true,
        }),
    )?;
    let mut found = Vec::new();
    walk(&v, &["playlistPanelVideoRenderer"], &mut found);
    let mut out: Vec<Track> = Vec::new();
    for (k, r) in found {
        if let Some(Item::Song(t)) = parse_item(k, r) {
            if !out.iter().any(|x| x.id == t.id) {
                out.push(t);
            }
        }
    }
    Ok(out)
}

/// The official music video for a song, when YouTube Music has one. Songs
/// ("Art tracks") only carry the cover as their picture.
pub fn music_video(title: &str, artist: &str) -> Option<String> {
    let clean = crate::lyrics::clean_title(title);
    let main = crate::lyrics::main_artist(artist);
    let q = format!("{clean} {main}");
    let v = call("search", json!({ "query": q, "params": "EgWKAQIQAWoKEAkQChAFEAMQBA%3D%3D" })).ok()?;
    let (tracks, _) = items(&v);
    tracks
        .into_iter()
        .take(6)
        .find(|t| {
            crate::lyrics::similar(&crate::lyrics::clean_title(&t.title), &clean)
                && (main.is_empty() || crate::lyrics::similar(&t.artist, &main) || crate::lyrics::similar(&t.title, &main))
        })
        .map(|t| t.id)
}

/// Playlist id from a pasted YouTube / YouTube Music link.
pub fn playlist_from_url(text: &str) -> Option<String> {
    let t = text.trim();
    if !(t.starts_with("http") || t.contains("youtube.com") || t.contains("youtu.be")) {
        return None;
    }
    let list = t.split(['?', '&']).find_map(|p| p.strip_prefix("list="))?;
    let id: String = list
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .collect();
    if id.is_empty() {
        None
    } else {
        Some(id)
    }
}
