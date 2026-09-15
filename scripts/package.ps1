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

function Get-StringSha256 {
    param([AllowEmptyString()] [string]$Value)

    $algorithm = [Security.Cryptography.SHA256]::Create()
    try {
        $bytes = [Text.UTF8Encoding]::new($false).GetBytes($Value)
        return ([BitConverter]::ToString($algorithm.ComputeHash($bytes))).Replace('-', '').ToLowerInvariant()
    }
    finally {
        $algorithm.Dispose()
    }
}

function Publish-TextFile {
    param(
        [Parameter(Mandatory = $true)] [string]$Destination,
        [Parameter(Mandatory = $true)] [AllowEmptyString()] [string]$Content
    )

    $parent = [IO.Path]::GetDirectoryName([IO.Path]::GetFullPath($Destination))
    if (-not (Test-Path -LiteralPath $parent -PathType Container)) { [IO.Directory]::CreateDirectory($parent) | Out-Null }
    $temporary = "$Destination.tmp.$([guid]::NewGuid().ToString('N'))"
    [IO.File]::WriteAllText($temporary, $Content, [Text.UTF8Encoding]::new($false))
    try {
        if (Test-Path -LiteralPath $Destination -PathType Leaf) {
            $backup = "$Destination.previous.$([guid]::NewGuid().ToString('N'))"
            [IO.File]::Replace($temporary, $Destination, $backup, $true)
            if (Test-Path -LiteralPath $backup -PathType Leaf) { Remove-Item -LiteralPath $backup -Force }
        }
        else {
            [IO.File]::Move($temporary, $Destination)
        }
    }
    finally {
        if (Test-Path -LiteralPath $temporary -PathType Leaf) { Remove-Item -LiteralPath $temporary -Force -ErrorAction SilentlyContinue }
    }
}
$dirtyLines = @(& git -C $repoRoot status --porcelain --untracked-files=all)
if ($LASTEXITCODE -ne 0) { throw 'Unable to inspect the package checkout status.' }
$dirtyState = [string]::Join("`n", @($dirtyLines | ForEach-Object { [string]$_ }))
$dirtyFingerprint = Get-StringSha256 $dirtyState
$isDirty = $dirtyLines.Count -gt 0
if ($isDirty -and -not $AllowDirty) {
    throw 'Release packaging requires a clean checkout. Pass -AllowDirty only for an explicitly local development archive.'
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
$provenancePath = Join-Path $targetRoot 'cap-build-provenance.json'
$oldTargetDirectory = $env:CARGO_TARGET_DIR
try {
    if (-not $SkipBuild) {
        $env:CARGO_TARGET_DIR = $targetRoot
        & cargo build --release --locked
        if ($LASTEXITCODE -ne 0) { throw "cargo build --release --locked failed with exit code $LASTEXITCODE" }
    }
    $binaryPath = ConvertTo-AbsolutePath $binaryPath -MustExist
    $binaryHash = Get-Sha256 $binaryPath
    $expectedProvenance = [ordered]@{
        schemaVersion = 1
        binaryPath = (Get-PathKey $binaryPath)
        binarySha256 = $binaryHash
        sourceRevision = $gitRevision.ToLowerInvariant()
        capsuleCoreRevision = $coreRevision
        version = $version
        platform = $Platform
        profile = 'release'
        featureSet = 'default'
        command = 'cargo build --release --locked'
        dirty = $isDirty
        dirtyFingerprint = $dirtyFingerprint
        builtAtUtc = (Get-Date).ToUniversalTime().ToString('o')
    }
    if ($SkipBuild) {
        if (-not (Test-Path -LiteralPath $provenancePath -PathType Leaf)) {
            throw "-SkipBuild requires a successful package provenance stamp: $provenancePath"
        }
        try { $provenance = Get-Content -LiteralPath $provenancePath -Raw | ConvertFrom-Json }
        catch { throw "The package provenance stamp is unreadable: $provenancePath" }
        foreach ($field in @('schemaVersion', 'binaryPath', 'binarySha256', 'sourceRevision', 'capsuleCoreRevision', 'version', 'platform', 'profile', 'featureSet', 'command', 'dirty', 'dirtyFingerprint', 'builtAtUtc')) {
            if ($null -eq $provenance.PSObject.Properties[$field]) { throw "The package provenance stamp is missing '$field': $provenancePath" }
        }
        if ([int]$provenance.schemaVersion -ne 1 -or
            (Get-PathKey ([string]$provenance.binaryPath)) -ine (Get-PathKey $binaryPath) -or
            [string]$provenance.binarySha256 -ine $binaryHash -or
            [string]$provenance.sourceRevision -ine $gitRevision -or
            [string]$provenance.capsuleCoreRevision -ine $coreRevision -or
            [string]$provenance.version -ine $version -or
            [string]$provenance.platform -ine $Platform -or
            [string]$provenance.profile -ine 'release' -or
            [string]$provenance.featureSet -ine 'default' -or
            [string]$provenance.command -ine 'cargo build --release --locked' -or
            [bool]$provenance.dirty -ne $isDirty -or
            [string]$provenance.dirtyFingerprint -ine $dirtyFingerprint) {
            throw "The existing package provenance stamp does not match this source, build, or checkout state: $provenancePath"
        }
    }

    $tempRoot = Get-PathKey (Join-Path ([IO.Path]::GetTempPath()) ("cap-package-" + [guid]::NewGuid().ToString('N')))
    $staging = Join-Path $tempRoot $archiveName
    [IO.Directory]::CreateDirectory((Join-Path $staging 'bin')) | Out-Null
    [IO.File]::WriteAllText((Join-Path $tempRoot '.cap-package-owned'), '')
    try {
        Copy-Literal -Source $binaryPath -Destination (Join-Path $staging 'bin\cap.exe')
        Copy-Literal -Source (Join-Path $repoRoot 'scripts\install.bat') -Destination (Join-Path $staging 'install.bat')
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
            dirty = $isDirty
            dirtyFingerprint = $dirtyFingerprint
            payload = @($payload)
            licenseStatus = 'color-cli upstream snapshot has no inspected license declaration; permission remains unresolved'
        }
        [IO.File]::WriteAllText((Join-Path $staging 'manifest.json'), (($manifest | ConvertTo-Json -Depth 8) + [Environment]::NewLine), [Text.UTF8Encoding]::new($false))

        if ($WhatIfPreference) { return }
        if (Test-Path -LiteralPath $archivePath -PathType Leaf) { Remove-Item -LiteralPath $archivePath -Force }
        # Windows PowerShell's Compress-Archive can store backslash member names.
        # Use canonical ZIP paths so cap update can read the exact known members.
        Add-Type -AssemblyName System.IO.Compression
        Add-Type -AssemblyName System.IO.Compression.FileSystem
        $zip = [IO.Compression.ZipFile]::Open($archivePath, [IO.Compression.ZipArchiveMode]::Create)
        try {
            foreach ($file in @(Get-ChildItem -LiteralPath $staging -Recurse -File | Sort-Object FullName)) {
                $member = "$archiveName/$(Get-RelativeUnixPath -Path $file.FullName -Root $staging)"
                [void][IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip, $file.FullName, $member, [IO.Compression.CompressionLevel]::Optimal)
            }
        }
        finally { $zip.Dispose() }
        $archiveHash = Get-Sha256 $archivePath
        [IO.File]::WriteAllText($archiveHashPath, "$archiveHash  $([IO.Path]::GetFileName($archivePath))$([Environment]::NewLine)", [Text.UTF8Encoding]::new($false))
        if (-not $SkipBuild) {
            Publish-TextFile -Destination $provenancePath -Content (($expectedProvenance | ConvertTo-Json -Depth 8) + [Environment]::NewLine)
        }

        $result = [pscustomobject]@{
            ok = $true
            archive = (Get-PathKey $archivePath)
            archiveSha256 = $archiveHash
            checksumManifest = 'checksums.sha256 (inside archive)'
            sourceRevision = $gitRevision.ToLowerInvariant()
            capsuleCoreRevision = $coreRevision
            version = $version
            platform = $Platform
            binarySha256 = $binaryHash
            dirty = $isDirty
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
