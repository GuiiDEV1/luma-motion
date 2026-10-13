$ErrorActionPreference = 'Stop'
Set-Location -LiteralPath (Split-Path -Parent $PSScriptRoot)
cargo build --release
if ($LASTEXITCODE -ne 0) { throw 'Falha na compilação.' }
New-Item -ItemType Directory -Path 'dist' -Force | Out-Null
Copy-Item -LiteralPath 'target\release\luma-motion.exe' -Destination 'dist\Luma Motion 0.5.exe' -Force
New-Item -ItemType Directory -Path 'dist\bin' -Force | Out-Null
foreach ($codec in @('ffmpeg', 'ffprobe')) {
    $codecPath = (Get-Command $codec -ErrorAction Stop).Source
    $shimPath = [System.IO.Path]::ChangeExtension($codecPath, '.shim')
    if (Test-Path -LiteralPath $shimPath) {
        $shimText = Get-Content -LiteralPath $shimPath -Raw
        if ($shimText -match 'path\s*=\s*"([^"]+)"') { $codecPath = $Matches[1] }
    }
    Copy-Item -LiteralPath $codecPath -Destination "dist\bin\$codec.exe" -Force
    $codecLicense = Join-Path (Split-Path -Parent (Split-Path -Parent $codecPath)) 'LICENSE'
    if (Test-Path -LiteralPath $codecLicense) { Copy-Item -LiteralPath $codecLicense -Destination 'dist\bin\FFmpeg-LICENSE.txt' -Force }
}
@'
Luma Motion 0.5 — Windows x64

Abra "Luma Motion 0.5.exe". A interface está dentro do executável.
Requer Microsoft Edge WebView2 Runtime, já presente na maioria dos PCs Windows.
FFmpeg e ffprobe estão incluídos na pasta bin. Mantenha essa pasta ao lado do exe.

Dados e configurações: %LOCALAPPDATA%\LumaMotion
Modo web: "Luma Motion 0.5.exe" --web (http://127.0.0.1:8765)
'@ | Set-Content -LiteralPath 'dist\LEIA-ME.txt' -Encoding utf8
