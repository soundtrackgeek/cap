[CmdletBinding(SupportsShouldProcess = $true, ConfirmImpact = 'High')]
param(
    [string]$InstallRoot,
    [string]$PathStatePath,
    [switch]$KeepPath,
    [switch]$Force,
    [switch]$PassThru
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function ConvertTo-AbsolutePath {
    param([Parameter(Mandatory = $true)] [string]$Path)

    if ([string]::IsNullOrWhiteSpace($Path)) {
        throw 'A non-empty path is required.'
    }
    return [IO.Path]::GetFullPath([Environment]::ExpandEnvironmentVariables($Path))
}

function Get-PathKey {
    param([Parameter(Mandatory = $true)] [string]$Path)

    $absolute = [IO.Path]::GetFullPath($Path).TrimEnd([char[]]@([char]92, [char]47))
    if ($absolute.Length -eq 2 -and $absolute[1] -eq ':') {
        $absolute += [char]92
    }
    return $absolute
}

function Assert-SafeInstallRoot {
    param([Parameter(Mandatory = $true)] [string]$Path)

    $root = Get-PathKey (ConvertTo-AbsolutePath $Path)
    $pathRoot = [IO.Path]::GetPathRoot($root)
    if ([string]::IsNullOrWhiteSpace($pathRoot) -or $root -eq $pathRoot -or $root.Length -lt ($pathRoot.Length + 3)) {
        throw "Refusing a drive or filesystem root as the cap install root: $root"
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

function Publish-TextFile {
    param(
        [Parameter(Mandatory = $true)] [string]$Destination,
        [Parameter(Mandatory = $true)] [AllowEmptyString()] [string]$Content
    )

    $parent = [IO.Path]::GetDirectoryName([IO.Path]::GetFullPath($Destination))
    if (-not (Test-Path -LiteralPath $parent -PathType Container)) {
        [IO.Directory]::CreateDirectory($parent) | Out-Null
    }
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
    if ($null -eq $value) { return '' }
    return [string]$value
}

function Set-PathValue {
    param(
        [string]$StatePath,
        [Parameter(Mandatory = $true)] [string]$Value
    )

    if (-not [string]::IsNullOrWhiteSpace($StatePath)) {
        Publish-TextFile -Destination (ConvertTo-AbsolutePath $StatePath) -Content $Value
    }
    else {
        [Environment]::SetEnvironmentVariable('Path', $Value, 'User')
    }
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
    if ([string]::IsNullOrWhiteSpace($candidate)) { return $false }
    try { return (Get-PathKey $candidate) -ieq (Get-PathKey $Expected) } catch { return $false }
}

function Remove-UserPathEntry {
    param(
        [Parameter(Mandatory = $true)] [string]$BinPath,
        [string]$StatePath
    )

    $before = Get-PathValue $StatePath
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

function Test-FileAvailableForRemoval {
    param([Parameter(Mandatory = $true)] [string]$Path)

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { return }
    $stream = $null
    try {
        $stream = [IO.File]::Open($Path, [IO.FileMode]::Open, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
    }
    catch {
        throw "Cannot remove $Path because it is running or in use. Close cap and try again."
    }
    finally {
        if ($null -ne $stream) { $stream.Dispose() }
    }
}

function Get-Receipt {
    param([Parameter(Mandatory = $true)] [string]$Path)

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { return $null }
    try { return (Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json) }
    catch { throw "The cap install receipt is unreadable: $Path" }
}

function Get-ReceiptFiles {
    param(
        [Parameter(Mandatory = $true)] $Receipt,
        [Parameter(Mandatory = $true)] [string]$Root
    )

    $relative = [Collections.Generic.List[string]]::new()
    if ($null -ne $Receipt.files) {
        foreach ($item in @($Receipt.files)) {
            if ([string]::IsNullOrWhiteSpace([string]$item)) { continue }
            $candidate = ([string]$item).Replace('/', '\')
            if ([IO.Path]::IsPathRooted($candidate) -or $candidate -match '(^|[\\/])\.\.([\\/]|$)') {
                throw "The cap install receipt contains an unsafe path: $candidate"
            }
            $absolute = Join-Path $Root $candidate
            Assert-UnderRoot $absolute $Root | Out-Null
            if (-not ($relative -contains $candidate)) { [void]$relative.Add($candidate) }
        }
    }
    if (-not ($relative -contains 'bin\cap.exe')) { [void]$relative.Insert(0, 'bin\cap.exe') }
    return @($relative)
}

function Remove-ProfileActivation {
    param(
        [Parameter(Mandatory = $true)] $Activation,
        [switch]$AllowModified
    )

    if ($null -eq $Activation -or [string]::IsNullOrWhiteSpace([string]$Activation.Profile)) { return $false }
    $profile = ConvertTo-AbsolutePath ([string]$Activation.Profile)
    if (-not (Test-Path -LiteralPath $profile -PathType Leaf)) { return $false }
    $text = [IO.File]::ReadAllText($profile)
    $marker = [string]$Activation.Marker
    $line = [string]$Activation.Line
    if ([string]::IsNullOrWhiteSpace($marker) -or [string]::IsNullOrWhiteSpace($line)) { return $false }
    $pattern = '(?m)^' + [regex]::Escape($marker) + '\r?\n' + [regex]::Escape($line) + '\r?\n?'
    if ($text -notmatch $pattern) {
        if (-not $AllowModified) { return $false }
        $pattern = '(?m)^' + [regex]::Escape($marker) + '.*(?:\r?\n|$)'
    }
    $updated = [regex]::Replace($text, $pattern, '')
    if ($updated -ne $text) { Publish-TextFile -Destination $profile -Content $updated; return $true }
    return $false
}

if ([string]::IsNullOrWhiteSpace($InstallRoot)) {
    if ([string]::IsNullOrWhiteSpace($env:LOCALAPPDATA)) {
        throw 'LOCALAPPDATA is not set. Pass -InstallRoot explicitly for a per-user uninstall.'
    }
    $InstallRoot = Join-Path $env:LOCALAPPDATA 'Programs\cap'
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
$receipt = Get-Receipt $receiptPath

if ($null -ne $receipt) {
    if (-not [string]::IsNullOrWhiteSpace([string]$receipt.installRoot) -and (Get-PathKey ([string]$receipt.installRoot)) -ine $root) {
        throw "The receipt belongs to a different install root: $($receipt.installRoot)"
    }
    if ([string]::IsNullOrWhiteSpace($PathStatePath) -and $null -ne $receipt.pathStatePath -and -not [string]::IsNullOrWhiteSpace([string]$receipt.pathStatePath)) {
        $PathStatePath = [string]$receipt.pathStatePath
    }
}
elseif (-not (Test-Path -LiteralPath $destination -PathType Leaf)) {
    $result = [pscustomobject]@{ ok = $true; action = 'not-installed'; installRoot = $root; pathUpdated = $false }
    if ($PassThru) { $result | ConvertTo-Json -Depth 8 } else { Write-Output 'cap is not installed at the requested root.' }
    return
}
elseif (-not $Force) {
    throw "No cap install receipt was found for $destination. Use -Force only after reviewing the directory."
}

$files = if ($null -ne $receipt) { Get-ReceiptFiles -Receipt $receipt -Root $root } else { @('bin\cap.exe') }
$preflightSkipped = [Collections.Generic.List[string]]::new()
foreach ($relative in $files) {
    $target = Assert-UnderRoot (Join-Path $root $relative) $root
    Assert-NoReparsePath $target
    if (-not (Test-Path -LiteralPath $target -PathType Leaf)) { continue }
    $expected = $null
    if ($null -ne $receipt -and $null -ne $receipt.fileHashes) {
        $property = $receipt.fileHashes.PSObject.Properties | Where-Object { $_.Name -ieq $relative } | Select-Object -First 1
        if ($null -ne $property) { $expected = [string]$property.Value }
    }
    if ($null -eq $expected -and $relative -ieq 'bin\cap.exe' -and $null -ne $receipt) {
        $expected = [string]$receipt.binarySha256
    }
    if (-not $Force -and $null -ne $expected -and (Get-Sha256 $target) -ine $expected) {
        [void]$preflightSkipped.Add($relative)
        continue
    }
    Test-FileAvailableForRemoval $target
}

$completion = $null
$completionExpected = $null
if ($null -ne $receipt -and $null -ne $receipt.completionActivation -and -not [string]::IsNullOrWhiteSpace([string]$receipt.completionActivation.Path)) {
    $completion = Assert-UnderRoot ([string]$receipt.completionActivation.Path) $root
    Assert-NoReparsePath $completion
    if ($null -ne $receipt.fileHashes) {
        $property = $receipt.fileHashes.PSObject.Properties | Where-Object { $_.Name -ieq 'cap-completions.ps1' } | Select-Object -First 1
        if ($null -ne $property) { $completionExpected = [string]$property.Value }
    }
    if (Test-Path -LiteralPath $completion -PathType Leaf) {
        if (-not $Force -and $null -ne $completionExpected -and (Get-Sha256 $completion) -ine $completionExpected) {
            if (-not ($preflightSkipped -contains 'cap-completions.ps1')) { [void]$preflightSkipped.Add('cap-completions.ps1') }
        }
        else {
            Test-FileAvailableForRemoval $completion
        }
    }
}

$profilePath = $null
if ($null -ne $receipt -and $null -ne $receipt.completionActivation -and -not [string]::IsNullOrWhiteSpace([string]$receipt.completionActivation.Profile)) {
    $profilePath = ConvertTo-AbsolutePath ([string]$receipt.completionActivation.Profile)
    Assert-NoReparsePath $profilePath
    if (Test-Path -LiteralPath $profilePath -PathType Leaf) {
        Test-FileAvailableForRemoval $profilePath
    }
}

$pathStateAbsolute = $null
if (-not [string]::IsNullOrWhiteSpace($PathStatePath)) {
    $pathStateAbsolute = ConvertTo-AbsolutePath $PathStatePath
    Assert-NoReparsePath $pathStateAbsolute
    if ((Test-Path -LiteralPath $pathStateAbsolute) -and -not (Test-Path -LiteralPath $pathStateAbsolute -PathType Leaf)) {
        throw "The PATH state path is not a file: $pathStateAbsolute"
    }
}
$shouldRemovePath = -not $KeepPath -and ($null -eq $receipt -or $receipt.pathEntry -eq $true)
if ($shouldRemovePath -and $null -ne $pathStateAbsolute -and (Test-Path -LiteralPath $pathStateAbsolute -PathType Leaf)) {
    Test-FileAvailableForRemoval $pathStateAbsolute
}
$removeReceipt = $null -ne $receipt -and $preflightSkipped.Count -eq 0
if ($removeReceipt -and (Test-Path -LiteralPath $receiptPath -PathType Leaf)) {
    Test-FileAvailableForRemoval $receiptPath
}

$oldPathValue = Get-PathValue $PathStatePath
$oldUserPathValue = [Environment]::GetEnvironmentVariable('Path', 'User')
$pathStateExisted = $null -ne $pathStateAbsolute -and (Test-Path -LiteralPath $pathStateAbsolute -PathType Leaf)
if ($WhatIfPreference) { return }

$pathChanged = $false
$pathTouched = $false
try {
    $removed = [Collections.Generic.List[string]]::new()
    $skipped = [Collections.Generic.List[string]]::new()
    foreach ($relative in $files) {
        $target = Assert-UnderRoot (Join-Path $root $relative) $root
        if (-not (Test-Path -LiteralPath $target -PathType Leaf)) { continue }
        $expected = $null
        if ($null -ne $receipt -and $null -ne $receipt.fileHashes) {
            $property = $receipt.fileHashes.PSObject.Properties | Where-Object { $_.Name -ieq $relative } | Select-Object -First 1
            if ($null -ne $property) { $expected = [string]$property.Value }
        }
        if ($null -eq $expected -and $relative -ieq 'bin\cap.exe' -and $null -ne $receipt) {
            $expected = [string]$receipt.binarySha256
        }
        if (-not $Force -and $null -ne $expected -and (Get-Sha256 $target) -ine $expected) {
            [void]$skipped.Add($relative)
            continue
        }
        Remove-Item -LiteralPath $target -Force
        [void]$removed.Add($relative)
    }

    $profileRemoved = $false
    if ($null -ne $receipt -and $null -ne $receipt.completionActivation) {
        $profileRemoved = Remove-ProfileActivation -Activation $receipt.completionActivation -AllowModified:$Force
    }

    if ($null -ne $completion -and (Test-Path -LiteralPath $completion -PathType Leaf)) {
        if ($Force -or $null -eq $completionExpected -or (Get-Sha256 $completion) -ieq $completionExpected) {
            Remove-Item -LiteralPath $completion -Force
            [void]$removed.Add('cap-completions.ps1')
        }
        elseif (-not ($skipped -contains 'cap-completions.ps1')) {
            [void]$skipped.Add('cap-completions.ps1')
        }
    }

    if ($null -ne $receipt -and $skipped.Count -eq 0 -and (Test-Path -LiteralPath $receiptPath -PathType Leaf)) {
        Remove-Item -LiteralPath $receiptPath -Force
    }

    if ($shouldRemovePath) {
        $pathTouched = $true
        $pathResult = Remove-UserPathEntry -BinPath $bin -StatePath $PathStatePath
        $pathChanged = $pathResult.Changed
    }

    if ((Test-Path -LiteralPath $bin -PathType Container) -and (@(Get-ChildItem -LiteralPath $bin -Force).Count -eq 0)) {
        Remove-Item -LiteralPath $bin -Force
    }
    if ((Test-Path -LiteralPath $root -PathType Container) -and (@(Get-ChildItem -LiteralPath $root -Force).Count -eq 0)) {
        Remove-Item -LiteralPath $root -Force
    }

    $result = [pscustomobject]@{
        ok = $true
        action = 'uninstalled'
        installRoot = $root
        pathUpdated = $pathChanged
        removed = @($removed)
        skippedModified = @($skipped)
        profileActivationRemoved = $profileRemoved
    }
    if ($PassThru) {
        $result | ConvertTo-Json -Depth 8
    }
    else {
        Write-Output "cap removed from $root."
        if ($skipped.Count -gt 0) { Write-Warning ("Preserved modified installer files: " + ($skipped -join ', ')) }
    }
}
catch {
    $failure = $_
    if ($pathTouched) {
        try {
            if ($null -ne $pathStateAbsolute) {
                if ($pathStateExisted) { Publish-TextFile -Destination $pathStateAbsolute -Content $oldPathValue }
                elseif (Test-Path -LiteralPath $pathStateAbsolute -PathType Leaf) { Remove-Item -LiteralPath $pathStateAbsolute -Force }
            }
            else {
                [Environment]::SetEnvironmentVariable('Path', $oldUserPathValue, 'User')
            }
        }
        catch {
            Write-Warning "Could not restore the original user PATH: $($_.Exception.Message)"
        }
    }
    throw $failure
}
