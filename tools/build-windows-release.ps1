[CmdletBinding()]
param(
  [switch]$SkipInstall,
  [switch]$SkipChecks,
  [switch]$PortableOnly
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$repositoryRoot = Split-Path -Parent $PSScriptRoot
$packageJsonPath = Join-Path $repositoryRoot 'package.json'
$tauriTarget = Join-Path $repositoryRoot 'src-tauri\target\release'

function Invoke-ExternalCommand {
  param([string]$FilePath, [string[]]$Arguments)

  & $FilePath @Arguments
  if ($LASTEXITCODE -ne 0) {
    throw "Command failed with exit code ${LASTEXITCODE}: $FilePath $($Arguments -join ' ')"
  }
}

if (-not (Test-Path $packageJsonPath)) {
  throw "DevPanel package.json was not found at $packageJsonPath."
}

$nodeVersion = [Version]((node --version).Trim().TrimStart('v'))
if ($nodeVersion.Major -lt 22) {
  throw "Node.js 22 or later is required. Detected Node.js $nodeVersion."
}

$package = Get-Content $packageJsonPath -Raw | ConvertFrom-Json
$version = $package.version
if ([string]::IsNullOrWhiteSpace($version)) {
  throw 'The package version is missing.'
}

Push-Location $repositoryRoot
try {
  if (-not $SkipInstall) {
    Invoke-ExternalCommand 'npm.cmd' @('ci')
  }

  if (-not $SkipChecks) {
    Invoke-ExternalCommand 'npm.cmd' @('run', 'check')
  }

  Invoke-ExternalCommand 'npm.cmd' @('run', 'tauri', 'build')
}
finally {
  Pop-Location
}

$releaseDirectory = Join-Path $repositoryRoot "release\DevPanel-$version-windows-x64"
New-Item -ItemType Directory -Force -Path $releaseDirectory | Out-Null

$portableExe = Get-ChildItem -LiteralPath $tauriTarget -Filter 'devpanel.exe' -File -ErrorAction SilentlyContinue | Select-Object -First 1
if ($null -eq $portableExe) {
  throw "Tauri did not produce the portable executable at $tauriTarget."
}

$portableZip = Join-Path $releaseDirectory "DevPanel-$version-windows-x64-portable.zip"
Compress-Archive -LiteralPath $portableExe.FullName -DestinationPath $portableZip -Force

$artifacts = @($portableZip)
if (-not $PortableOnly) {
  $msi = Get-ChildItem -Path (Join-Path $tauriTarget 'bundle\msi\*.msi') -File -ErrorAction SilentlyContinue | Select-Object -First 1
  $nsis = Get-ChildItem -Path (Join-Path $tauriTarget 'bundle\nsis\*.exe') -File -ErrorAction SilentlyContinue | Select-Object -First 1

  if ($null -eq $msi -or $null -eq $nsis) {
    throw 'Tauri did not produce both MSI and NSIS installers. Install the Windows packaging prerequisites or use -PortableOnly for a portable test build.'
  }

  $msiOutput = Join-Path $releaseDirectory "DevPanel-$version-windows-x64.msi"
  $nsisOutput = Join-Path $releaseDirectory "DevPanel-$version-windows-x64-setup.exe"
  Copy-Item -LiteralPath $msi.FullName -Destination $msiOutput -Force
  Copy-Item -LiteralPath $nsis.FullName -Destination $nsisOutput -Force
  $artifacts += $msiOutput, $nsisOutput
}

$checksums = foreach ($artifact in $artifacts) {
  $hash = Get-FileHash -LiteralPath $artifact -Algorithm SHA256
  "{0}  {1}" -f $hash.Hash.ToLowerInvariant(), (Split-Path $artifact -Leaf)
}
$checksums | Set-Content -LiteralPath (Join-Path $releaseDirectory 'SHA256SUMS.txt') -Encoding ascii

Write-Host ''
Write-Host 'DevPanel Windows release artifacts created:' -ForegroundColor Cyan
$artifacts | ForEach-Object { Write-Host "  $_" }
Write-Host "  $(Join-Path $releaseDirectory 'SHA256SUMS.txt')"
