//! First-run assistant (`marmota-music setup`): connects YouTube Music and
//! Spotify step by step in a terminal; the installer opens it. The notch
//! does the same from its "Cuentas" view, reusing the helpers here.

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

pub fn open_url(url: &str) {
    // `cmd /C start` would split the URL at every `&`.
    #[cfg(windows)]
    let mut cmd = {
        let mut c = Command::new("rundll32");
        c.arg("url.dll,FileProtocolHandler");
        c
    };
    #[cfg(target_os = "macos")]
    let mut cmd = Command::new("open");
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut cmd = Command::new("xdg-open");
    let _ = cmd
        .arg(url)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

#[cfg(target_os = "linux")]
fn has(cmd: &str) -> bool {
    Command::new("sh")
        .args(["-c", &format!("command -v {cmd}")])
        .stdout(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Browsers with a profile on this machine, as yt-dlp `--cookies-from-browser` specs.
#[cfg(windows)]
pub fn browsers() -> Vec<(&'static str, String)> {
    let roaming = dirs::config_dir().unwrap_or_default();
    let local = dirs::data_local_dir().unwrap_or_default();
    let candidates: [(&str, PathBuf, String); 7] = [
        ("Firefox", roaming.join(r"Mozilla\Firefox"), "firefox".into()),
        ("Zen", roaming.join("zen"), format!("firefox:{}", roaming.join("zen").display())),
        ("Google Chrome", local.join(r"Google\Chrome\User Data"), "chrome".into()),
        ("Microsoft Edge", local.join(r"Microsoft\Edge\User Data"), "edge".into()),
        ("Brave", local.join(r"BraveSoftware\Brave-Browser\User Data"), "brave".into()),
        ("Vivaldi", local.join(r"Vivaldi\User Data"), "vivaldi".into()),
        ("Chromium", local.join(r"Chromium\User Data"), "chromium".into()),
    ];
    candidates
        .into_iter()
        .filter(|(_, dir, _)| dir.exists())
        .map(|(name, _, spec)| (name, spec))
        .collect()
}

/// Browsers with a profile on this machine, as yt-dlp `--cookies-from-browser` specs.
#[cfg(target_os = "macos")]
pub fn browsers() -> Vec<(&'static str, String)> {
    let home = dirs::home_dir().unwrap_or_default();
    let support = home.join("Library/Application Support");
    let candidates: [(&str, PathBuf, String); 8] = [
        ("Safari", home.join("Library/Safari"), "safari".into()),
        ("Firefox", support.join("Firefox"), "firefox".into()),
        ("Zen", support.join("zen"), format!("firefox:{}", support.join("zen").display())),
        ("Google Chrome", support.join("Google/Chrome"), "chrome".into()),
        ("Brave", support.join("BraveSoftware/Brave-Browser"), "brave".into()),
        ("Microsoft Edge", support.join("Microsoft Edge"), "edge".into()),
        ("Vivaldi", support.join("Vivaldi"), "vivaldi".into()),
        ("Chromium", support.join("Chromium"), "chromium".into()),
    ];
    candidates
        .into_iter()
        .filter(|(_, dir, _)| dir.exists())
        .map(|(name, _, spec)| (name, spec))
        .collect()
}

/// Browsers with a profile on this machine, as yt-dlp `--cookies-from-browser` specs.
#[cfg(all(unix, not(target_os = "macos")))]
pub fn browsers() -> Vec<(&'static str, String)> {
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
    #[cfg(target_os = "macos")]
    println!(
        "  {D}Safari necesita «Acceso total al disco» para la Terminal; Chrome, Brave y Edge\n  \
         pedirán permiso al llavero (pulsa «Permitir»).{X}"
    );
    #[cfg(windows)]
    println!(
        "  {D}En Windows, Firefox es el más fiable: Chrome, Edge y Brave cifran sus cookies\n  \
         y yt-dlp a veces no puede leerlas (ciérralos antes de importar).{X}"
    );
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

#[cfg(target_os = "linux")]
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

/// spotifyd plays through MPRIS (D-Bus), which Windows and macOS do not have.
#[cfg(not(target_os = "linux"))]
fn spotify_playback() {
    println!("\n  {B}Reproducir desde Spotify{X}");
    #[cfg(target_os = "macos")]
    println!("  {D}En macOS las canciones de Spotify suenan desde YouTube Music (la búsqueda");
    #[cfg(windows)]
    println!("  {D}En Windows las canciones de Spotify suenan desde YouTube Music (la búsqueda");
    println!("  en Spotify funciona igual).{X}");
}

#[cfg(target_os = "linux")]
fn spotify_playback() {
    println!("\n  {B}Reproducir desde Spotify{X} {D}(requiere Premium){X}");
    let creds = dirs::cache_dir().unwrap_or_default().join("spotifyd/oauth/credentials.json");
    if creds.exists() && has("spotifyd") {
        ok("Sesión de Spotify ya guardada en este equipo");
        let _ = Command::new("systemctl").args(["--user", "enable", "--now", "spotifyd"]).status();
        return;
    }
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
    let creds = dirs::cache_dir().unwrap_or_default().join("spotifyd/oauth/credentials.json");
    if crate::spotify_api::configured() && creds.exists() {
        ok("Spotify ya está conectado (búsqueda y reproducción)");
        return;
    }
    if !crate::spotify_api::configured() && !yes("  ¿Tienes cuenta de Spotify?", false) {
        println!("  {D}Sin problema: todo funciona con YouTube Music.{X}");
        return;
    }
    spotify_search();
    spotify_playback();
}

/// A path typed or dragged into the terminal: quotes and `\ ` escapes off.
fn clean_path(s: &str) -> PathBuf {
    let t = s.trim().trim_matches(|c| c == '\'' || c == '"');
    let home = dirs::home_dir().unwrap_or_default();
    let t = t.replace("\\ ", " ");
    match t.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None => PathBuf::from(t),
    }
}

/// Apple Music catalog search with a MusicKit key from Apple Developer.
pub fn apple() {
    println!("  {D}Busca en todo el catálogo de Apple Music; requiere una cuenta de Apple Developer.{X}");
    if crate::apple_api::configured() {
        ok(&format!("La búsqueda en Apple Music ya está conectada (país: {})", crate::apple_api::storefront()));
        if !yes("  ¿Cambiar la clave?", false) {
            apple_playback();
            return;
        }
    } else if !yes("  ¿Tienes una cuenta de Apple Developer?", false) {
        println!("  {D}Sin problema: puedes conectarla después desde la Biblioteca.{X}");
        return;
    }
    println!("  La búsqueda usa una clave MusicKit de tu cuenta (se abre ahora el portal):");
    println!("    1. {B}Identifiers{X} → {B}+{X} → {B}Media IDs{X}: descripción «Marmota Music»,");
    println!("       identificador {B}media.dev.marmota.music{X}, marca {B}MusicKit{X} y regístralo.");
    println!("    2. {B}Keys{X} → {B}+{X}: nombre «Marmota Music», marca {B}Media Services{X}, pulsa");
    println!("       {B}Configure{X}, elige el Media ID del paso 1 y registra la clave.");
    println!("    3. Descarga el archivo {B}AuthKey_XXXXXXXXXX.p8{X} (solo se puede una vez) y anota");
    println!("       el {B}Key ID{X}. El {B}Team ID{X} está en {B}Membership details{X}.");
    println!("  {D}La clave se guarda solo en este equipo; no la compartas con nadie.{X}");
    open_url("https://developer.apple.com/account/resources/authkeys/list");
    loop {
        let team = ask("  Team ID:").to_ascii_uppercase();
        if team.is_empty() {
            println!("  {D}Omitido; puedes hacerlo después.{X}");
            return;
        }
        let key_id = ask("  Key ID:").to_ascii_uppercase();
        let file = clean_path(&ask("  Archivo .p8 (puedes arrastrarlo a esta ventana):"));
        let pem = match std::fs::read_to_string(&file) {
            Ok(p) if p.contains("PRIVATE KEY") => p,
            _ => {
                bad(&format!("No pude leer una clave en {}", file.display()));
                if yes("  ¿Probar de nuevo?", true) {
                    continue;
                }
                return;
            }
        };
        // Keep a private copy next to the library: the download folder
        // gets cleaned up.
        let dest = paths::data_dir().join("apple-musickit.p8");
        if let Err(e) = save_private(&dest, &pem) {
            bad(&format!("No pude guardar la clave: {e}"));
            return;
        }
        let guess = crate::apple_api::storefront();
        let sf = ask(&format!("  País de tu cuenta de Apple Music (2 letras) {D}[{guess}]{X}:")).to_ascii_lowercase();
        let sf = if sf.len() == 2 { sf } else { guess };
        paths::set_config("apple_team_id", team.into());
        paths::set_config("apple_key_id", key_id.into());
        paths::set_config("apple_key_file", dest.display().to_string().into());
        paths::set_config("apple_storefront", sf.into());
        crate::apple_api::reset();
        match crate::apple_api::search("Bohemian Rhapsody") {
            Ok(t) if !t.is_empty() => {
                ok("Búsqueda en Apple Music conectada");
                apple_playback();
                return;
            }
            Ok(_) => bad("Apple Music respondió sin resultados (¿el país es correcto?)"),
            Err(e) => bad(&format!("{e}")),
        }
        if !yes("  ¿Probar de nuevo?", true) {
            for k in ["apple_team_id", "apple_key_id", "apple_key_file"] {
                paths::set_config(k, "".into());
            }
            let _ = std::fs::remove_file(&dest);
            return;
        }
    }
}

/// Ask macOS for access to Apple Music, so songs play with the user's
/// subscription instead of from YouTube Music.
fn apple_playback() {
    #[cfg(target_os = "macos")]
    {
        if crate::apple_music::helper_path().is_none() {
            println!("  {D}Sus canciones suenan desde YouTube Music: el ayudante de Apple Music no está instalado.{X}");
            println!("  {D}Instálalo con ./install-mac.sh (necesita macOS 14 y tu Team ID).{X}");
            return;
        }
        if std::env::var_os("MARMOTA_SKIP_APPLE_CHECK").is_some() {
            // install-mac.sh signs the helper and checks it right after.
            return;
        }
        println!("  Para reproducir con tu suscripción, macOS pedirá acceso a Apple Music.");
        match crate::apple_music::check(&["--check"]) {
            Some(true) => ok("Reproducción con tu suscripción de Apple Music lista"),
            Some(false) => println!("  {D}Mientras tanto, sus canciones suenan desde YouTube Music.{X}"),
            None => bad("El ayudante de Apple Music no respondió"),
        }
    }
    #[cfg(not(target_os = "macos"))]
    println!("  {D}Sus canciones suenan desde YouTube Music (la reproducción con Apple Music es solo para macOS).{X}");
}

/// Write a secret readable by this user only.
fn save_private(path: &std::path::Path, text: &str) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut f = std::fs::File::create(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    f.write_all(text.as_bytes())
}

/// Keyboard shortcut that opens the notch on this platform.
pub fn shortcut() -> String {
    #[cfg(windows)]
    return crate::win::hotkey_label();
    #[cfg(target_os = "macos")]
    return crate::mac::hotkey_label();
    #[cfg(all(unix, not(target_os = "macos")))]
    "Super + Shift + M".into()
}

pub fn run(only: Option<&str>) {
    #[cfg(windows)]
    crate::win::enable_ansi();
    match only {
        Some("spotify") => {
            spotify_playback();
            return;
        }
        Some("youtube") => {
            youtube();
            return;
        }
        Some("apple") => {
            println!("\n{A}{B}♪ Marmota Music{X} — Apple Music");
            apple();
            notify_notch();
            if std::env::var_os("MM_SETUP_WAIT").is_some() {
                ask(&format!("\n  {D}Pulsa Enter para cerrar{X}"));
            }
            return;
        }
        _ => {}
    }
    println!("\n{A}{B}♪ Marmota Music{X} — conectemos tus cuentas");
    println!("{D}  Responde s/n (Enter elige la opción en mayúscula). Repítelo cuando quieras con «marmota-music setup».{X}");
    youtube();
    spotify();
    step(3, "Apple Music");
    apple();
    paths::set_config("setup_done", true.into());
    step(4, "Listo");
    println!("  Abre el reproductor con {B}{}{X}.", shortcut());
    notify_notch();
    if std::env::var_os("MM_SETUP_WAIT").is_some() {
        ask(&format!("\n  {D}Pulsa Enter para cerrar{X}"));
    }
}

/// Tell a running notch to pick up new lists and keys.
fn notify_notch() {
    let exe = std::env::current_exe().unwrap_or_else(|_| "marmota-music".into());
    let _ = Command::new(exe)
        .arg("refresh")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
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
    (browser || paths::cookies_file().is_some()) && !declined
}

/// Display name of the browser whose session is configured, if detected
/// (none when signed in with Google from the notch).
pub fn connected_browser() -> Option<&'static str> {
    if paths::cookies_file().is_some() {
        return None;
    }
    let spec = paths::config().get("cookies_browser")?.as_str()?.to_string();
    browsers().into_iter().find(|(_, s)| *s == spec).map(|(name, _)| name)
}
