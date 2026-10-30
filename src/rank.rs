//! Order mixed YouTube Music + Spotify results the way the user asked:
//! song title match first, then artist, then whether it has a video, then
//! audio quality. The source's own relevance order breaks the remaining ties.

use crate::lyrics::main_artist;
use crate::model::Track;

fn tokens(s: &str) -> Vec<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| {
            w.chars()
                .map(|c| match c {
                    'á' | 'à' | 'ä' => 'a',
                    'é' | 'è' | 'ë' => 'e',
                    'í' | 'ì' | 'ï' => 'i',
                    'ó' | 'ò' | 'ö' => 'o',
                    'ú' | 'ù' | 'ü' => 'u',
                    'ñ' => 'n',
                    c => c,
                })
                .collect()
        })
        .collect()
}

fn coverage(words: &[String], query: &[String]) -> f64 {
    if words.is_empty() {
        return 0.0;
    }
    words.iter().filter(|w| query.contains(w)).count() as f64 / words.len() as f64
}

const DERIVATIVE: [&str; 15] = [
    "karaoke", "cover", "remix", "live", "vivo", "instrumental", "8d", "slowed",
    "sped", "reverb", "nightcore", "acapella", "tribute", "originally", "piano",
];

/// The song name alone: no "Artist - " / "Artist | " prefix, nothing in
/// brackets, no "ft. …".
fn core_title(title: &str, artist: &str) -> String {
    let main = main_artist(artist);
    let mut t = title;
    for sep in [" - ", " | ", " – ", ": "] {
        if let Some((head, tail)) = t.split_once(sep) {
            let h = tokens(head);
            if !h.is_empty() && (coverage(&h, &tokens(&main)) >= 0.999 || coverage(&tokens(&main), &h) >= 0.999) {
                t = tail;
                break;
            }
        }
    }
    let mut out = String::new();
    let mut depth = 0;
    for c in t.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = (depth - 1).max(0),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    // Keep " | " segments: uploads that cram tags into the title should
    // score lower, not be cut down to a perfect match.
    let lower = out.to_lowercase();
    let cut = [" ft. ", " ft ", " feat. ", " feat "]
        .iter()
        .filter_map(|s| lower.find(s))
        .min()
        .unwrap_or(out.len());
    out[..cut].trim().to_string()
}

/// (title, artist, video, quality) — higher is better.
fn key(t: &Track, q: &[String]) -> (u8, u8, u8, u8) {
    let title_words = tokens(&core_title(&t.title, &t.artist));
    let full_title = tokens(&t.title);
    let cov = coverage(&title_words, q);
    let mut title: u8 = if cov >= 0.999 {
        3
    } else if cov >= 0.5 {
        2
    } else if cov > 0.0 {
        1
    } else {
        0
    };
    // "Song (Karaoke Version)" when the user didn't ask for a karaoke.
    if full_title.iter().any(|w| DERIVATIVE.contains(&w.as_str()) && !q.contains(w)) {
        title = title.saturating_sub(2);
    }
    let artist_words = tokens(&main_artist(&t.artist));
    let a = coverage(&artist_words, q);
    let leftover = q.iter().any(|w| !title_words.contains(w));
    let artist = if a >= 0.999 {
        2
    } else if a > 0.0 || !leftover {
        // Nothing beyond the title was typed: every artist is a fair match.
        1
    } else {
        0
    };
    (title, artist, t.video as u8, t.quality)
}

/// Merge both sources and sort. Each list is already in its own relevance
/// order, which is kept as the final tiebreaker.
pub fn merge(query: &str, youtube: Vec<Track>, spotify: Vec<Track>) -> Vec<Track> {
    let q = tokens(query);
    let mut all: Vec<(usize, Track)> = youtube
        .into_iter()
        .enumerate()
        .chain(spotify.into_iter().enumerate())
        .collect();
    all.sort_by(|(ia, a), (ib, b)| key(b, &q).cmp(&key(a, &q)).then(ia.cmp(ib)));
    all.into_iter().map(|(_, t)| t).collect()
}

pub fn debug_key(query: &str, t: &Track) -> (u8, u8, u8, u8) {
    key(t, &tokens(query))
}
