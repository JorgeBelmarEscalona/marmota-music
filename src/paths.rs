use std::path::PathBuf;

pub fn data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("marmota-music")
}

pub fn cache_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("marmota-music")
}

pub fn art_path(id: &str) -> PathBuf {
    cache_dir().join("art").join(format!("{}.img", id))
}

pub fn runtime_dir() -> PathBuf {
    dirs::runtime_dir().unwrap_or_else(|| PathBuf::from("/tmp"))
}

pub fn socket() -> PathBuf {
    runtime_dir().join("marmota-music.sock")
}

pub fn mpv_socket() -> PathBuf {
    runtime_dir().join("marmota-music-mpv.sock")
}

pub fn ensure_dirs() {
    let _ = std::fs::create_dir_all(data_dir());
    let _ = std::fs::create_dir_all(cache_dir().join("art"));
    let _ = std::fs::create_dir_all(cache_dir().join("thumbs"));
}

pub fn thumb_path(url: &str) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    url.hash(&mut h);
    cache_dir().join("thumbs").join(format!("{:016x}.img", h.finish()))
}

pub fn config_file() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("marmota-music")
        .join("config.json")
}

/// Browser whose YouTube session yt-dlp borrows to import the user's
/// playlists (`--cookies-from-browser` syntax). Defaults to the system
/// browser on this machine (Zen, a Firefox fork).
pub fn cookies_browser() -> String {
    let path = config_file();
    if let Ok(text) = std::fs::read_to_string(&path) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
            if let Some(b) = v.get("cookies_browser").and_then(|b| b.as_str()) {
                return b.to_string();
            }
        }
    }
    let zen = dirs::config_dir().map(|d| d.join("zen")).filter(|d| d.exists());
    let default = match zen {
        Some(dir) => format!("firefox:{}", dir.display()),
        None => "firefox".to_string(),
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
        let _ = std::fs::write(
            &path,
            serde_json::to_string_pretty(&serde_json::json!({ "cookies_browser": default }))
                .unwrap_or_default(),
        );
    }
    default
}
