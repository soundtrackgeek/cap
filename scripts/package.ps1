[CmdletBinding(SupportsShouldProcess = $true, ConfirmImpact = 'Medium')]
param(
    [string]$OutputDirectory,
    [string]$SourceRevision,
    [string]$ExpectedCoreRevision,
    [string]$Platform = 'windows-x86_64',
    [switch]$SkipBuild,
    [switch]$AllowDirty,
    [switch]$KeepStaging,
    [switch]$Force,
    [switch]$PassThru
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function ConvertTo-AbsolutePath {
    param(
        [Parameter(Mandatory = $true)] [string]$Path,
        [switch]$MustExist
    )

    if ([string]::IsNullOrWhiteSpace($Path)) { throw 'A non-empty path is required.' }
    $absolute = [IO.Path]::GetFullPath([Environment]::ExpandEnvironmentVariables($Path))
    if ($MustExist -and -not (Test-Path -LiteralPath $absolute -PathType Leaf)) { throw "File not found: $absolute" }
    return $absolute
}

function Get-PathKey {
    param([Parameter(Mandatory = $true)] [string]$Path)

    $absolute = [IO.Path]::GetFullPath($Path).TrimEnd([char[]]@([char]92, [char]47))
    if ($absolute.Length -eq 2 -and $absolute[1] -eq ':') { $absolute += [char]92 }
    return $absolute
}

function Get-Sha256 {
    param([Parameter(Mandatory = $true)] [string]$Path)

    try { return (Get-FileHash -LiteralPath $Path -Algorithm SHA256 -ErrorAction Stop).Hash.ToLowerInvariant() }
    catch {
        $algorithm = [Security.Cryptography.SHA256]::Create()
        $stream = [IO.File]::OpenRead($Path)
        try { return ([BitConverter]::ToString($algorithm.ComputeHash($stream))).Replace('-', '').ToLowerInvariant() }
        finally { $stream.Dispose(); $algorithm.Dispose() }
    }
}

function Copy-Literal {
    param(
        [Parameter(Mandatory = $true)] [string]$Source,
        [Parameter(Mandatory = $true)] [string]$Destination
    )

    if (-not (Test-Path -LiteralPath $Source -PathType Leaf)) { throw "Required packaging input is missing: $Source" }
    $parent = [IO.Path]::GetDirectoryName([IO.Path]::GetFullPath($Destination))
    if (-not (Test-Path -LiteralPath $parent -PathType Container)) { [IO.Directory]::CreateDirectory($parent) | Out-Null }
    Copy-Item -LiteralPath $Source -Destination $Destination -Force
}

function Get-RelativeUnixPath {
    param(
        [Parameter(Mandatory = $true)] [string]$Path,
        [Parameter(Mandatory = $true)] [string]$Root
    )

    $rootKey = Get-PathKey $Root
    $pathKey = Get-PathKey $Path
    if (-not $pathKey.StartsWith($rootKey + [char]92, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Packaging path escaped its staging root: $pathKey"
    }
    return $pathKey.Substring($rootKey.Length + 1).Replace('\', '/')
}

function Remove-OwnedStaging {
    param(
        [Parameter(Mandatory = $true)] [string]$Path,
        [Parameter(Mandatory = $true)] [string]$TempRoot
    )

    $pathKey = Get-PathKey $Path
    $tempKey = Get-PathKey $TempRoot
    if (-not $pathKey.StartsWith($tempKey + [char]92, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Refusing to remove a staging path outside the temporary packaging root: $pathKey"
    }
    $marker = Join-Path $pathKey '.cap-package-owned'
    if (-not (Test-Path -LiteralPath $marker -PathType Leaf)) {
        throw "Refusing to remove an unmarked staging directory: $pathKey"
    }
    Remove-Item -LiteralPath $pathKey -Recurse -Force
}

$repoRoot = Get-PathKey ([IO.Path]::GetDirectoryName([IO.Path]::GetFullPath($PSScriptRoot)))
$cargoTomlPath = Join-Path $repoRoot 'Cargo.toml'
$cargoToml = Get-Content -LiteralPath $cargoTomlPath -Raw
if ($cargoToml -match '(?m)^\s*capsule-core\s*=.*path\s*=') {
    throw 'Release packaging refuses a local capsule-core path dependency.'
}
$coreMatch = [regex]::Match($cargoToml, '(?m)^\s*capsule-core\s*=.*?rev\s*=\s*"([0-9a-fA-F]{40})"')
if (-not $coreMatch.Success) { throw 'Cargo.toml does not contain a pinned 40-character capsule-core Git revision.' }
$coreRevision = $coreMatch.Groups[1].Value.ToLowerInvariant()
if (-not [string]::IsNullOrWhiteSpace($ExpectedCoreRevision) -and $coreRevision -ine $ExpectedCoreRevision.ToLowerInvariant()) {
    throw "Pinned capsule-core revision mismatch. Expected $ExpectedCoreRevision, found $coreRevision."
}

foreach ($configName in @('.cargo\config.toml', '.cargo\config')) {
    $configPath = Join-Path $repoRoot $configName
    if (Test-Path -LiteralPath $configPath -PathType Leaf) {
        $config = Get-Content -LiteralPath $configPath -Raw
        if ($config -match '(?im)^\s*\[patch\.' -or $config -match '(?im)capsule-core.*path\s*=') {
            throw "Release packaging refuses temporary Cargo patches in $configPath."
        }
    }
}

$versionMatch = [regex]::Match($cargoToml, '(?m)^version\s*=\s*"([^"]+)"')
if (-not $versionMatch.Success) { throw 'Cargo.toml does not contain a package version.' }
$version = $versionMatch.Groups[1].Value
if ($version -notmatch '^[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?$') { throw "Unsupported package version: $version" }

$gitRevision = (& git -C $repoRoot rev-parse HEAD).Trim()
if ($LASTEXITCODE -ne 0 -or $gitRevision -notmatch '^[0-9a-fA-F]{40}$') { throw 'Unable to resolve the package source revision.' }
if (-not [string]::IsNullOrWhiteSpace($SourceRevision) -and $gitRevision -ine $SourceRevision) {
    throw "Source revision mismatch. Expected $SourceRevision, found $gitRevision."
}
if (-not $AllowDirty) {
    $dirty = @(& git -C $repoRoot status --porcelain --untracked-files=all)
    if ($dirty.Count -gt 0) {
        throw 'Release packaging requires a clean checkout. Pass -AllowDirty only for an explicitly local development archive.'
    }
}

if ([string]::IsNullOrWhiteSpace($OutputDirectory)) { $OutputDirectory = Join-Path $repoRoot 'dist' }
$outputRoot = Get-PathKey (ConvertTo-AbsolutePath $OutputDirectory)
if (-not (Test-Path -LiteralPath $outputRoot -PathType Container)) { [IO.Directory]::CreateDirectory($outputRoot) | Out-Null }
$archiveName = "cap-$version-$Platform"
$archivePath = Join-Path $outputRoot "$archiveName.zip"
$archiveHashPath = "$archivePath.sha256"
if (((Test-Path -LiteralPath $archivePath -PathType Leaf) -or (Test-Path -LiteralPath $archiveHashPath -PathType Leaf)) -and -not $Force) {
    throw "Packaging output already exists. Pass -Force after reviewing it: $archivePath"
}

$targetRoot = Join-Path $repoRoot 'target\delivery'
$binaryPath = Join-Path $targetRoot 'release\cap.exe'
$oldTargetDirectory = $env:CARGO_TARGET_DIR
try {
    if (-not $SkipBuild) {
        $env:CARGO_TARGET_DIR = $targetRoot
        & cargo build --release --locked
        if ($LASTEXITCODE -ne 0) { throw "cargo build --release --locked failed with exit code $LASTEXITCODE" }
    }
    $binaryPath = ConvertTo-AbsolutePath $binaryPath -MustExist

    $tempRoot = Get-PathKey (Join-Path ([IO.Path]::GetTempPath()) ("cap-package-" + [guid]::NewGuid().ToString('N')))
    $staging = Join-Path $tempRoot $archiveName
    [IO.Directory]::CreateDirectory((Join-Path $staging 'bin')) | Out-Null
    [IO.File]::WriteAllText((Join-Path $tempRoot '.cap-package-owned'), '')
    try {
        Copy-Literal -Source $binaryPath -Destination (Join-Path $staging 'bin\cap.exe')
        Copy-Literal -Source (Join-Path $repoRoot 'scripts\install.ps1') -Destination (Join-Path $staging 'install.ps1')
        Copy-Literal -Source (Join-Path $repoRoot 'scripts\uninstall.ps1') -Destination (Join-Path $staging 'uninstall.ps1')
        Copy-Literal -Source (Join-Path $repoRoot 'README.md') -Destination (Join-Path $staging 'README.md')
        Copy-Literal -Source (Join-Path $repoRoot 'docs\windows-install.md') -Destination (Join-Path $staging 'docs\windows-install.md')
        Copy-Literal -Source (Join-Path $repoRoot 'docs\provenance\color-cli.md') -Destination (Join-Path $staging 'docs\provenance\color-cli.md')

        $notice = @"
cap Windows development archive

This archive contains the native Rust cap executable, installation scripts,
checksums, and source documentation. The installer is per-user and does not
modify Capsule journal, recovery, settings, or shell profile state unless the
operator explicitly opts into completion activation.

Rendering attribution: cap-effects ports pure rendering data and equations from
the read-only color-cli revision documented in docs/provenance/color-cli.md.
The inspected upstream snapshot contained no license file or explicit license
declaration. This notice preserves that status and is not a license grant or a
statement of public-release permission for upstream material.
"@
        [IO.File]::WriteAllText((Join-Path $staging 'NOTICE.txt'), $notice.TrimStart(), [Text.UTF8Encoding]::new($false))

        $payload = [Collections.Generic.List[object]]::new()
        foreach ($file in @(Get-ChildItem -LiteralPath $staging -Recurse -File | Sort-Object FullName)) {
            $relative = Get-RelativeUnixPath -Path $file.FullName -Root $staging
            if ($relative -ieq 'checksums.sha256' -or $relative -ieq 'manifest.json' -or $relative -ieq '.cap-package-owned') { continue }
            $payload.Add([pscustomobject]@{ path = $relative; sha256 = (Get-Sha256 $file.FullName) })
        }
        $checksumLines = foreach ($item in $payload) { "$($item.sha256)  $($item.path)" }
        [IO.File]::WriteAllText((Join-Path $staging 'checksums.sha256'), ([string]::Join([Environment]::NewLine, $checksumLines) + [Environment]::NewLine), [Text.UTF8Encoding]::new($false))

        $manifest = [ordered]@{
            schemaVersion = 1
            artifact = $archiveName
            version = $version
            platform = $Platform
            sourceRevision = $gitRevision.ToLowerInvariant()
            capsuleCoreRevision = $coreRevision
            builtAtUtc = (Get-Date).ToUniversalTime().ToString('o')
            payload = @($payload)
            licenseStatus = 'color-cli upstream snapshot has no inspected license declaration; permission remains unresolved'
        }
        [IO.File]::WriteAllText((Join-Path $staging 'manifest.json'), (($manifest | ConvertTo-Json -Depth 8) + [Environment]::NewLine), [Text.UTF8Encoding]::new($false))

        if ($WhatIfPreference) { return }
        if (Test-Path -LiteralPath $archivePath -PathType Leaf) { Remove-Item -LiteralPath $archivePath -Force }
        Compress-Archive -LiteralPath $staging -DestinationPath $archivePath -CompressionLevel Optimal -Force
        $archiveHash = Get-Sha256 $archivePath
        [IO.File]::WriteAllText($archiveHashPath, "$archiveHash  $([IO.Path]::GetFileName($archivePath))$([Environment]::NewLine)", [Text.UTF8Encoding]::new($false))

        $result = [pscustomobject]@{
            ok = $true
            archive = (Get-PathKey $archivePath)
            archiveSha256 = $archiveHash
            checksumManifest = 'checksums.sha256 (inside archive)'
            sourceRevision = $gitRevision.ToLowerInvariant()
            capsuleCoreRevision = $coreRevision
            version = $version
            platform = $Platform
            staging = if ($KeepStaging) { (Get-PathKey $staging) } else { $null }
        }
        if ($PassThru) { $result | ConvertTo-Json -Depth 8 } else {
            Write-Output "Created $($result.archive)"
            Write-Output "Archive SHA-256: $archiveHash"
            Write-Output "Pinned capsule-core: $coreRevision"
            Write-Output "Source revision: $($gitRevision.ToLowerInvariant())"
        }
    }
    finally {
        if (-not $KeepStaging) { Remove-OwnedStaging -Path $tempRoot -TempRoot ([IO.Path]::GetTempPath()) }
    }
}
finally {
    if ($null -eq $oldTargetDirectory) { Remove-Item Env:CARGO_TARGET_DIR -ErrorAction SilentlyContinue }
    else { $env:CARGO_TARGET_DIR = $oldTargetDirectory }
}
