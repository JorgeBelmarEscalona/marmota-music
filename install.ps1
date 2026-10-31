# Marmota Music installer for Windows.
#   powershell -ExecutionPolicy Bypass -File install.ps1           install or update
#   powershell -ExecutionPolicy Bypass -File install.ps1 -Setup    also re-run the account assistant
param([switch]$Setup)
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
Set-Location $PSScriptRoot

$E = [char]27
function ok($m)   { Write-Host "  $E[32m✓$E[0m $m" }
function info($m) { Write-Host "  $E[2m$m$E[0m" }
function step($m) { Write-Host ""; Write-Host "$E[38;5;209m$E[1m$m$E[0m" }

$Msys    = if ($env:MSYS2_ROOT) { $env:MSYS2_ROOT } else { 'C:\msys64' }
$Ucrt    = Join-Path $Msys 'ucrt64'
$Bash    = Join-Path $Msys 'usr\bin\bash.exe'
$Prefix  = Join-Path $env:LOCALAPPDATA 'Programs\MarmotaMusic'
$BinDir  = Join-Path $Prefix 'bin'
$Bin     = Join-Path $BinDir 'marmota-music.exe'
$Config  = Join-Path $env:APPDATA 'marmota-music\config.json'

Write-Host "$E[38;5;209m$E[1m♪ Marmota Music$E[0m — instalación"

step '1 · Dependencias'
if (-not (Test-Path $Bash)) {
    info 'Instalando MSYS2 (GTK4, libmpv y Rust vienen de ahí)…'
    winget install --id MSYS2.MSYS2 -e --silent --accept-source-agreements --accept-package-agreements
    # The first update restarts MSYS2's own runtime; the second one finishes it.
    & $Bash -lc 'pacman -Syu --noconfirm' | Out-Null
}
$pkgs = 'gtk4', 'mpv', 'rust', 'pkgconf', 'gcc' | ForEach-Object { "mingw-w64-ucrt-x86_64-$_" }
& $Bash -lc "pacman -Syu --noconfirm && pacman -S --needed --noconfirm $($pkgs -join ' ')"
if ($LASTEXITCODE) { throw 'pacman no pudo instalar las dependencias' }
if (-not (Get-Command yt-dlp -ErrorAction SilentlyContinue)) {
    winget install --id yt-dlp.yt-dlp -e --silent --accept-source-agreements --accept-package-agreements
}
ok 'gtk4, mpv, rust y yt-dlp listos'

step '2 · Compilar'
$env:PATH = "$Ucrt\bin;$env:PATH"
cargo build --release --quiet
if ($LASTEXITCODE) { throw 'cargo build falló' }

# Stop a running notch so its files can be replaced.
if (Get-Process marmota-music -ErrorAction SilentlyContinue) {
    & $Bin quit | Out-Null
    Start-Sleep -Seconds 1
    Stop-Process -Name marmota-music -Force -ErrorAction SilentlyContinue
}

# GTK looks for share/ and lib/ next to bin/, so the program travels with
# the DLLs it links (found with ldd) and the data GTK reads at run time.
New-Item -ItemType Directory -Force $BinDir | Out-Null
Copy-Item target\release\marmota-music.exe $Bin -Force
$loaders = Get-ChildItem "$Ucrt\lib\gdk-pixbuf-2.0\2.10.0\loaders\*.dll" | ForEach-Object { $_.FullName }
$targets = @($Bin) + $loaders | ForEach-Object { '"' + ($_ -replace '\\', '/') + '"' }
# ldd resolves DLLs through PATH, which only has /ucrt64/bin in that environment.
$env:MSYSTEM = 'UCRT64'
$dlls = & $Bash -lc "ldd $($targets -join ' ') | grep -i '/ucrt64/' | awk '{print `$3}' | sort -u | xargs -r cygpath -w"
foreach ($dll in $dlls) { Copy-Item $dll $BinDir -Force }
foreach ($dir in 'share\glib-2.0\schemas', 'share\icons\Adwaita', 'share\icons\hicolor', 'lib\gdk-pixbuf-2.0') {
    $dest = Join-Path $Prefix $dir
    New-Item -ItemType Directory -Force (Split-Path $dest) | Out-Null
    Copy-Item (Join-Path $Ucrt $dir) (Split-Path $dest) -Recurse -Force
}
ok "Instalado en $Bin"

step '3 · Windows'
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if (($userPath -split ';') -notcontains $BinDir) {
    [Environment]::SetEnvironmentVariable('Path', "$BinDir;$userPath", 'User')
    ok 'marmota-music agregado al PATH (abre una terminal nueva para usarlo)'
}
$env:PATH = "$BinDir;$env:PATH"
$shell = New-Object -ComObject WScript.Shell
$startup = Join-Path ([Environment]::GetFolderPath('Startup')) 'Marmota Music.lnk'
$link = $shell.CreateShortcut($startup)
$link.TargetPath = $Bin
$link.Arguments = 'daemon'
$link.WindowStyle = 7
$link.Save()
$menu = Join-Path ([Environment]::GetFolderPath('Programs')) 'Marmota Music.lnk'
$link = $shell.CreateShortcut($menu)
$link.TargetPath = $Bin
$link.Arguments = 'toggle'
$link.WindowStyle = 7
$link.Save()
ok 'Arranque automático y acceso en el menú Inicio; atajo Win + Alt + M'

step '4 · Reproductor'
Start-Process $Bin -ArgumentList 'daemon' -WindowStyle Hidden
ok 'Notch en marcha'

step '5 · Cuentas'
if (-not $Setup -and (Test-Path $Config) -and (Select-String -Quiet -Path $Config -Pattern '"(setup_done|cookies_browser)"')) {
    ok 'Tus cuentas ya están configuradas'
    info 'Para cambiarlas: marmota-music setup'
} else {
    & $Bin setup
}

Write-Host ""
Write-Host "$E[32m$E[1mListo.$E[0m Abre el notch con $E[1mWin + Alt + M$E[0m."
