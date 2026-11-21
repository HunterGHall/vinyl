# Publishes the current version to GitHub Releases: builds dist\vinyl-setup.exe and attaches it to a
# new release tagged v<version> (the version in Cargo.toml). Installed copies of Vinyl look at the
# latest release when they start and offer to install it, so bump the version before every release.
#
#   gh auth login            (once; also make sure a stale GITHUB_TOKEN variable isn't set)
#   .\installer\release.ps1 -Notes "What's new"
param([string]$Notes = '')
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$version = (Select-String -Path Cargo.toml -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
$tag = "v$version"
gh release view $tag 2>$null | Out-Null
if ($LASTEXITCODE -eq 0) { throw "Release $tag already exists. Bump the version in Cargo.toml first." }

& "$PSScriptRoot\build.ps1"
if (-not $Notes) { $Notes = "Vinyl $version" }
gh release create $tag "dist\vinyl-setup.exe" --title "Vinyl $version" --notes $Notes
if ($LASTEXITCODE -ne 0) { throw 'gh release create failed' }
Write-Host "Published $tag"
