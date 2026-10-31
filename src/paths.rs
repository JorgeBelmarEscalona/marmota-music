use std::path::PathBuf;

pub fn data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("marmota-music")
}

pub fn cache_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("marmota-music")
}

pub fn art_path(id: &str) -> PathBuf {
    cache_dir().join("art").join(format!("{}.img", id))
}

pub fn runtime_dir() -> PathBuf {
    dirs::runtime_dir().unwrap_or_else(std::env::temp_dir)
}

/// Control socket of the daemon. Windows has no Unix sockets in std, so there
/// the daemon listens on a loopback TCP port and this file holds the port.
pub fn socket() -> PathBuf {
    if cfg!(windows) {
        runtime_dir().join("marmota-music.port")
    } else {
        runtime_dir().join("marmota-music.sock")
    }
}

/// mpv's JSON IPC endpoint: a Unix socket, or a named pipe on Windows.
pub fn mpv_socket() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(r"\\.\pipe\marmota-music-mpv")
    } else {
        runtime_dir().join("marmota-music-mpv.sock")
    }
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
        .unwrap_or_else(std::env::temp_dir)
        .join("marmota-music")
        .join("config.json")
}

/// The whole config file (empty object if missing or invalid).
pub fn config() -> serde_json::Value {
    std::fs::read_to_string(config_file())
        .ok()
        // Windows editors may save it with a byte-order mark.
        .and_then(|t| serde_json::from_str(t.trim_start_matches('\u{feff}')).ok())
        .unwrap_or_else(|| serde_json::json!({}))
}

/// Set one key in the config file, keeping the rest.
pub fn set_config(key: &str, value: serde_json::Value) {
    let mut cfg = config();
    if let Some(obj) = cfg.as_object_mut() {
        obj.insert(key.to_string(), value);
    }
    let path = config_file();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, serde_json::to_string_pretty(&cfg).unwrap_or_default());
}

/// cookies.txt saved by "Iniciar sesión con Google", if there is one.
pub fn cookies_file() -> Option<PathBuf> {
    let path = config().get("cookies_file")?.as_str().filter(|s| !s.is_empty())?.to_string();
    Some(PathBuf::from(path)).filter(|p| p.is_file())
}

/// How yt-dlp gets the user's YouTube session: the signed-in cookies.txt,
/// or else the browser's own cookie store.
pub fn cookie_args() -> [String; 2] {
    match cookies_file() {
        Some(file) => ["--cookies".into(), file.display().to_string()],
        None => ["--cookies-from-browser".into(), cookies_browser()],
    }
}

/// Browser whose YouTube session yt-dlp borrows to import the user's
/// playlists (`--cookies-from-browser` syntax). Defaults to the system
/// browser on this machine (Zen, a Firefox fork).
pub fn cookies_browser() -> String {
    let mut cfg = config();
    if let Some(b) = cfg.get("cookies_browser").and_then(|b| b.as_str()) {
        return b.to_string();
    }
    let zen = dirs::config_dir().map(|d| d.join("zen")).filter(|d| d.exists());
    let default = match zen {
        Some(dir) => format!("firefox:{}", dir.display()),
        None => "firefox".to_string(),
    };
    // Write the default next to whatever else is configured.
    if let Some(obj) = cfg.as_object_mut() {
        obj.insert("cookies_browser".into(), serde_json::Value::String(default.clone()));
        obj.entry("spotify_client_id").or_insert_with(|| "".into());
        obj.entry("spotify_client_secret").or_insert_with(|| "".into());
    }
    let path = config_file();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
        let _ = std::fs::write(&path, serde_json::to_string_pretty(&cfg).unwrap_or_default());
    }
    default
}
