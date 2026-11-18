//! On macOS, Homebrew's libmpv lives outside the default linker path
//! (/opt/homebrew on Apple silicon, /usr/local on Intel): ask pkg-config,
//! falling back to the usual prefixes.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let libdir = std::process::Command::new("pkg-config")
        .args(["--variable=libdir", "mpv"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty());
    if let Some(dir) = libdir {
        println!("cargo:rustc-link-search=native={dir}");
    }
    for dir in ["/opt/homebrew/lib", "/usr/local/lib"] {
        if std::path::Path::new(dir).exists() {
            println!("cargo:rustc-link-search=native={dir}");
        }
    }
}
