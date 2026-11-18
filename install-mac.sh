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
# Ad-hoc signature, so Gatekeeper and the keychain recognize it between runs.
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
