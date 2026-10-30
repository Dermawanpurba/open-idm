$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$source = Join-Path $projectRoot 'browser-extension'
$destination = Join-Path $projectRoot 'dist\openidm-chrome'
$manifest = Get-Content -LiteralPath (Join-Path $source 'manifest.json') -Raw | ConvertFrom-Json
$files = @('manifest.json', 'background.js', 'shared.js', 'content.js', 'content.css', 'popup.html', 'popup.js', 'popup.css', 'INSTALL.md')
New-Item -ItemType Directory -Path $destination -Force | Out-Null
foreach ($file in $files) {
    Copy-Item -LiteralPath (Join-Path $source $file) -Destination (Join-Path $destination $file) -Force
}
New-Item -ItemType Directory -Path (Join-Path $destination 'icons') -Force | Out-Null
foreach ($size in @(16, 48, 128)) {
    $icon = "icons\icon$size.png"
    Copy-Item -LiteralPath (Join-Path $source $icon) -Destination (Join-Path $destination $icon) -Force
}
$archive = Join-Path $projectRoot "dist\openidm-chrome-$($manifest.version).zip"
Compress-Archive -Path (Join-Path $destination '*') -DestinationPath $archive -Force
Write-Output "Chrome unpacked folder: $destination"
Write-Output "ZIP: $archive"
