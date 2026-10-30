#!/usr/bin/env bash
# Marmota Music installer for Omarchy / Hyprland.
#   ./install.sh            install or update
#   ./install.sh --setup    also re-run the account assistant
set -euo pipefail
cd "$(dirname "$0")"

B=$'\e[1m'; D=$'\e[2m'; G=$'\e[32m'; A=$'\e[38;5;209m'; X=$'\e[0m'
ok()   { echo "  ${G}✓${X} $*"; }
info() { echo "  ${D}$*${X}"; }
step() { echo; echo "${A}${B}$*${X}"; }

BIN="$HOME/.local/bin/marmota-music"
HYPR="$HOME/.config/hypr"
CONFIG="$HOME/.config/marmota-music/config.json"

echo "${A}${B}♪ Marmota Music${X} — instalación"

step "1 · Dependencias"
missing=()
# spotifyd is small and only runs once the user signs in to Spotify.
for p in gtk4 gtk4-layer-shell mpv yt-dlp spotifyd; do
  pacman -Q "$p" &>/dev/null || missing+=("$p")
done
command -v cargo &>/dev/null || missing+=(rust)
if ((${#missing[@]})); then
  echo "  Faltan: ${B}${missing[*]}${X} (se instalarán con sudo)"
  sudo pacman -S --needed "${missing[@]}"
fi
ok "gtk4, gtk4-layer-shell, mpv, yt-dlp, spotifyd y rust listos"

step "2 · Compilar"
cargo build --release --quiet
install -Dm755 target/release/marmota-music "$BIN"
ok "Instalado en $BIN"

step "3 · Hyprland"
if grep -rqs "marmota-music" "$HYPR" --include='*.lua' --include='*.conf'; then
  ok "Atajo y arranque automático ya configurados"
elif [[ -f "$HYPR/hyprland.lua" ]]; then
  # Omarchy 4: Lua config.
  cat > "$HYPR/marmota-music.lua" <<LUA
-- Marmota Music: notch de música (SUPER+SHIFT+M).
o.bind("SUPER + SHIFT + M", "Music", "marmota-music toggle")
o.launch_on_start("$BIN daemon")
hl.layer_rule({ name = "mm-glass", match = { namespace = "marmota-music" }, blur = true, ignore_alpha = 0.18 })
LUA
  echo 'require("hypr.marmota-music")' >> "$HYPR/hyprland.lua"
  hyprctl reload &>/dev/null || true
  ok "Atajo Super + Shift + M agregado ($HYPR/marmota-music.lua)"
elif [[ -f "$HYPR/hyprland.conf" ]]; then
  cat >> "$HYPR/hyprland.conf" <<CONF

# Marmota Music: notch de música (SUPER+SHIFT+M).
bindd = SUPER SHIFT, M, Music, exec, marmota-music toggle
exec-once = $BIN daemon
layerrule = blur, marmota-music
layerrule = ignorealpha 0.18, marmota-music
CONF
  hyprctl reload &>/dev/null || true
  ok "Atajo Super + Shift + M agregado a hyprland.conf"
else
  info "No encontré la configuración de Hyprland; agrega un atajo a «marmota-music toggle»."
fi

step "4 · Reproductor"
if pgrep -x marmota-music &>/dev/null; then
  marmota-music quit &>/dev/null || pkill -x marmota-music || true
  sleep 1
fi
setsid -f "$BIN" daemon &>/dev/null
ok "Notch en marcha"

step "5 · Cuentas"
if [[ "${1:-}" != "--setup" ]] && [[ -f "$CONFIG" ]] && grep -qE '"(setup_done|cookies_browser)"' "$CONFIG"; then
  ok "Tus cuentas ya están configuradas"
  # Spotify was set up for search but never signed in on this machine
  # (e.g. spotifyd was just installed): offer only that step.
  creds="${XDG_CACHE_HOME:-$HOME/.cache}/spotifyd/oauth/credentials.json"
  if grep -qE '"spotify_client_id": *"[^"]+"' "$CONFIG" && [[ ! -f "$creds" ]]; then
    "$BIN" setup spotify
  fi
  info "Para cambiarlas: marmota-music setup"
else
  "$BIN" setup
fi

echo
echo "${G}${B}Listo.${X} Abre el notch con ${B}Super + Shift + M${X}."
