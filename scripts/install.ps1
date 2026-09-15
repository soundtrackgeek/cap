[CmdletBinding(SupportsShouldProcess = $true, ConfirmImpact = 'Medium')]
param(
    [Parameter(Position = 0)]
    [string]$SourcePath,

    [string]$InstallRoot,
    [string]$ChecksumPath,
    [string]$PackageRoot,
    [string]$PathStatePath,
    [string]$CompletionProfilePath,
    [switch]$SkipPath,
    [switch]$ActivateCompletions,
    [switch]$PromptForReplace,
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

    if ([string]::IsNullOrWhiteSpace($Path)) {
        throw 'A non-empty path is required.'
    }
    $expanded = [Environment]::ExpandEnvironmentVariables($Path)
    $absolute = [IO.Path]::GetFullPath($expanded)
    if ($MustExist -and -not (Test-Path -LiteralPath $absolute -PathType Leaf)) {
        throw "File not found: $absolute"
    }
    return $absolute
}

function Get-PathKey {
    param([Parameter(Mandatory = $true)] [string]$Path)

    $absolute = [IO.Path]::GetFullPath($Path)
    $absolute = $absolute.TrimEnd([char[]]@([char]92, [char]47))
    if ($absolute.Length -eq 2 -and $absolute[1] -eq ':') {
        $absolute += [char]92
    }
    return $absolute
}

function Assert-SafeInstallRoot {
    param([Parameter(Mandatory = $true)] [string]$Path)

    $root = Get-PathKey (ConvertTo-AbsolutePath $Path)
    $pathRoot = [IO.Path]::GetPathRoot($root)
    if ([string]::IsNullOrWhiteSpace($pathRoot) -or $root -eq $pathRoot) {
        throw "Refusing to use a drive or filesystem root as the cap install root: $root"
    }
    if ($root.Length -lt ($pathRoot.Length + 3)) {
        throw "Refusing an unexpectedly broad cap install root: $root"
    }
    return $root
}

function Assert-UnderRoot {
    param(
        [Parameter(Mandatory = $true)] [string]$Path,
        [Parameter(Mandatory = $true)] [string]$Root
    )

    $pathKey = Get-PathKey (ConvertTo-AbsolutePath $Path)
    $rootKey = Get-PathKey (ConvertTo-AbsolutePath $Root)
    if ($pathKey -ne $rootKey -and -not $pathKey.StartsWith($rootKey + [char]92, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Refusing a path outside the cap install root. Path: $pathKey Root: $rootKey"
    }
    return $pathKey
}

function Assert-NoReparsePath {
    param([Parameter(Mandatory = $true)] [string]$Path)

    $probe = Get-PathKey (ConvertTo-AbsolutePath $Path)
    while (-not [string]::IsNullOrWhiteSpace($probe)) {
        if (Test-Path -LiteralPath $probe) {
            $item = Get-Item -LiteralPath $probe -Force
            if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
                throw "Refusing a reparse-point install path: $probe"
            }
        }
        $parent = [IO.Path]::GetDirectoryName($probe)
        if ([string]::IsNullOrWhiteSpace($parent) -or $parent -ieq $probe) { break }
        $probe = Get-PathKey $parent
    }
}

function New-DirectoryLiteral {
    param([Parameter(Mandatory = $true)] [string]$Path)

    if (-not (Test-Path -LiteralPath $Path -PathType Container)) {
        [IO.Directory]::CreateDirectory($Path) | Out-Null
    }
    return (Get-PathKey $Path)
}

function Get-Sha256 {
    param([Parameter(Mandatory = $true)] [string]$Path)

    try {
        return (Get-FileHash -LiteralPath $Path -Algorithm SHA256 -ErrorAction Stop).Hash.ToLowerInvariant()
    }
    catch {
        $algorithm = [Security.Cryptography.SHA256]::Create()
        $stream = [IO.File]::OpenRead($Path)
        try {
            return ([BitConverter]::ToString($algorithm.ComputeHash($stream))).Replace('-', '').ToLowerInvariant()
        }
        finally {
            $stream.Dispose()
            $algorithm.Dispose()
        }
    }
}

function Read-ExpectedSha256 {
    param(
        [Parameter(Mandatory = $true)] [string]$Path,
        [Parameter(Mandatory = $true)] [string]$FileName
    )

    foreach ($line in [IO.File]::ReadAllLines($Path)) {
        $trimmed = $line.Trim()
        if ($trimmed.Length -eq 0 -or $trimmed.StartsWith('#')) {
            continue
        }
        if ($trimmed -notmatch '^([0-9A-Fa-f]{64})\s+\*?(.+?)\s*$') {
            continue
        }
        $name = $Matches[2].Trim().Replace('/', '\')
        $leaf = [IO.Path]::GetFileName($name)
        if ($leaf -ieq $FileName -or $name -ieq "bin\$FileName") {
            return $Matches[1].ToLowerInvariant()
        }
    }
    throw "Checksum manifest does not contain a SHA-256 entry for ${FileName}: $Path"
}

function Publish-TextFile {
    param(
        [Parameter(Mandatory = $true)] [string]$Destination,
        [Parameter(Mandatory = $true)] [AllowEmptyString()] [string]$Content
    )

    $parent = [IO.Path]::GetDirectoryName([IO.Path]::GetFullPath($Destination))
    New-DirectoryLiteral $parent | Out-Null
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
        if (Test-Path -LiteralPath $temporary -PathType Leaf) {
            Remove-Item -LiteralPath $temporary -Force -ErrorAction SilentlyContinue
        }
    }
}

function Get-PathValue {
    param([string]$StatePath)

    if (-not [string]::IsNullOrWhiteSpace($StatePath)) {
        $path = ConvertTo-AbsolutePath $StatePath
        if (Test-Path -LiteralPath $path -PathType Leaf) {
            return [IO.File]::ReadAllText($path)
        }
        return ''
    }
    $value = [Environment]::GetEnvironmentVariable('Path', 'User')
    if ($null -eq $value) {
        return ''
    }
    return [string]$value
}

function Set-PathValue {
    param(
        [string]$StatePath,
        [Parameter(Mandatory = $true)] [string]$Value
    )

    if (-not [string]::IsNullOrWhiteSpace($StatePath)) {
        $path = ConvertTo-AbsolutePath $StatePath
        Publish-TextFile -Destination $path -Content $Value
        return
    }
    [Environment]::SetEnvironmentVariable('Path', $Value, 'User')
}

function Test-PathTokenMatches {
    param(
        [string]$Token,
        [Parameter(Mandatory = $true)] [string]$Expected
    )

    $candidate = $Token.Trim()
    if ($candidate.Length -ge 2 -and $candidate[0] -eq '"' -and $candidate[$candidate.Length - 1] -eq '"') {
        $candidate = $candidate.Substring(1, $candidate.Length - 2)
    }
    if ([string]::IsNullOrWhiteSpace($candidate)) {
        return $false
    }
    try {
        return (Get-PathKey $candidate) -ieq (Get-PathKey $Expected)
    }
    catch {
        return $false
    }
}

function Add-UserPathEntry {
    param(
        [Parameter(Mandatory = $true)] [string]$BinPath,
        [string]$StatePath
    )

    $before = Get-PathValue $StatePath
    $parts = if ($before.Length -eq 0) { @() } else { $before.Split(';', [StringSplitOptions]::None) }
    foreach ($part in $parts) {
        if (Test-PathTokenMatches $part $BinPath) {
            return [pscustomobject]@{ Before = $before; After = $before; Changed = $false }
        }
    }
    $after = if ($before.Length -eq 0) { $BinPath } elseif ($before.EndsWith(';')) { $before + $BinPath } else { $before + ';' + $BinPath }
    Set-PathValue -StatePath $StatePath -Value $after
    return [pscustomobject]@{ Before = $before; After = $after; Changed = $true }
}

function Remove-UserPathEntry {
    param(
        [Parameter(Mandatory = $true)] [string]$BinPath,
        [string]$StatePath,
        [bool]$OnlyIfAdded = $true
    )

    $before = Get-PathValue $StatePath
    if ($OnlyIfAdded -and $before.Length -eq 0) {
        return [pscustomobject]@{ Before = $before; After = $before; Changed = $false }
    }
    $parts = if ($before.Length -eq 0) { @() } else { $before.Split(';', [StringSplitOptions]::None) }
    $removed = $false
    $kept = [Collections.Generic.List[string]]::new()
    foreach ($part in $parts) {
        if (-not $removed -and (Test-PathTokenMatches $part $BinPath)) {
            $removed = $true
        }
        else {
            [void]$kept.Add($part)
        }
    }
    if (-not $removed) {
        return [pscustomobject]@{ Before = $before; After = $before; Changed = $false }
    }
    $after = [string]::Join(';', $kept)
    Set-PathValue -StatePath $StatePath -Value $after
    return [pscustomobject]@{ Before = $before; After = $after; Changed = $true }
}

function Get-Receipt {
    param([Parameter(Mandatory = $true)] [string]$Path)

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        return $null
    }
    try {
        return (Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json)
    }
    catch {
        throw "The existing cap install receipt is unreadable: $Path"
    }
}

function Get-PackageRootForSource {
    param([Parameter(Mandatory = $true)] [string]$Source)

    $parent = [IO.Path]::GetDirectoryName([IO.Path]::GetFullPath($Source))
    if (Test-Path -LiteralPath (Join-Path $parent 'checksums.sha256') -PathType Leaf) {
        return Get-PathKey $parent
    }
    $grandparent = [IO.Path]::GetDirectoryName([IO.Path]::GetFullPath($parent))
    if (Test-Path -LiteralPath (Join-Path $grandparent 'checksums.sha256') -PathType Leaf) {
        return Get-PathKey $grandparent
    }
    return $null
}

function Get-RelativePath {
    param(
        [Parameter(Mandatory = $true)] [string]$Path,
        [Parameter(Mandatory = $true)] [string]$Root
    )

    $pathKey = Assert-UnderRoot $Path $Root
    $rootKey = Get-PathKey (ConvertTo-AbsolutePath $Root)
    if ($pathKey -eq $rootKey) {
        return ''
    }
    return $pathKey.Substring($rootKey.Length + 1)
}

function Test-FileAvailableForReplacement {
    param([Parameter(Mandatory = $true)] [string]$Path)

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        return
    }
    $stream = $null
    try {
        $stream = [IO.File]::Open($Path, [IO.FileMode]::Open, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
    }
    catch {
        throw "Cannot replace $Path because it is running or in use. Close cap and try again."
    }
    finally {
        if ($null -ne $stream) {
            $stream.Dispose()
        }
    }
}

function Get-ExistingCapCommandPaths {
    $paths = [Collections.Generic.List[string]]::new()
    foreach ($name in @('cap', 'cap.exe')) {
        foreach ($command in @(Get-Command $name -All -ErrorAction SilentlyContinue)) {
            if ($command.CommandType -ne 'Application') {
                continue
            }
            try {
                [void]$paths.Add((Get-PathKey $command.Source))
            }
            catch {
                continue
            }
        }
    }
    return @($paths | Select-Object -Unique)
}

function Confirm-ExecutableReplacement {
    param([string]$Path)

    [Console]::WriteLine("An existing cap.exe was found at: $Path")
    [Console]::Write('Replace this cap.exe with the packaged version? [y/N]: ')
    $answer = [Console]::ReadLine()
    if ($null -eq $answer -or $answer.Trim() -notmatch '^(?i:y|yes)$') {
        [Console]::WriteLine('Installation cancelled. The existing installation was left unchanged.')
        exit 2
    }
}

function Replace-StandaloneExecutable {
    param([string]$Source, [string]$Destination, [string]$SourceHash)

    # A manually installed PATH executable has no managed install directory.
    # Replace that exact file without adopting its directory or changing PATH.
    Assert-NoReparsePath $Destination
    Test-FileAvailableForReplacement $Destination
    $previousHash = Get-Sha256 $Destination
    if ($WhatIfPreference) { return }
    Confirm-ExecutableReplacement $Destination
    $temporary = "$Destination.tmp.$([guid]::NewGuid().ToString('N'))"
    $backup = "$Destination.previous.$([guid]::NewGuid().ToString('N'))"
    try {
        Copy-Item -LiteralPath $Source -Destination $temporary
        if ((Get-Sha256 $temporary) -ine $SourceHash) { throw 'The source executable changed; install was aborted.' }
        Test-FileAvailableForReplacement $Destination
        if ((Get-Sha256 $Destination) -ine $previousHash) { throw 'The existing cap.exe changed after confirmation. Run the installer again.' }
        [IO.File]::Replace($temporary, $Destination, $backup, $true)
        try { Remove-Item -LiteralPath $backup -Force }
        catch { Write-Warning "cap was updated, but its previous binary remains at $backup." }
    }
    finally {
        if (Test-Path -LiteralPath $temporary -PathType Leaf) { Remove-Item -LiteralPath $temporary -Force }
    }
    if ($PassThru) {
        [pscustomobject]@{ ok = $true; action = 'updated'; binaryPath = $Destination; sha256 = $SourceHash; pathUpdated = $false; receiptPath = $null } | ConvertTo-Json
    }
    else { Write-Output "cap updated at $Destination (SHA-256 $SourceHash)" }
}

function Copy-PackageFile {
    param(
        [Parameter(Mandatory = $true)] [string]$Source,
        [Parameter(Mandatory = $true)] [string]$Destination,
        [Parameter(Mandatory = $true)] [string]$Root
    )

    Assert-UnderRoot $Destination $Root | Out-Null
    $parent = [IO.Path]::GetDirectoryName([IO.Path]::GetFullPath($Destination))
    New-DirectoryLiteral $parent | Out-Null
    $temporary = "$Destination.tmp.$([guid]::NewGuid().ToString('N'))"
    Copy-Item -LiteralPath $Source -Destination $temporary -Force
    try {
        if (Test-Path -LiteralPath $Destination -PathType Leaf) {
            Test-FileAvailableForReplacement $Destination
            $backup = "$Destination.previous.$([guid]::NewGuid().ToString('N'))"
            [IO.File]::Replace($temporary, $Destination, $backup, $true)
            if (Test-Path -LiteralPath $backup -PathType Leaf) { Remove-Item -LiteralPath $backup -Force }
        }
        else {
            [IO.File]::Move($temporary, $Destination)
        }
    }
    finally {
        if (Test-Path -LiteralPath $temporary -PathType Leaf) {
            Remove-Item -LiteralPath $temporary -Force -ErrorAction SilentlyContinue
        }
    }
}

function Write-CompletionActivation {
    param(
        [Parameter(Mandatory = $true)] [string]$CapPath,
        [Parameter(Mandatory = $true)] [string]$InstallRoot,
        [string]$ProfilePath,
        [string]$PackageRoot
    )

    if ([string]::IsNullOrWhiteSpace($ProfilePath)) {
        $ProfilePath = $PROFILE
    }
    $profileAbsolute = ConvertTo-AbsolutePath $ProfilePath
    $completionPath = Join-Path $InstallRoot 'cap-completions.ps1'
    if (-not [string]::IsNullOrWhiteSpace($PackageRoot) -and (Test-Path -LiteralPath (Join-Path $PackageRoot 'cap-completions.ps1') -PathType Leaf)) {
        Copy-PackageFile -Source (Join-Path $PackageRoot 'cap-completions.ps1') -Destination $completionPath -Root $InstallRoot
    }
    else {
        $completionOutput = & $CapPath completions powershell 2>&1
        if ($LASTEXITCODE -ne 0) {
            throw "cap completions powershell failed with exit code $LASTEXITCODE"
        }
        Publish-TextFile -Destination $completionPath -Content ([string]::Join([Environment]::NewLine, @($completionOutput)) + [Environment]::NewLine)
    }
    $escaped = $completionPath.Replace("'", "''")
    $marker = "# cap completion activation (managed by cap installer)"
    $line = "& '$escaped'"
    $existing = if (Test-Path -LiteralPath $profileAbsolute -PathType Leaf) { [IO.File]::ReadAllText($profileAbsolute) } else { '' }
    if ($existing -notmatch [regex]::Escape($marker)) {
        $separator = if ($existing.Length -eq 0 -or $existing.EndsWith("`n") -or $existing.EndsWith("`r")) { '' } else { [Environment]::NewLine }
        Publish-TextFile -Destination $profileAbsolute -Content ($existing + $separator + $marker + [Environment]::NewLine + $line + [Environment]::NewLine)
    }
    return [pscustomobject]@{ Path = $completionPath; Profile = $profileAbsolute; Marker = $marker; Line = $line }
}

$repoRoot = Get-PathKey ([IO.Path]::GetDirectoryName([IO.Path]::GetFullPath($PSScriptRoot)))
if ([string]::IsNullOrWhiteSpace($SourcePath)) {
    $candidatePaths = @(
        (Join-Path $PSScriptRoot '..\target\release\cap.exe'),
        (Join-Path $PSScriptRoot 'bin\cap.exe')
    )
    foreach ($candidate in $candidatePaths) {
        $candidateAbsolute = ConvertTo-AbsolutePath $candidate
        if (Test-Path -LiteralPath $candidateAbsolute -PathType Leaf) {
            $SourcePath = $candidateAbsolute
            break
        }
    }
}
$source = ConvertTo-AbsolutePath $SourcePath -MustExist
$package = if ([string]::IsNullOrWhiteSpace($PackageRoot)) { Get-PackageRootForSource $source } else { ConvertTo-AbsolutePath $PackageRoot }
if ($null -ne $package -and -not (Test-Path -LiteralPath $package -PathType Container)) {
    throw "Package root not found: $package"
}
if ([string]::IsNullOrWhiteSpace($ChecksumPath) -and $null -ne $package) {
    $candidateChecksum = Join-Path $package 'checksums.sha256'
    if (Test-Path -LiteralPath $candidateChecksum -PathType Leaf) {
        $ChecksumPath = $candidateChecksum
    }
}
$expectedHash = $null
if (-not [string]::IsNullOrWhiteSpace($ChecksumPath)) {
    $checksumAbsolute = ConvertTo-AbsolutePath $ChecksumPath -MustExist
    $expectedHash = Read-ExpectedSha256 -Path $checksumAbsolute -FileName 'cap.exe'
}
$sourceHash = Get-Sha256 $source
if ($null -ne $expectedHash -and $sourceHash -ine $expectedHash) {
    throw "Source SHA-256 mismatch. Expected $expectedHash, got $sourceHash."
}

if ([string]::IsNullOrWhiteSpace($InstallRoot)) {
    if ([string]::IsNullOrWhiteSpace($env:LOCALAPPDATA)) {
        throw 'LOCALAPPDATA is not set. Pass -InstallRoot explicitly for a per-user install.'
    }
    $InstallRoot = Join-Path $env:LOCALAPPDATA 'Programs\cap'
    if ($PromptForReplace -and -not $Force -and -not (Test-Path -LiteralPath (Join-Path $InstallRoot 'bin\cap.exe'))) {
        $existingCommand = Get-ExistingCapCommandPaths | Where-Object { $_ -ine $source -and [IO.Path]::GetFileName($_) -ieq 'cap.exe' } | Select-Object -First 1
        if ($null -ne $existingCommand) {
            $existingBin = [IO.Path]::GetDirectoryName($existingCommand)
            $existingRoot = [IO.Path]::GetDirectoryName($existingBin)
            $managedReceipt = if ([IO.Path]::GetFileName($existingBin) -ieq 'bin') { Get-Receipt (Join-Path $existingRoot '.cap-install.json') } else { $null }
            if ($null -ne $managedReceipt) {
                if ((Get-PathKey ([string]$managedReceipt.binaryPath)) -ine $existingCommand -or
                    (Get-PathKey ([string]$managedReceipt.installRoot)) -ine $existingRoot) {
                    throw "The existing cap receipt does not match $existingCommand."
                }
                $InstallRoot = $existingRoot
            }
            else {
                if ($ActivateCompletions) { throw 'Completion activation requires a managed installation. Use -InstallRoot to choose one.' }
                Replace-StandaloneExecutable -Source $source -Destination $existingCommand -SourceHash $sourceHash
                return
            }
        }
    }
}
$root = Assert-SafeInstallRoot $InstallRoot
Assert-NoReparsePath $root
$bin = Get-PathKey (Join-Path $root 'bin')
$destination = Get-PathKey (Join-Path $bin 'cap.exe')
$receiptPath = Get-PathKey (Join-Path $root '.cap-install.json')
if ((Test-Path -LiteralPath $root) -and -not (Test-Path -LiteralPath $root -PathType Container)) {
    throw "The requested install root is not a directory: $root"
}
if ((Test-Path -LiteralPath $bin) -and -not (Test-Path -LiteralPath $bin -PathType Container)) {
    throw "The requested install bin path is not a directory: $bin"
}
Assert-NoReparsePath $bin
Assert-NoReparsePath $destination
Assert-NoReparsePath $receiptPath
Assert-UnderRoot $bin $root | Out-Null
Assert-UnderRoot $destination $root | Out-Null
Assert-UnderRoot $receiptPath $root | Out-Null

if ((Get-PathKey $source) -ieq $destination) {
    throw 'The source executable is already the destination. Install from a build or package copy.'
}

$existingReceipt = Get-Receipt $receiptPath
$destinationExists = Test-Path -LiteralPath $destination -PathType Leaf
$replacementApproved = $false
$approvedBinaryHash = $null
if ($destinationExists -and $PromptForReplace -and -not $Force -and -not $WhatIfPreference) {
    Test-FileAvailableForReplacement $destination
    $approvedBinaryHash = Get-Sha256 $destination
    Confirm-ExecutableReplacement $destination
    $replacementApproved = $true
}
if ($destinationExists -and $null -eq $existingReceipt -and -not $Force -and -not $replacementApproved) {
    throw "An existing cap.exe was found without a cap install receipt: $destination. Use -Force only after reviewing it."
}

function Assert-FileDestination {
    param([Parameter(Mandatory = $true)] [string]$Path)

    if (Test-Path -LiteralPath $Path -PathType Container) {
        throw "A directory already occupies the expected cap file destination: $Path"
    }
    if (Test-Path -LiteralPath $Path -PathType Leaf) {
        Test-FileAvailableForReplacement $Path
    }
}

function Capture-FileSnapshot {
    param([Parameter(Mandatory = $true)] [string]$Path)

    if (Test-Path -LiteralPath $Path -PathType Leaf) {
        return [pscustomobject]@{ Exists = $true; Bytes = [IO.File]::ReadAllBytes($Path) }
    }
    if (Test-Path -LiteralPath $Path) {
        throw "Expected a file or an absent path, but found a non-file destination: $Path"
    }
    return [pscustomobject]@{ Exists = $false; Bytes = $null }
}

function Restore-FileSnapshot {
    param(
        [Parameter(Mandatory = $true)] [string]$Path,
        [Parameter(Mandatory = $true)] $Snapshot
    )

    if ($Snapshot.Exists) {
        Assert-FileDestination $Path
        [IO.File]::WriteAllBytes($Path, $Snapshot.Bytes)
    }
    elseif (Test-Path -LiteralPath $Path -PathType Leaf) {
        Test-FileAvailableForReplacement $Path
        Remove-Item -LiteralPath $Path -Force
    }
}

function Remove-EmptyInstallDirectories {
    param(
        [Parameter(Mandatory = $true)] [string]$Root,
        [Parameter(Mandatory = $true)] [string]$Bin,
        [bool]$RootExisted,
        [bool]$BinExisted
    )

    if (-not $BinExisted -and (Test-Path -LiteralPath $Bin -PathType Container) -and (@(Get-ChildItem -LiteralPath $Bin -Force).Count -eq 0)) {
        Remove-Item -LiteralPath $Bin -Force -ErrorAction SilentlyContinue
    }
    if (-not $RootExisted -and (Test-Path -LiteralPath $Root -PathType Container) -and (@(Get-ChildItem -LiteralPath $Root -Force).Count -eq 0)) {
        Remove-Item -LiteralPath $Root -Force -ErrorAction SilentlyContinue
    }
}
if ($destinationExists) {
    Assert-FileDestination $destination
}
if ($destinationExists -and $null -ne $existingReceipt -and -not $Force -and -not $replacementApproved -and -not [string]::IsNullOrWhiteSpace([string]$existingReceipt.binarySha256)) {
    $currentBinaryHash = Get-Sha256 $destination
    if ($currentBinaryHash -ine [string]$existingReceipt.binarySha256) {
        throw "The existing cap.exe does not match its install receipt. Use -Force only after reviewing the replacement."
    }
}
if (-not $destinationExists -and -not $Force) {
    foreach ($existingCommand in (Get-ExistingCapCommandPaths)) {
        if ($existingCommand -ine $destination) {
            throw "A different cap command already resolves at $existingCommand. Use -Force only after reviewing the collision."
        }
    }
}

$rootExistedBefore = Test-Path -LiteralPath $root -PathType Container
$binExistedBefore = Test-Path -LiteralPath $bin -PathType Container
$originalReceipt = Capture-FileSnapshot $receiptPath
$originalFiles = @{}
$plannedPaths = [Collections.Generic.List[string]]::new()
$copyCandidates = @()
$copyPlan = [Collections.Generic.List[object]]::new()
$metadataRelative = [Collections.Generic.List[string]]::new()
$profileActivation = $null
$profilePathForPlan = $null

if ($null -ne $package) {
    if ((Get-PathKey $package) -ieq $root -or (Get-PathKey $package).StartsWith($root + [char]92, [StringComparison]::OrdinalIgnoreCase) -or $root.StartsWith((Get-PathKey $package) + [char]92, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'The package source and install root must not overlap.'
    }
    $copyCandidates = @(
        @{ Relative = 'checksums.sha256'; Source = (Join-Path $package 'checksums.sha256') },
        @{ Relative = 'manifest.json'; Source = (Join-Path $package 'manifest.json') },
        @{ Relative = 'NOTICE.txt'; Source = (Join-Path $package 'NOTICE.txt') },
        @{ Relative = 'docs\provenance\color-cli.md'; Source = (Join-Path $package 'docs\provenance\color-cli.md') },
        @{ Relative = 'docs\windows-install.md'; Source = (Join-Path $package 'docs\windows-install.md') },
        @{ Relative = 'uninstall.ps1'; Source = (Join-Path $package 'uninstall.ps1') }
    ) | Where-Object { Test-Path -LiteralPath $_.Source -PathType Leaf }
    foreach ($candidate in @($copyCandidates)) {
        $target = Get-PathKey (Join-Path $root $candidate.Relative)
        Assert-UnderRoot $target $root | Out-Null
        Assert-NoReparsePath $target
        Assert-FileDestination $target
        $wasOwned = $false
        if ($null -ne $existingReceipt -and $null -ne $existingReceipt.files) {
            $wasOwned = @($existingReceipt.files) -contains $candidate.Relative
        }
        if ((Test-Path -LiteralPath $target -PathType Leaf) -and -not $wasOwned -and -not $Force) {
            throw "Refusing to overwrite unrelated install content: $target"
        }
        [void]$copyPlan.Add([pscustomobject]@{ Relative = $candidate.Relative; Source = $candidate.Source; Target = $target })
        [void]$metadataRelative.Add($candidate.Relative)
        [void]$plannedPaths.Add($target)
    }
}

if ($null -ne $existingReceipt -and $null -ne $existingReceipt.files) {
    foreach ($previousRelative in @($existingReceipt.files)) {
        $previous = ([string]$previousRelative).Replace('/', '\')
        if ($previous -ieq 'bin\cap.exe' -or $metadataRelative -contains $previous) { continue }
        $previousTarget = Assert-UnderRoot (Join-Path $root $previous) $root
        Assert-NoReparsePath $previousTarget
        if (Test-Path -LiteralPath $previousTarget -PathType Leaf) {
            [void]$metadataRelative.Add($previous)
        }
    }
}

if ($ActivateCompletions) {
    $profilePathForPlan = if ([string]::IsNullOrWhiteSpace($CompletionProfilePath)) { $PROFILE } else { $CompletionProfilePath }
    $profilePathForPlan = ConvertTo-AbsolutePath $profilePathForPlan
    Assert-NoReparsePath $profilePathForPlan
    if (Test-Path -LiteralPath $profilePathForPlan -PathType Container) {
        throw "Completion profile path is a directory: $profilePathForPlan"
    }
    if (Test-Path -LiteralPath $profilePathForPlan -PathType Leaf) {
        Test-FileAvailableForReplacement $profilePathForPlan
    }
    $completionPath = Get-PathKey (Join-Path $root 'cap-completions.ps1')
    Assert-NoReparsePath $completionPath
    Assert-FileDestination $completionPath
    [void]$metadataRelative.Add('cap-completions.ps1')
    [void]$plannedPaths.Add($completionPath)
    [void]$plannedPaths.Add($profilePathForPlan)
}
Assert-FileDestination $receiptPath
if (-not $SkipPath -and -not [string]::IsNullOrWhiteSpace($PathStatePath)) {
    $pathStateAbsolute = ConvertTo-AbsolutePath $PathStatePath
    Assert-NoReparsePath $pathStateAbsolute
    Assert-FileDestination $pathStateAbsolute
}
foreach ($plannedPath in @($plannedPaths | Select-Object -Unique)) {
    $originalFiles[$plannedPath] = Capture-FileSnapshot $plannedPath
}
$oldPathValue = Get-PathValue $PathStatePath
$oldUserPathValue = [Environment]::GetEnvironmentVariable('Path', 'User')
$pathStateAbsolute = if ([string]::IsNullOrWhiteSpace($PathStatePath)) { $null } else { ConvertTo-AbsolutePath $PathStatePath }
$pathStateExisted = if ($null -eq $pathStateAbsolute) { $false } else { Test-Path -LiteralPath $pathStateAbsolute -PathType Leaf }

if ($WhatIfPreference) {
    return
}

$pathChanged = $false
$pathTouched = $false
$backup = $null
$temporary = $null
$binaryPublished = $false
$committed = $false
try {
    New-DirectoryLiteral $root | Out-Null
    New-DirectoryLiteral $bin | Out-Null
    Test-FileAvailableForReplacement $destination
    $temporary = "$destination.tmp.$([guid]::NewGuid().ToString('N'))"
    Copy-Item -LiteralPath $source -Destination $temporary -Force
    $stagedHash = Get-Sha256 $temporary
    if ($stagedHash -ine $sourceHash) {
        throw 'The staged executable hash changed while copying; install was aborted.'
    }
    if ($destinationExists) {
        if ($replacementApproved -and (Get-Sha256 $destination) -ine $approvedBinaryHash) {
            throw 'The existing cap.exe changed after confirmation. Run the installer again.'
        }
        $backup = "$destination.previous.$([guid]::NewGuid().ToString('N'))"
        Assert-UnderRoot $backup $root | Out-Null
        [IO.File]::Move($destination, $backup)
    }
    [IO.File]::Move($temporary, $destination)
    $temporary = $null
    $binaryPublished = $true

    if (-not $SkipPath) {
        $pathTouched = $true
        $pathResult = Add-UserPathEntry -BinPath $bin -StatePath $PathStatePath
        $pathChanged = $pathResult.Changed
    }

    foreach ($candidate in $copyPlan) {
        Copy-PackageFile -Source $candidate.Source -Destination $candidate.Target -Root $root
    }

    if ($ActivateCompletions) {
        $profileActivation = Write-CompletionActivation -CapPath $destination -InstallRoot $root -ProfilePath $profilePathForPlan -PackageRoot $package
    }

    $profileActivation = if ($ActivateCompletions) { $profileActivation } elseif ($null -ne $existingReceipt -and $null -ne $existingReceipt.completionActivation) { $existingReceipt.completionActivation } else { $null }

    $allFiles = [Collections.Generic.List[string]]::new()
    [void]$allFiles.Add('bin\cap.exe')
    foreach ($metadata in $metadataRelative) {
        if (-not ($allFiles -contains $metadata)) {
            [void]$allFiles.Add($metadata)
        }
    }
    $fileHashes = [ordered]@{}
    foreach ($relative in $allFiles) {
        $ownedPath = Assert-UnderRoot (Join-Path $root $relative) $root
        if (Test-Path -LiteralPath $ownedPath -PathType Leaf) {
            $fileHashes[$relative] = Get-Sha256 $ownedPath
        }
    }
    $receipt = [ordered]@{
        schemaVersion = 1
        installerVersion = '0.2.0-dev.23'
        installedAtUtc = (Get-Date).ToUniversalTime().ToString('o')
        installRoot = $root
        binPath = $bin
        binaryPath = $destination
        binarySha256 = $sourceHash
        files = @($allFiles.ToArray())
        fileHashes = $fileHashes
        pathEntry = ($pathChanged -or ($null -ne $existingReceipt -and $existingReceipt.pathEntry -eq $true))
        pathStatePath = if ([string]::IsNullOrWhiteSpace($PathStatePath)) { $null } else { (ConvertTo-AbsolutePath $PathStatePath) }
        completionActivation = $profileActivation
    }
    Publish-TextFile -Destination $receiptPath -Content (($receipt | ConvertTo-Json -Depth 8) + [Environment]::NewLine)
    $committed = $true

    if ($null -ne $backup -and (Test-Path -LiteralPath $backup -PathType Leaf)) {
        try {
            Test-FileAvailableForReplacement $backup
            Remove-Item -LiteralPath $backup -Force
        }
        catch {
            Write-Warning "cap was committed, but the previous binary could not be cleaned up: $backup. It is safe to remove after reviewing it."
        }
    }
}
catch {
    $failure = $_
    if ($committed) {
        Write-Warning "cap was committed before a cleanup error: $($failure.Exception.Message)"
        throw $failure
    }
    if ($null -ne $temporary -and (Test-Path -LiteralPath $temporary -PathType Leaf)) {
        Remove-Item -LiteralPath $temporary -Force -ErrorAction SilentlyContinue
    }
    if ($binaryPublished -and (Test-Path -LiteralPath $destination -PathType Leaf)) {
        try {
            Test-FileAvailableForReplacement $destination
            Remove-Item -LiteralPath $destination -Force -ErrorAction Stop
        }
        catch {
            Write-Warning "Could not remove the newly published binary during rollback: $destination. Review it before retrying."
        }
    }
    if ($null -ne $backup -and (Test-Path -LiteralPath $backup -PathType Leaf)) {
        try {
            if (Test-Path -LiteralPath $destination -PathType Leaf) { Test-FileAvailableForReplacement $destination }
            [IO.File]::Move($backup, $destination)
        }
        catch {
            Write-Warning "Could not restore the previous binary during rollback: $destination. The backup remains at $backup."
        }
    }
    foreach ($originalPath in @($originalFiles.Keys)) {
        try { Restore-FileSnapshot -Path $originalPath -Snapshot $originalFiles[$originalPath] }
        catch { Write-Warning "Could not restore the original install file ${originalPath}: $($_.Exception.Message)" }
    }
    try {
        Restore-FileSnapshot -Path $receiptPath -Snapshot $originalReceipt
    }
    catch {
        Write-Warning "Could not restore the original install receipt ${receiptPath}: $($_.Exception.Message)"
    }
    if ($pathTouched) {
        try {
            if ($null -ne $pathStateAbsolute) {
                if ($pathStateExisted) { Publish-TextFile -Destination $pathStateAbsolute -Content $oldPathValue }
                elseif (Test-Path -LiteralPath $pathStateAbsolute -PathType Leaf) {
                    Remove-Item -LiteralPath $pathStateAbsolute -Force
                }
            }
            else {
                [Environment]::SetEnvironmentVariable('Path', $oldUserPathValue, 'User')
            }
        }
        catch {
            Write-Warning "Could not restore the original user PATH: $($_.Exception.Message)"
        }
    }
    Remove-EmptyInstallDirectories -Root $root -Bin $bin -RootExisted $rootExistedBefore -BinExisted $binExistedBefore
    throw $failure
}

$result = [pscustomobject]@{
    ok = $true
    action = if ($destinationExists) { 'updated' } else { 'installed' }
    installRoot = $root
    binaryPath = $destination
    sha256 = $sourceHash
    pathUpdated = $pathChanged
    completionActivated = $ActivateCompletions.IsPresent
    receiptPath = $receiptPath
}
if ($PassThru) {
    $result | ConvertTo-Json -Depth 8
}
else {
    Write-Output ("cap {0} at {1} (SHA-256 {2})" -f $result.action, $destination, $sourceHash)
    if (-not $SkipPath) {
        Write-Output "User PATH contains $bin (a fresh PowerShell session will see it)."
    }
    if ($ActivateCompletions) {
        Write-Output 'PowerShell completion activation was enabled explicitly.'
    }
}
