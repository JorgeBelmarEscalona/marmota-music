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
}
