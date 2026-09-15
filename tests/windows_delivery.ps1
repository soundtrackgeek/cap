[CmdletBinding()]
param(
    [string]$InstallScriptPath,
    [string]$UninstallScriptPath,
    [switch]$KeepArtifacts
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Assert-Condition {
    param([bool]$Condition, [string]$Message)
    if (-not $Condition) { throw "FAIL: $Message" }
}

function Expect-Failure {
    param([scriptblock]$Action, [string]$Message)
    $failed = $false
    try { & $Action } catch { $failed = $true }
    Assert-Condition $failed $Message
}

function Get-Sha256 {
    param([string]$Path)
    try { return (Get-FileHash -LiteralPath $Path -Algorithm SHA256 -ErrorAction Stop).Hash.ToLowerInvariant() }
    catch {
        $algorithm = [Security.Cryptography.SHA256]::Create()
        $stream = [IO.File]::OpenRead($Path)
        try { return ([BitConverter]::ToString($algorithm.ComputeHash($stream))).Replace('-', '').ToLowerInvariant() }
        finally { $stream.Dispose(); $algorithm.Dispose() }
    }
}

if ([string]::IsNullOrWhiteSpace($InstallScriptPath)) { $InstallScriptPath = Join-Path $PSScriptRoot '..\scripts\install.ps1' }
if ([string]::IsNullOrWhiteSpace($UninstallScriptPath)) { $UninstallScriptPath = Join-Path $PSScriptRoot '..\scripts\uninstall.ps1' }
$installScript = [IO.Path]::GetFullPath($InstallScriptPath)
$uninstallScript = [IO.Path]::GetFullPath($UninstallScriptPath)
if (-not (Test-Path -LiteralPath $installScript -PathType Leaf)) { throw "Install script not found: $installScript" }
if (-not (Test-Path -LiteralPath $uninstallScript -PathType Leaf)) { throw "Uninstall script not found: $uninstallScript" }

$tempRoot = [IO.Path]::GetFullPath((Join-Path ([IO.Path]::GetTempPath()) ("cap-delivery-tests-" + [guid]::NewGuid().ToString('N'))))
 [IO.Directory]::CreateDirectory($tempRoot) | Out-Null
$marker = Join-Path $tempRoot '.cap-delivery-tests-owned'
[IO.File]::WriteAllText($marker, '')
$holderStream = $null
$receiptHolderStream = $null
$lateSourceHolder = $null
$originalProcessPath = $env:PATH
try {
    # Keep a real installed cap from colliding with the disposable install roots.
    $env:PATH = Join-Path $env:SystemRoot 'System32'
    $sourceDirectory = Join-Path $tempRoot 'source with spaces'
    [IO.Directory]::CreateDirectory($sourceDirectory) | Out-Null
    $source = Join-Path $sourceDirectory 'cap.exe'
    [IO.File]::WriteAllBytes($source, [Text.Encoding]::UTF8.GetBytes('cap-test-v1'))
    $checksum = Join-Path $sourceDirectory 'checksums.sha256'
    $sourceHash = Get-Sha256 $source
    [IO.File]::WriteAllText($checksum, "$sourceHash  cap.exe`n", [Text.UTF8Encoding]::new($false))

    $installRoot = Join-Path $tempRoot 'Programs with spaces\cap'
    $pathState = Join-Path $tempRoot 'user PATH.txt'
    $profilePath = Join-Path $tempRoot 'profile.ps1'
    $originalPath = 'C:\Windows\System32;C:\Tools With Spaces;;C:\Legacy'
    [IO.File]::WriteAllText($pathState, $originalPath, [Text.UTF8Encoding]::new($false))
    $journalSentinel = Join-Path $tempRoot 'journal-recovery-settings'
    [IO.Directory]::CreateDirectory($journalSentinel) | Out-Null
    [IO.File]::WriteAllText((Join-Path $journalSentinel 'capsule.db'), 'must survive', [Text.UTF8Encoding]::new($false))

    $first = & $installScript -SourcePath $source -ChecksumPath $checksum -InstallRoot $installRoot -PathStatePath $pathState -PassThru | ConvertFrom-Json
    Assert-Condition ($first.ok -eq $true -and $first.action -eq 'installed') 'initial install did not succeed'
    $destination = Join-Path $installRoot 'bin\cap.exe'
    Assert-Condition ((Get-Sha256 $destination) -eq $sourceHash) 'installed binary hash differs'
    Assert-Condition ([IO.File]::ReadAllText($pathState) -eq ($originalPath + ';' + ([IO.Path]::GetDirectoryName([IO.Path]::GetFullPath($destination))))) 'initial PATH update did not preserve exact text'
    Assert-Condition (-not (Test-Path -LiteralPath $profilePath)) 'completion profile changed without opt-in'
    $receipt = Get-Content -LiteralPath (Join-Path $installRoot '.cap-install.json') -Raw | ConvertFrom-Json
    Assert-Condition ($null -ne $receipt.fileHashes) 'install receipt omitted file hashes'

    $secondSourceBytes = [Text.Encoding]::UTF8.GetBytes('cap-test-v2-updated')
    [IO.File]::WriteAllBytes($source, $secondSourceBytes)
    $secondHash = Get-Sha256 $source
    [IO.File]::WriteAllText($checksum, "$secondHash  cap.exe`n", [Text.UTF8Encoding]::new($false))
    $update = & $installScript -SourcePath $source -ChecksumPath $checksum -InstallRoot $installRoot -PathStatePath $pathState -PassThru | ConvertFrom-Json
    Assert-Condition ($update.ok -eq $true -and $update.action -eq 'updated') 'update did not succeed'
    Assert-Condition ((Get-Sha256 $destination) -eq $secondHash) 'updated binary hash differs'
    Assert-Condition ([IO.File]::ReadAllText($pathState) -eq ($originalPath + ';' + ([IO.Path]::GetDirectoryName([IO.Path]::GetFullPath($destination))))) 'repeated install duplicated or rewrote PATH'

    $receiptPath = Join-Path $installRoot '.cap-install.json'
    $receiptBeforeBlockedInstall = [Convert]::ToBase64String([IO.File]::ReadAllBytes($receiptPath))
    $destinationBeforeBlockedInstall = Get-Sha256 $destination
    $pathBeforeBlockedInstall = [IO.File]::ReadAllText($pathState)
    $receiptHolderStream = [IO.File]::Open($receiptPath, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::None)
    $thirdSourceBytes = [Text.Encoding]::UTF8.GetBytes('cap-test-receipt-lock-failure')
    [IO.File]::WriteAllBytes($source, $thirdSourceBytes)
    $thirdHash = Get-Sha256 $source
    [IO.File]::WriteAllText($checksum, "$thirdHash  cap.exe`n", [Text.UTF8Encoding]::new($false))
    Expect-Failure { & $installScript -SourcePath $source -ChecksumPath $checksum -InstallRoot $installRoot -PathStatePath $pathState } 'locked receipt install was accepted'
    $receiptHolderStream.Dispose()
    $receiptHolderStream = $null
    Assert-Condition ((Get-Sha256 $destination) -eq $destinationBeforeBlockedInstall) 'locked receipt install changed the binary'
    Assert-Condition ([Convert]::ToBase64String([IO.File]::ReadAllBytes($receiptPath)) -eq $receiptBeforeBlockedInstall) 'locked receipt install changed receipt bytes'
    Assert-Condition ([IO.File]::ReadAllText($pathState) -eq $pathBeforeBlockedInstall) 'locked receipt install changed PATH state'

    [IO.File]::WriteAllText($checksum, ('0' * 64) + "  cap.exe`n", [Text.UTF8Encoding]::new($false))
    Expect-Failure { & $installScript -SourcePath $source -ChecksumPath $checksum -InstallRoot (Join-Path $tempRoot 'checksum-failure\cap') -PathStatePath $pathState } 'checksum mismatch was accepted'
    Assert-Condition (-not (Test-Path -LiteralPath (Join-Path $tempRoot 'checksum-failure\cap\bin\cap.exe'))) 'checksum failure left an executable behind'
    [IO.File]::WriteAllText($checksum, "$thirdHash  cap.exe`n", [Text.UTF8Encoding]::new($false))

    $metadataPackage = Join-Path $tempRoot 'metadata collision package'
    [IO.Directory]::CreateDirectory($metadataPackage) | Out-Null
    $metadataSource = Join-Path $metadataPackage 'cap.exe'
    [IO.File]::WriteAllBytes($metadataSource, [Text.Encoding]::UTF8.GetBytes('cap-metadata-collision'))
    $metadataHash = Get-Sha256 $metadataSource
    [IO.File]::WriteAllText((Join-Path $metadataPackage 'checksums.sha256'), "$metadataHash  cap.exe`n", [Text.UTF8Encoding]::new($false))
    [IO.File]::WriteAllText((Join-Path $metadataPackage 'manifest.json'), '{"package":"owned-by-test"}', [Text.UTF8Encoding]::new($false))
    $metadataRoot = Join-Path $tempRoot 'metadata collision\cap'
    [IO.Directory]::CreateDirectory($metadataRoot) | Out-Null
    $metadataTarget = Join-Path $metadataRoot 'manifest.json'
    [IO.File]::WriteAllText($metadataTarget, 'unrelated metadata must survive', [Text.UTF8Encoding]::new($false))
    $metadataBefore = [IO.File]::ReadAllText($metadataTarget)
    $metadataPathBefore = [IO.File]::ReadAllText($pathState)
    Expect-Failure { & $installScript -SourcePath $metadataSource -PackageRoot $metadataPackage -InstallRoot $metadataRoot -PathStatePath $pathState } 'unrelated metadata collision was accepted'
    Assert-Condition ([IO.File]::ReadAllText($metadataTarget) -eq $metadataBefore) 'metadata collision changed unrelated content'
    Assert-Condition (-not (Test-Path -LiteralPath (Join-Path $metadataRoot 'bin\cap.exe'))) 'metadata collision left an executable behind'
    Assert-Condition (-not (Test-Path -LiteralPath (Join-Path $metadataRoot '.cap-install.json'))) 'metadata collision left a receipt behind'
    Assert-Condition ([IO.File]::ReadAllText($pathState) -eq $metadataPathBefore) 'metadata collision changed PATH state'

    $latePackage = Join-Path $tempRoot 'late source failure package'
    [IO.Directory]::CreateDirectory($latePackage) | Out-Null
    $lateSource = Join-Path $latePackage 'cap.exe'
    [IO.File]::WriteAllBytes($lateSource, [Text.Encoding]::UTF8.GetBytes('cap-late-source-failure'))
    $lateHash = Get-Sha256 $lateSource
    [IO.File]::WriteAllText((Join-Path $latePackage 'checksums.sha256'), "$lateHash  cap.exe`n", [Text.UTF8Encoding]::new($false))
    $lateManifest = Join-Path $latePackage 'manifest.json'
    [IO.File]::WriteAllText($lateManifest, '{"package":"late-source-failure"}', [Text.UTF8Encoding]::new($false))
    $lateRoot = Join-Path $tempRoot 'late source failure\cap'
    $latePathBefore = [IO.File]::ReadAllText($pathState)
    $lateSourceHolder = [IO.File]::Open($lateManifest, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::None)
    Expect-Failure { & $installScript -SourcePath $lateSource -PackageRoot $latePackage -InstallRoot $lateRoot -PathStatePath $pathState } 'late source failure was accepted'
    $lateSourceHolder.Dispose()
    $lateSourceHolder = $null
    Assert-Condition (-not (Test-Path -LiteralPath (Join-Path $lateRoot 'bin\cap.exe'))) 'late source failure left an executable behind'
    Assert-Condition (-not (Test-Path -LiteralPath (Join-Path $lateRoot '.cap-install.json'))) 'late source failure left a receipt behind'
    Assert-Condition ([IO.File]::ReadAllText($pathState) -eq $latePathBefore) 'late source failure changed PATH state'

    $completionFailureRoot = Join-Path $tempRoot 'completion profile failure\cap'
    $completionProfileDirectory = Join-Path $tempRoot 'completion profile failure\profile directory'
    [IO.Directory]::CreateDirectory($completionProfileDirectory) | Out-Null
    $completionPathBefore = [IO.File]::ReadAllText($pathState)
    Expect-Failure { & $installScript -SourcePath $source -ChecksumPath $checksum -InstallRoot $completionFailureRoot -PathStatePath $pathState -ActivateCompletions -CompletionProfilePath $completionProfileDirectory } 'completion profile directory failure was accepted'
    Assert-Condition (-not (Test-Path -LiteralPath (Join-Path $completionFailureRoot 'bin\cap.exe'))) 'completion profile failure left an executable behind'
    Assert-Condition (-not (Test-Path -LiteralPath (Join-Path $completionFailureRoot '.cap-install.json'))) 'completion profile failure left a receipt behind'
    Assert-Condition ([IO.File]::ReadAllText($pathState) -eq $completionPathBefore) 'completion profile failure changed PATH state'
    Assert-Condition (Test-Path -LiteralPath $completionProfileDirectory -PathType Container) 'completion profile failure removed the profile directory'

    $collisionRoot = Join-Path $tempRoot 'collision\cap'
    [IO.Directory]::CreateDirectory((Join-Path $collisionRoot 'bin')) | Out-Null
    [IO.File]::Copy($source, (Join-Path $collisionRoot 'bin\cap.exe'))
    Expect-Failure { & $installScript -SourcePath $source -InstallRoot $collisionRoot -PathStatePath $pathState } 'unreceipted binary collision was accepted'

    $lockedHash = Get-Sha256 $destination
    $holderStream = [IO.File]::Open($destination, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::None)
    [IO.File]::WriteAllBytes($source, [Text.Encoding]::UTF8.GetBytes('cap-test-lock-failure'))
    $lockSourceHash = Get-Sha256 $source
    [IO.File]::WriteAllText($checksum, "$lockSourceHash  cap.exe`n", [Text.UTF8Encoding]::new($false))
    Expect-Failure { & $installScript -SourcePath $source -InstallRoot $installRoot -PathStatePath $pathState } 'in-use binary replacement was accepted'
    $holderStream.Dispose()
    $holderStream = $null
    Assert-Condition ((Get-Sha256 $destination) -eq $lockedHash) 'failed replacement changed the installed binary'

    $lockedUninstallPath = [IO.File]::ReadAllText($pathState)
    $lockedUninstallReceipt = [Convert]::ToBase64String([IO.File]::ReadAllBytes($receiptPath))
    $holderStream = [IO.File]::Open($destination, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::None)
    Expect-Failure { & $uninstallScript -InstallRoot $installRoot -PathStatePath $pathState } 'locked uninstall was accepted'
    $holderStream.Dispose()
    $holderStream = $null
    Assert-Condition (Test-Path -LiteralPath $destination -PathType Leaf) 'locked uninstall removed the binary'
    Assert-Condition ([IO.File]::ReadAllText($pathState) -eq $lockedUninstallPath) 'locked uninstall changed PATH state'
    Assert-Condition ([Convert]::ToBase64String([IO.File]::ReadAllBytes($receiptPath)) -eq $lockedUninstallReceipt) 'locked uninstall changed receipt bytes'

    $uninstall = & $uninstallScript -InstallRoot $installRoot -PathStatePath $pathState -PassThru | ConvertFrom-Json
    Assert-Condition ($uninstall.ok -eq $true -and $uninstall.action -eq 'uninstalled') 'uninstall did not succeed'
    Assert-Condition (-not (Test-Path -LiteralPath $destination)) 'uninstall left cap.exe behind'
    Assert-Condition ([IO.File]::ReadAllText($pathState) -eq $originalPath) 'uninstall did not restore the exact PATH text'
    Assert-Condition (Test-Path -LiteralPath (Join-Path $journalSentinel 'capsule.db')) 'uninstall touched journal/recovery/settings data'

    [pscustomobject]@{ ok = $true; cases = @('spaces', 'checksum', 'receipt-lock-rollback', 'metadata-collision', 'late-source-rollback', 'completion-profile-failure', 'collision', 'running-binary', 'locked-uninstall', 'path-preservation', 'uninstall-data-safety') } | ConvertTo-Json -Depth 4
}
finally {
    $env:PATH = $originalProcessPath
    if ($null -ne $holderStream) { $holderStream.Dispose() }
    if ($null -ne $receiptHolderStream) { $receiptHolderStream.Dispose() }
    if ($null -ne $lateSourceHolder) { $lateSourceHolder.Dispose() }
    if (-not $KeepArtifacts) {
        if (-not (Test-Path -LiteralPath $marker -PathType Leaf)) { throw "Refusing to remove unmarked test root: $tempRoot" }
        Remove-Item -LiteralPath $tempRoot -Recurse -Force
    }
}
