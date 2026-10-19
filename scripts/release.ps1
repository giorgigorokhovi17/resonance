# Builds a signed release and the latest.json update feed.
#
#   1. Bump "version" in src-tauri/tauri.conf.json (and package.json / Cargo.toml to match).
#   2. powershell -ExecutionPolicy Bypass -File scripts/release.ps1 -Notes "What's new"
#   3. Create a GitHub release tagged v<version> and upload the files this script prints.
#
# The private signing key lives outside the project ($env:USERPROFILE\.tauri\resonance-updater.key).
# Never share or commit it; without it you can't publish updates that existing installs accept.
param(
    [string]$Notes = "",
    [string]$KeyPath = "$env:USERPROFILE\.tauri\resonance-updater.key"
)
$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
$conf = Get-Content "$root\src-tauri\tauri.conf.json" -Raw | ConvertFrom-Json
$version = $conf.version
$endpoint = $conf.plugins.updater.endpoints[0]
if ($endpoint -notmatch "github\.com/([^/]+)/([^/]+)/releases") { throw "Set plugins.updater.endpoints in tauri.conf.json to your GitHub repo first." }
if ($Matches[1] -eq "YOUR_GITHUB_USERNAME") { throw "Replace YOUR_GITHUB_USERNAME in tauri.conf.json (plugins.updater.endpoints) with your GitHub user name." }
$repo = "$($Matches[1])/$($Matches[2])"
if (-not (Test-Path $KeyPath)) { throw "Signing key not found at $KeyPath" }

$env:TAURI_SIGNING_PRIVATE_KEY = $KeyPath
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ""
Push-Location $root
try { corepack pnpm tauri build; if ($LASTEXITCODE) { throw "Build failed" } } finally { Pop-Location }

$bundle = "$root\src-tauri\target\release\bundle"
$setup = Get-ChildItem "$bundle\nsis" -Filter "*_$version*-setup.exe" | Select-Object -First 1
if (-not $setup) { throw "NSIS installer for $version not found in $bundle\nsis" }
$sig = Get-Content "$($setup.FullName).sig" -Raw

$feed = [ordered]@{
    version  = $version
    notes    = $Notes
    pub_date = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
    platforms = [ordered]@{
        "windows-x86_64" = [ordered]@{
            signature = $sig.Trim()
            url       = "https://github.com/$repo/releases/download/v$version/$($setup.Name)"
        }
    }
}
$latest = "$bundle\latest.json"
[IO.File]::WriteAllText($latest, ($feed | ConvertTo-Json -Depth 5))

Write-Host ""
Write-Host "Release v$version is ready. Create a GitHub release tagged v$version in $repo and upload:" -ForegroundColor Green
Write-Host "  $($setup.FullName)"
Write-Host "  $latest"
Write-Host "  $bundle\msi\*.msi  (optional, for people who prefer MSI)"
