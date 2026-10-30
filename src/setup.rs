//! First-run assistant (`marmota-music setup`): connects YouTube Music and
//! Spotify step by step. Runs in a terminal; the installer and the
//! "Conectar cuentas" button in the library both open it.

use std::io::{self, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::paths;

const B: &str = "\x1b[1m";
const D: &str = "\x1b[2m";
const G: &str = "\x1b[32m";
const R: &str = "\x1b[31m";
const A: &str = "\x1b[38;5;209m";
const X: &str = "\x1b[0m";

fn ask(q: &str) -> String {
    print!("{q} ");
    let _ = io::stdout().flush();
    let mut s = String::new();
    let _ = io::stdin().read_line(&mut s);
    s.trim().to_string()
}

fn yes(q: &str, default: bool) -> bool {
    let hint = if default { "[S/n]" } else { "[s/N]" };
    let a = ask(&format!("{q} {D}{hint}{X}")).to_lowercase();
    if a.is_empty() {
        default
    } else {
        a.starts_with('s') || a.starts_with('y')
    }
}

fn ok(msg: &str) {
    println!("  {G}✓{X} {msg}");
}
fn bad(msg: &str) {
    println!("  {R}✗{X} {msg}");
}
fn step(n: u8, title: &str) {
    println!("\n{A}{B}{n} · {title}{X}");
}

fn open_url(url: &str) {
    let _ = Command::new("xdg-open")
        .arg(url)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

fn has(cmd: &str) -> bool {
    Command::new("sh")
        .args(["-c", &format!("command -v {cmd}")])
        .stdout(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Browsers with a profile on this machine, as yt-dlp `--cookies-from-browser` specs.
fn browsers() -> Vec<(&'static str, String)> {
    let home = dirs::home_dir().unwrap_or_default();
    let cfg = dirs::config_dir().unwrap_or_default();
    let candidates: [(&str, PathBuf, String); 7] = [
        ("Zen", cfg.join("zen"), format!("firefox:{}", cfg.join("zen").display())),
        ("Firefox", home.join(".mozilla/firefox"), "firefox".into()),
        ("Chromium", cfg.join("chromium"), "chromium".into()),
        ("Google Chrome", cfg.join("google-chrome"), "chrome".into()),
        ("Brave", cfg.join("BraveSoftware/Brave-Browser"), "brave".into()),
        ("Vivaldi", cfg.join("vivaldi"), "vivaldi".into()),
        ("Microsoft Edge", cfg.join("microsoft-edge"), "edge".into()),
    ];
    candidates
        .into_iter()
        .filter(|(_, dir, _)| dir.exists())
        .map(|(name, _, spec)| (name, spec))
        .collect()
}

fn youtube() {
    step(1, "YouTube Music");
    println!("  Sin cuenta ya puedes buscar y escuchar todo. Con tu cuenta, además,");
    println!("  se importan tus listas y tus «Me gusta».");
    if !yes("  ¿Tienes cuenta de YouTube / YouTube Music?", true) {
        println!("  {D}Sin problema: podrás conectarla después desde la Biblioteca.{X}");
        paths::set_config("youtube_account", false.into());
        return;
    }
    let found = browsers();
    if found.is_empty() {
        bad("No encontré navegadores compatibles (Firefox, Zen, Chrome, Brave…).");
        return;
    }
    println!("  ¿En qué navegador tienes la sesión de YouTube iniciada?");
    for (i, (name, _)) in found.iter().enumerate() {
        println!("    {B}{}{X}) {name}", i + 1);
    }
    let pick = ask("  Número:").parse::<usize>().unwrap_or(1).clamp(1, found.len());
    let (name, spec) = &found[pick - 1];
    paths::set_config("cookies_browser", spec.clone().into());

    loop {
        println!("  {D}Leyendo tus listas desde {name}…{X}");
        match crate::yt::account_playlists() {
            Ok(lists) => {
                ok(&format!("Encontré {} listas en tu cuenta", lists.len()));
                if let Ok(db) = crate::db::Db::open() {
                    for (id, title) in &lists {
                        let _ = db.upsert_remote_playlist(title, &format!("VL{id}"), "");
                    }
                    ok("Importadas a tu Biblioteca");
                }
                paths::set_config("youtube_account", true.into());
                return;
            }
            Err(e) => {
                bad(&format!("{e}"));
                println!("  Abre YouTube Music en {name}, inicia sesión y vuelve aquí.");
                open_url("https://music.youtube.com");
                if !yes("  ¿Reintentar?", true) {
                    return;
                }
            }
        }
    }
}

fn spotifyd_conf() {
    let dir = dirs::config_dir().unwrap_or_default().join("spotifyd");
    let file = dir.join("spotifyd.conf");
    if file.exists() {
        return;
    }
    let cache = dirs::cache_dir().unwrap_or_default().join("spotifyd");
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::create_dir_all(&cache);
    let _ = std::fs::write(
        &file,
        format!(
            "[global]\ndevice_name = \"Marmota Music\"\ndevice_type = \"computer\"\n\
             backend = \"pulseaudio\"\nbitrate = 320\ncache_path = \"{}\"\n\
             volume_normalisation = true\ninitial_volume = \"70\"\n\
             use_mpris = true\ndbus_type = \"session\"\n",
            cache.display()
        ),
    );
    ok(&format!("Configuración de spotifyd creada en {}", file.display()));
}

fn spotify_playback() {
    println!("\n  {B}Reproducir desde Spotify{X} {D}(requiere Premium){X}");
    if !yes("  ¿Tu cuenta de Spotify es Premium?", true) {
        println!("  {D}Las canciones de Spotify sonarán desde YouTube Music.{X}");
        return;
    }
    if !has("spotifyd") {
        println!("  Hace falta instalar spotifyd (te pedirá tu contraseña de sudo).");
        if !yes("  ¿Instalarlo ahora?", true) {
            return;
        }
        let st = Command::new("sudo").args(["pacman", "-S", "--needed", "spotifyd"]).status();
        if !st.is_ok_and(|s| s.success()) {
            bad("No se pudo instalar spotifyd");
            return;
        }
    }
    ok("spotifyd instalado");
    spotifyd_conf();
    let creds = dirs::cache_dir().unwrap_or_default().join("spotifyd/oauth/credentials.json");
    if !creds.exists() {
        println!("  Ahora se abrirá Spotify para que inicies sesión y aceptes.");
        let _ = Command::new("spotifyd").arg("authenticate").status();
    }
    if creds.exists() {
        ok("Sesión de Spotify guardada");
    } else {
        bad("No se completó el inicio de sesión (puedes repetirlo con «marmota-music setup»)");
        return;
    }
    let st = Command::new("systemctl").args(["--user", "enable", "--now", "spotifyd"]).status();
    if st.is_ok_and(|s| s.success()) {
        ok("spotifyd arranca solo al iniciar sesión");
    }
}

fn spotify_search() {
    println!("\n  {B}Buscar en Spotify{X}");
    if crate::spotify_api::configured() {
        ok("La búsqueda en Spotify ya está conectada");
        if !yes("  ¿Cambiar las claves?", false) {
            return;
        }
    }
    println!("  Spotify pide una «app» gratuita para buscar en su catálogo (2 minutos):");
    println!("    1. Inicia sesión y pulsa {B}Create app{X} (se abre ahora en el navegador).");
    println!("    2. Nombre: Marmota Music · Redirect URI: {B}http://127.0.0.1:8888/callback{X}");
    println!("    3. Marca {B}Web API{X}, guarda y abre {B}Settings{X}.");
    println!("    4. Copia el Client ID y el Client secret aquí.");
    open_url("https://developer.spotify.com/dashboard/create");
    loop {
        let id = ask("  Client ID:");
        if id.is_empty() {
            println!("  {D}Omitido; puedes hacerlo después.{X}");
            return;
        }
        let secret = ask("  Client secret:");
        paths::set_config("spotify_client_id", id.into());
        paths::set_config("spotify_client_secret", secret.into());
        match crate::spotify_api::search("Bohemian Rhapsody") {
            Ok(t) if !t.is_empty() => {
                ok("Búsqueda en Spotify conectada");
                return;
            }
            Ok(_) => bad("Spotify respondió sin resultados"),
            Err(e) => bad(&format!("{e}")),
        }
        if !yes("  ¿Probar de nuevo?", true) {
            paths::set_config("spotify_client_id", "".into());
            paths::set_config("spotify_client_secret", "".into());
            return;
        }
    }
}

fn spotify() {
    step(2, "Spotify");
    if !yes("  ¿Tienes cuenta de Spotify?", false) {
        println!("  {D}Sin problema: todo funciona con YouTube Music.{X}");
        return;
    }
    spotify_search();
    spotify_playback();
}

pub fn run() {
    println!("\n{A}{B}♪ Marmota Music{X} — conectemos tus cuentas");
    println!("{D}  Responde s/n (Enter elige la opción en mayúscula). Repítelo cuando quieras con «marmota-music setup».{X}");
    youtube();
    spotify();
    paths::set_config("setup_done", true.into());
    step(3, "Listo");
    println!("  Abre el reproductor con {B}Super + Shift + M{X}.");
    // Tell a running notch to pick up the new lists.
    let _ = Command::new("marmota-music")
        .arg("refresh")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    if std::env::var_os("MM_SETUP_WAIT").is_some() {
        ask(&format!("\n  {D}Pulsa Enter para cerrar{X}"));
    }
}

/// Is a YouTube account connected? Existing installs that already picked a
/// browser count as connected unless the user said they have no account.
pub fn accounts_configured() -> bool {
    let c = paths::config();
    let browser = c
        .get("cookies_browser")
        .and_then(|v| v.as_str())
        .is_some_and(|s| !s.is_empty());
    let declined = c.get("youtube_account").and_then(|v| v.as_bool()) == Some(false);
    browser && !declined
}
