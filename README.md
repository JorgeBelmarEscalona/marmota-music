# Marmota Music

Reproductor de música nativo para Omarchy/Hyprland: una barra flotante tipo
*notch* en la parte inferior de la pantalla, con estética de vidrio, portada,
letras sincronizadas y controles completos. Suena con **mpv** + **yt-dlp**
(YouTube Music); Spotify queda para la siguiente fase.

- **Abrir/cerrar:** `Super + Shift + M` (o `marmota-music toggle`)
- Reemplaza a Orchard: el atajo y el botón de la barra ya apuntan aquí.

## Uso

| Acción | Cómo |
| --- | --- |
| Abrir el notch | `Super + Shift + M` |
| Reproducir / pausar | clic en el botón central, o `Espacio` |
| Adelantar / retroceder 5 s | `←` / `→` |
| Volumen | arrastrar la barra, rueda del ratón, o `↑` / `↓` |
| Encoger a la píldora mini | clic en el chevron, `Esc` |
| Letras / biblioteca / buscar / cola | pestañas de la derecha |
| Ver el videoclip dentro del notch | botón ▶▭ junto a *Guardar*, o `v` |
| Guardar en la biblioteca | botón *Guardar* |
| Crear lista | Biblioteca → *Nueva lista* |
| Importar tus listas de YouTube Music | Biblioteca → *Importar de YouTube Music* (usa la sesión del navegador; se configura en `~/.config/marmota-music/config.json`, clave `cookies_browser`) |
| Reproducir a continuación / quitar de la cola | botones que aparecen al pasar el ratón por una fila |

La píldora mini muestra la línea actual de la letra y la siguiente, además de
los controles y una línea de progreso en el borde superior.

El notch aparece siempre en el monitor que tiene el foco y se escala según su
altura lógica: 1040×612 en una pantalla 1440p, 832×490 en el panel del portátil
(1080p a 1.25×). Al cambiar de monitor se vuelve a mapear la superficie.

## Cómo está hecho

| Pieza | Implementación |
| --- | --- |
| Ventana | GTK4 + `gtk4-layer-shell` (capa overlay, anclada abajo) |
| Dibujo | Cairo + Pango, todo pintado a mano (vidrio, iconos, texto) |
| Audio y vídeo | libmpv embebido (render por software al notch) con IPC JSON por socket |
| Búsqueda / inicio / álbumes / listas / radio | API InnerTube de YouTube Music (`src/ytm.rs`); yt-dlp como respaldo y para listas privadas |
| Letras | LRCLIB (exacta y búsqueda) y NetEase de respaldo; solo se cachean las sincronizadas |
| Portadas | descargadas y cacheadas en `~/.cache/marmota-music/art/` |
| Biblioteca / listas | SQLite (`~/.local/share/marmota-music/marmota-music.db`) |
| Control externo | socket `$XDG_RUNTIME_DIR/marmota-music.sock` + CLI |

## CLI

```
marmota-music toggle | show | hide | collapse | daemon | quit
marmota-music status                 # JSON del estado actual
marmota-music play-pause | next | prev | video
marmota-music play <videoId> [--title T] [--artist A]
marmota-music search [texto]
```

## Integración con el escritorio

- `~/.config/hypr/bindings.lua`: `SUPER + SHIFT + M` → `marmota-music toggle`.
- `~/.config/hypr/autostart.lua`: arranca `marmota-music daemon` al iniciar sesión.
- `~/.config/hypr/music.lua`: regla de capa `marmota-music` y ventana flotante
  `marmota-video` (960×540, centrada) para el modo vídeo.
- `~/.config/omarchy/shell.json`: el botón de la barra muestra la canción y abre
  el notch (`~/.config/omarchy/bar/scripts/marmota-music-button`).

## Compilar / instalar

```bash
cd ~/Projects/marmota-music
cargo build --release
install -Dm755 target/release/marmota-music ~/.local/bin/marmota-music
```

Dependencias de sistema: `gtk4`, `gtk4-layer-shell`, `mpv`, `yt-dlp`.

## Limitaciones conocidas

- **Spotify**: pendiente (fase 2, vía `librespot` como dispositivo Connect;
  requiere Premium).
- **Apple Music**: descartado (no hay reproducción viable en Linux).
- **Blur de fondo**: Hyprland 0.56 no aplica `layer_rule` con `blur` en esta
  instalación (no hay `hyprpm`, así que tampoco HyprGlass). El vidrio se pinta
  con opacidad propia y la portada desenfocada como textura.
- La superficie de capa no puede cambiar de monitor sin volver a mapearse: al
  abrir el notch en otro monitor se recrea (parpadeo mínimo, ~1 frame).
- El modo vídeo recarga la pista con un formato que incluye vídeo; si la pista
  no tiene vídeo, vuelve solo a audio en unos segundos.
