#!/usr/bin/env bash
# Marmota Music installer for macOS (Apple silicon and Intel).
#   ./install-mac.sh            install or update
#   ./install-mac.sh --setup    also re-run the account assistant
set -euo pipefail
cd "$(dirname "$0")"

B=$'\e[1m'; D=$'\e[2m'; G=$'\e[32m'; R=$'\e[31m'; A=$'\e[38;5;209m'; X=$'\e[0m'
ok()   { echo "  ${G}✓${X} $*"; }
info() { echo "  ${D}$*${X}"; }
bad()  { echo "  ${R}✗${X} $*"; }
step() { echo; echo "${A}${B}$*${X}"; }

APP="$HOME/Applications/Marmota Music.app"
EXE="$APP/Contents/MacOS/marmota-music"
AGENT_ID="dev.marmota.music"
AGENT="$HOME/Library/LaunchAgents/$AGENT_ID.plist"
HELPER_APP="$APP/Contents/Helpers/Marmota Apple Music.app"
# Bundle id of the Apple Music helper: its App ID needs the MusicKit service.
HELPER_ID="${MARMOTA_HELPER_ID:-dev.marmota.music.helper}"
CONFIG="$HOME/Library/Application Support/marmota-music/config.json"
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)

echo "${A}${B}♪ Marmota Music${X} — instalación para macOS"

step "1 · Dependencias"
if ! command -v brew &>/dev/null; then
  for p in /opt/homebrew/bin/brew /usr/local/bin/brew; do
    [[ -x "$p" ]] && eval "$("$p" shellenv)" && break
  done
fi
if ! command -v brew &>/dev/null; then
  bad "Hace falta Homebrew (https://brew.sh). Instálalo con:"
  echo '    /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"'
  echo "  y vuelve a ejecutar ${B}./install-mac.sh${X}."
  exit 1
fi
missing=()
for p in gtk4 mpv yt-dlp pkgconf adwaita-icon-theme; do
  brew list --formula "$p" &>/dev/null || missing+=("$p")
done
command -v cargo &>/dev/null || [[ -x "$HOME/.cargo/bin/cargo" ]] || missing+=(rust)
if ((${#missing[@]})); then
  echo "  Faltan: ${B}${missing[*]}${X} (se instalarán con Homebrew)"
  brew install "${missing[@]}"
fi
[[ -x "$HOME/.cargo/bin/cargo" ]] && export PATH="$HOME/.cargo/bin:$PATH"
ok "gtk4, mpv, yt-dlp y rust listos"

step "2 · Compilar"
cargo build --release --quiet
ok "target/release/marmota-music"

# Apple Music with your subscription: a small MusicKit helper (macOS 14+).
MACOS_MAJOR=$(sw_vers -productVersion | cut -d. -f1)
HELPER_BIN=""
if (( MACOS_MAJOR < 14 )); then
  info "Apple Music con tu suscripción necesita macOS 14; sus canciones sonarán desde YouTube Music"
elif ! command -v swift &>/dev/null; then
  info "Sin Swift (xcode-select --install): las canciones de Apple Music sonarán desde YouTube Music"
elif swift build -c release --package-path macos/AppleMusicHelper &>/tmp/marmota-helper-build.log; then
  HELPER_BIN="$(swift build -c release --package-path macos/AppleMusicHelper --show-bin-path)/MarmotaAppleMusic"
  ok "Ayudante de Apple Music compilado"
else
  bad "No se pudo compilar el ayudante de Apple Music (detalles en /tmp/marmota-helper-build.log)"
fi

step "3 · Aplicación"
# Stop a running notch before replacing its binary.
if pgrep -x marmota-music &>/dev/null; then
  "$EXE" quit &>/dev/null || marmota-music quit &>/dev/null || pkill -x marmota-music || true
  sleep 1
fi
launchctl bootout "gui/$(id -u)/$AGENT_ID" &>/dev/null || true

mkdir -p "$APP/Contents/MacOS"
install -m755 target/release/marmota-music "$EXE"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Marmota Music</string>
  <key>CFBundleDisplayName</key><string>Marmota Music</string>
  <key>CFBundleIdentifier</key><string>$AGENT_ID</string>
  <key>CFBundleExecutable</key><string>marmota-music</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <!-- An accessory: no Dock icon, no menu bar of its own. -->
  <key>LSUIElement</key><true/>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST
rm -rf "$APP/Contents/Helpers"
if [[ -n "$HELPER_BIN" ]]; then
  mkdir -p "$HELPER_APP/Contents/MacOS"
  install -m755 "$HELPER_BIN" "$HELPER_APP/Contents/MacOS/marmota-apple-music"
  sed -e "s/__BUNDLE_ID__/$HELPER_ID/" -e "s/__VERSION__/$VERSION/g" \
    macos/AppleMusicHelper/Info.plist > "$HELPER_APP/Contents/Info.plist"
  # MusicKit makes its own developer token, but only for an app signed by
  # your team whose App ID has the MusicKit service.
  TEAM=$(sed -n 's/.*"apple_team_id"[[:space:]]*:[[:space:]]*"\([A-Z0-9]*\)".*/\1/p' "$CONFIG" 2>/dev/null | head -1)
  IDENTITY="${MARMOTA_SIGN_IDENTITY:-}"
  if [[ -z "$IDENTITY" && -n "$TEAM" ]]; then
    IDENTITY=$(security find-identity -v -p codesigning 2>/dev/null \
      | grep -E "\"(Apple Development|Developer ID Application|Mac Developer).*\($TEAM\)\"" \
      | head -1 | awk '{print $2}')
  fi
  if [[ -n "$IDENTITY" ]] && codesign --force --sign "$IDENTITY" --identifier "$HELPER_ID" "$HELPER_APP" &>/dev/null; then
    ok "Ayudante de Apple Music firmado con tu equipo ($HELPER_ID)"
  else
    codesign --force --sign - --identifier "$HELPER_ID" "$HELPER_APP" &>/dev/null || true
    if [[ -z "$TEAM" ]]; then
      info "Configura Apple Music (marmota-music setup apple) y repite ./install-mac.sh para firmar el ayudante"
    else
      bad "No encontré un certificado de desarrollo del equipo $TEAM en el llavero"
      info "Xcode → Settings → Accounts → Manage Certificates → + Apple Development, y repite ./install-mac.sh"
    fi
    info "Mientras tanto, las canciones de Apple Music suenan desde YouTube Music"
  fi
fi
# Ad-hoc signature, so Gatekeeper and the keychain recognize it between runs
# (the helper keeps its own signature).
codesign --force --sign - "$APP" &>/dev/null || true
ok "Instalada en $APP"

# The CLI (marmota-music toggle, status…) on the PATH. A wrapper rather
# than a symlink, so a daemon it starts runs from inside the bundle.
BIN_DIR="$(brew --prefix)/bin"
rm -f "$BIN_DIR/marmota-music"
printf '#!/bin/sh\nexec "%s" "$@"\n' "$EXE" > "$BIN_DIR/marmota-music"
chmod 755 "$BIN_DIR/marmota-music"
ok "Comando ${B}marmota-music${X} en $BIN_DIR"

step "4 · Arranque automático"
mkdir -p "$(dirname "$AGENT")"
cat > "$AGENT" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>$AGENT_ID</string>
  <key>ProgramArguments</key>
  <array>
    <string>$EXE</string>
    <string>daemon</string>
  </array>
  <key>RunAtLoad</key><true/>
  <key>ProcessType</key><string>Interactive</string>
  <key>LimitLoadToSessionType</key><string>Aqua</string>
</dict>
</plist>
PLIST
launchctl bootstrap "gui/$(id -u)" "$AGENT"
ok "Arranca al iniciar sesión ($AGENT)"
ok "Notch en marcha: ábrelo con ${B}⌘ + ⇧ + M${X}"

step "5 · Cuentas"
if [[ "${1:-}" != "--setup" ]] && [[ -f "$CONFIG" ]] && grep -qE '"(setup_done|cookies_browser)"' "$CONFIG"; then
  ok "Tus cuentas ya están configuradas"
  info "Para repetirlo: ./install-mac.sh --setup"
else
  "$EXE" setup
fi
