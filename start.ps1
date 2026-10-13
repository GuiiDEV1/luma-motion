$ErrorActionPreference = 'Stop'
Set-Location -LiteralPath $PSScriptRoot
if (-not (Get-Command ffmpeg -ErrorAction SilentlyContinue)) { throw 'Instale FFmpeg e adicione ao PATH.' }
if (-not (Get-Command ffprobe -ErrorAction SilentlyContinue)) { throw 'Instale ffprobe e adicione ao PATH.' }
if (Test-Path -LiteralPath 'target\release\luma-motion.exe') {
    Start-Process -FilePath '.\target\release\luma-motion.exe' -WindowStyle Hidden
} else {
    cargo build --release
    if ($LASTEXITCODE -ne 0) { throw 'Falha ao compilar o aplicativo Rust.' }
    Start-Process -FilePath '.\target\release\luma-motion.exe' -WindowStyle Hidden
}
