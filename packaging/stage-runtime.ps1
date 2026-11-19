<#
.SYNOPSIS
  Stage local llama-server binaries and compute a SHA-256 manifest for offline Tauri packaging.
.DESCRIPTION
  Copies llama-server.exe, adjacent *.dll files, and LICENSE* notices from a local
  llama.cpp directory into OutDir (default: packaging/staged/llama), records the
  observed `llama-server.exe --version` output, and writes SHA256SUMS.txt and
  runtime-manifest.json. Never downloads binaries from the internet.
#>
[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)]
  [string]$SourceDir,

  [string]$OutDir = (Join-Path $PSScriptRoot "staged\llama")
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if (-not (Test-Path -LiteralPath $SourceDir -PathType Container)) {
  Write-Error "Source directory does not exist: $SourceDir"
  exit 1
}

$resolvedSource = (Resolve-Path -LiteralPath $SourceDir).Path
$serverExe = Join-Path $resolvedSource "llama-server.exe"
if (-not (Test-Path -LiteralPath $serverExe -PathType Leaf)) {
  Write-Error "llama-server.exe not found in: $resolvedSource"
  exit 1
}

if (Test-Path -LiteralPath $OutDir) {
  Remove-Item -LiteralPath $OutDir -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$resolvedOut = (Resolve-Path -LiteralPath $OutDir).Path

Copy-Item -LiteralPath $serverExe -Destination $resolvedOut -Force

Get-ChildItem -LiteralPath $resolvedSource -File | Where-Object {
  $_.Extension -ieq ".dll" -or $_.Name -ilike "LICENSE*" -or $_.Name -ieq "NOTICE"
} | ForEach-Object {
  Copy-Item -LiteralPath $_.FullName -Destination $resolvedOut -Force
}

# llama-server prints --version to stderr; avoid PS 5.1 NativeCommandError under Stop.
$prevErrPref = $ErrorActionPreference
$ErrorActionPreference = "Continue"
$versionOut = & (Join-Path $resolvedOut "llama-server.exe") --version 2>&1 | Out-String
$ErrorActionPreference = $prevErrPref
$versionLine = ($versionOut -split "`r?`n" | ForEach-Object { $_.Trim() } | Where-Object { $_ -like "version:*" } | Select-Object -First 1)
if ($versionLine) {
  $version = ($versionLine -replace "^version:\s*", "").Trim()
} else {
  $version = "unknown"
}

$files = @()
$sums = @()
Get-ChildItem -LiteralPath $resolvedOut -File | Where-Object {
  $_.Name -ne "SHA256SUMS.txt" -and $_.Name -ne "runtime-manifest.json"
} | Sort-Object Name | ForEach-Object {
  $hash = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
  $sums += "$hash  $($_.Name)"
  $files += [ordered]@{
    name   = $_.Name
    bytes  = [int64]$_.Length
    sha256 = $hash
  }
}

$sumsPath = Join-Path $resolvedOut "SHA256SUMS.txt"
$manifestPath = Join-Path $resolvedOut "runtime-manifest.json"
[System.IO.File]::WriteAllText($sumsPath, ($sums -join "`n") + "`n", [System.Text.UTF8Encoding]::new($false))

$arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString().ToLowerInvariant()
$manifest = [ordered]@{
  runtime = "llama.cpp"
  license = "MIT"
  os      = "windows"
  arch    = $arch
  version = $version
  files   = $files
}
$json = $manifest | ConvertTo-Json -Depth 4
[System.IO.File]::WriteAllText($manifestPath, $json + "`n", [System.Text.UTF8Encoding]::new($false))

Write-Host "Staged $($files.Count) file(s) to $resolvedOut (llama-server version: $version)"
