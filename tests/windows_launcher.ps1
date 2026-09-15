[CmdletBinding()]
param(
    [string]$PackageRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Assert-Condition {
    param([bool]$Condition, [string]$Message)
    if (-not $Condition) { throw "FAIL: $Message" }
}

# Exercise the batch file through cmd.exe, including its pause and exit code.
function Invoke-Launcher {
    param(
        [string]$Launcher,
        [string]$InstallRoot,
        [string]$PathState,
        [string]$Answer = 'y',
        [string]$SearchPath,
        [string]$LocalAppData
    )

    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = Join-Path $env:SystemRoot 'System32\cmd.exe'
    $rootArgument = if ([string]::IsNullOrWhiteSpace($InstallRoot)) { '' } else { ' -InstallRoot "{0}"' -f $InstallRoot }
    $start.Arguments = '/d /s /c ""{0}"{1} -PathStatePath "{2}""' -f $Launcher, $rootArgument, $PathState
    $start.WorkingDirectory = $tempRoot
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardInput = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    # Exclude any real installed cap from collision detection.
    $start.EnvironmentVariables['PATH'] = Join-Path $env:SystemRoot 'System32'
    if ($SearchPath) { $start.EnvironmentVariables['PATH'] = $SearchPath + ';' + $start.EnvironmentVariables['PATH'] }
    if ($LocalAppData) { $start.EnvironmentVariables['LOCALAPPDATA'] = $LocalAppData }
    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $start
    try {
        [void]$process.Start()
        $stdout = $process.StandardOutput.ReadToEndAsync()
        $stderr = $process.StandardError.ReadToEndAsync()
        $process.StandardInput.WriteLine($Answer)
        $process.StandardInput.Close()
        if (-not $process.WaitForExit(30000)) {
            $process.Kill()
            throw 'Batch installation timed out.'
        }
        return [pscustomobject]@{ ExitCode = $process.ExitCode; Output = $stdout.Result + $stderr.Result }
    }
    finally { $process.Dispose() }
}

$tempBase = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\')
$tempRoot = Join-Path $tempBase ('cap-launcher-tests-' + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($tempRoot)
$marker = Join-Path $tempRoot '.cap-launcher-tests-owned'
[IO.File]::WriteAllText($marker, '')
try {
    $package = Join-Path $tempRoot 'Extracted & ready (test)!'
    [void][IO.Directory]::CreateDirectory((Join-Path $package 'bin'))
    if ([string]::IsNullOrWhiteSpace($PackageRoot)) {
        foreach ($name in @('install.bat', 'install.ps1')) {
            Copy-Item -LiteralPath (Join-Path $PSScriptRoot "..\scripts\$name") -Destination (Join-Path $package $name)
        }
        [IO.File]::WriteAllText((Join-Path $package 'bin\cap.exe'), 'synthetic cap binary')
        $hash = (Get-FileHash -LiteralPath (Join-Path $package 'bin\cap.exe') -Algorithm SHA256).Hash.ToLowerInvariant()
        [IO.File]::WriteAllText((Join-Path $package 'checksums.sha256'), "$hash  bin/cap.exe`n")
        [IO.File]::WriteAllText((Join-Path $package 'manifest.json'), '{"version":"launcher-test"}')
    }
    else {
        foreach ($item in Get-ChildItem -LiteralPath $PackageRoot) {
            Copy-Item -LiteralPath $item.FullName -Destination $package -Recurse -Force
        }
    }
    $launcher = Join-Path $package 'install.bat'
    $source = Join-Path $package 'bin\cap.exe'
    $sourceHash = (Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash
    $installRoot = Join-Path $tempRoot 'Programs & tools (user)!\cap'
    $pathState = Join-Path $tempRoot 'user PATH.txt'
    $originalPath = 'C:\Windows\System32;C:\Tools With Spaces;;C:\Legacy'
    [IO.File]::WriteAllText($pathState, $originalPath)
    $realUserPath = [Environment]::GetEnvironmentVariable('Path', 'User')

    # An adjacent source checkout must not override the packaged executable.
    [void][IO.Directory]::CreateDirectory((Join-Path $tempRoot 'target\release'))
    [IO.File]::WriteAllText((Join-Path $tempRoot 'target\release\cap.exe'), 'wrong adjacent binary')

    $first = Invoke-Launcher $launcher $installRoot $pathState
    Assert-Condition ($first.ExitCode -eq 0) "batch install failed: $($first.Output)"
    Assert-Condition ($first.Output.Contains('Installation complete.')) 'success instructions were missing'
    $destination = Join-Path $installRoot 'bin\cap.exe'
    Assert-Condition ((Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash -eq $sourceHash) 'batch installed the wrong executable'
    $expectedPath = $originalPath + ';' + (Join-Path $installRoot 'bin')
    Assert-Condition ([IO.File]::ReadAllText($pathState) -eq $expectedPath) 'batch changed existing PATH entries'
    $receiptPath = Join-Path $installRoot '.cap-install.json'
    $receipt = Get-Content -LiteralPath $receiptPath -Raw | ConvertFrom-Json
    Assert-Condition ($null -eq $receipt.completionActivation) 'batch enabled completions without opting in'

    $receiptBeforeDecline = [IO.File]::ReadAllText($receiptPath)
    foreach ($answer in @('n', '', 'maybe')) {
        $declined = Invoke-Launcher $launcher $installRoot $pathState -Answer $answer
        Assert-Condition ($declined.ExitCode -eq 2 -and $declined.Output.Contains('Installation cancelled.')) 'declined replacement was not cancelled'
        Assert-Condition ($declined.Output.Contains($destination) -and $declined.Output.Contains('[y/N]')) "replacement prompt omitted the path or default choice: $($declined.Output)"
        Assert-Condition (-not $declined.Output.Contains('Installation complete.') -and -not $declined.Output.Contains('Installation failed.')) 'cancellation was shown as success or failure'
        Assert-Condition ((Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash -eq $sourceHash) 'declining replaced the existing binary'
        Assert-Condition ([IO.File]::ReadAllText($receiptPath) -eq $receiptBeforeDecline) 'declining changed the receipt'
        Assert-Condition ([IO.File]::ReadAllText($pathState) -eq $expectedPath) 'declining changed PATH'
    }

    # Re-running the double-click installer uses the existing managed install.
    $second = Invoke-Launcher $launcher $installRoot $pathState
    Assert-Condition ($second.ExitCode -eq 0 -and $second.Output.Contains('cap updated at')) "batch update failed: $($second.Output)"
    Assert-Condition ([IO.File]::ReadAllText($pathState) -eq $expectedPath) 'batch update duplicated PATH'

    # Explicit consent can replace a binary whose old receipt hash no longer matches.
    [IO.File]::WriteAllText($destination, 'an independently updated cap')
    $changed = Invoke-Launcher $launcher $installRoot $pathState -Answer 'YES'
    Assert-Condition ($changed.ExitCode -eq 0 -and $changed.Output.Contains('[y/N]')) "modified binary replacement failed: $($changed.Output)"
    Assert-Condition ((Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash -eq $sourceHash) 'modified binary was not replaced'

    # An executable at the chosen destination without a receipt also gets a prompt.
    $unmanagedRoot = Join-Path $tempRoot 'unmanaged destination\cap'
    [void][IO.Directory]::CreateDirectory((Join-Path $unmanagedRoot 'bin'))
    $unmanagedBinary = Join-Path $unmanagedRoot 'bin\cap.exe'
    [IO.File]::WriteAllText($unmanagedBinary, 'previous unmanaged cap')
    $unmanagedPathState = Join-Path $tempRoot 'unmanaged PATH.txt'
    [IO.File]::WriteAllText($unmanagedPathState, $originalPath)
    $unmanagedDecline = Invoke-Launcher $launcher $unmanagedRoot $unmanagedPathState -Answer 'n'
    Assert-Condition ($unmanagedDecline.ExitCode -eq 2 -and [IO.File]::ReadAllText($unmanagedBinary) -eq 'previous unmanaged cap') 'declining an unreceipted binary changed it'
    Assert-Condition (-not (Test-Path -LiteralPath (Join-Path $unmanagedRoot '.cap-install.json'))) 'declining created a receipt'
    $unrelatedManifest = Join-Path $unmanagedRoot 'manifest.json'
    [IO.File]::WriteAllText($unrelatedManifest, 'unrelated manifest must survive')
    $metadataCollision = Invoke-Launcher $launcher $unmanagedRoot $unmanagedPathState
    Assert-Condition ($metadataCollision.ExitCode -ne 0 -and $metadataCollision.Output.Contains('unrelated install content')) 'replacement consent bypassed unrelated metadata protection'
    Assert-Condition ([IO.File]::ReadAllText($unrelatedManifest) -eq 'unrelated manifest must survive') 'replacement consent overwrote unrelated metadata'
    Assert-Condition ([IO.File]::ReadAllText($unmanagedBinary) -eq 'previous unmanaged cap') 'metadata collision changed the executable'
    Assert-Condition ([IO.File]::ReadAllText($unmanagedPathState) -eq $originalPath) 'metadata collision changed PATH'
    Remove-Item -LiteralPath $unrelatedManifest
    $unmanagedAccept = Invoke-Launcher $launcher $unmanagedRoot $unmanagedPathState
    Assert-Condition ($unmanagedAccept.ExitCode -eq 0) "unreceipted replacement failed: $($unmanagedAccept.Output)"
    Assert-Condition ((Get-FileHash -LiteralPath $unmanagedBinary -Algorithm SHA256).Hash -eq $sourceHash) 'unreceipted binary was not replaced'

    # A manually installed cap elsewhere on PATH is replaced at that exact path.
    $externalDirectory = Join-Path $tempRoot 'Existing tools & apps!'
    [void][IO.Directory]::CreateDirectory($externalDirectory)
    $externalBinary = Join-Path $externalDirectory 'cap.exe'
    [IO.File]::WriteAllText($externalBinary, 'previous PATH cap')
    $sentinel = Join-Path $externalDirectory 'manifest.json'
    [IO.File]::WriteAllText($sentinel, 'unrelated metadata')
    $fakeLocalAppData = Join-Path $tempRoot 'fake local app data'
    $externalDecline = Invoke-Launcher $launcher '' $pathState -Answer 'n' -SearchPath $externalDirectory -LocalAppData $fakeLocalAppData
    Assert-Condition ($externalDecline.ExitCode -eq 2 -and $externalDecline.Output.Contains($externalBinary)) 'PATH replacement did not ask about the resolved executable'
    Assert-Condition ([IO.File]::ReadAllText($externalBinary) -eq 'previous PATH cap') 'declining changed the PATH executable'
    $externalAccept = Invoke-Launcher $launcher '' $pathState -SearchPath $externalDirectory -LocalAppData $fakeLocalAppData
    Assert-Condition ($externalAccept.ExitCode -eq 0) "PATH replacement failed: $($externalAccept.Output)"
    Assert-Condition ((Get-FileHash -LiteralPath $externalBinary -Algorithm SHA256).Hash -eq $sourceHash) 'PATH executable was not replaced in place'
    Assert-Condition ([IO.File]::ReadAllText($sentinel) -eq 'unrelated metadata') 'PATH replacement changed unrelated metadata'
    Assert-Condition (@(Get-ChildItem -LiteralPath $externalDirectory).Count -eq 2) 'PATH replacement created extra files'
    Assert-Condition (-not (Test-Path -LiteralPath $fakeLocalAppData)) 'PATH replacement installed a competing default copy'
    Assert-Condition ([IO.File]::ReadAllText($pathState) -eq $expectedPath) 'PATH replacement rewrote PATH'

    # A managed installation found on PATH retains its receipt and PATH ownership.
    $managedElsewhere = Invoke-Launcher $launcher '' $pathState -SearchPath (Join-Path $installRoot 'bin') -LocalAppData $fakeLocalAppData
    Assert-Condition ($managedElsewhere.ExitCode -eq 0 -and $managedElsewhere.Output.Contains($destination)) "managed PATH replacement failed: $($managedElsewhere.Output)"
    $updatedReceipt = Get-Content -LiteralPath $receiptPath -Raw | ConvertFrom-Json
    Assert-Condition ($updatedReceipt.binarySha256 -ieq $sourceHash -and $updatedReceipt.pathEntry) 'managed PATH replacement lost its receipt or PATH ownership'
    Assert-Condition (-not (Test-Path -LiteralPath $fakeLocalAppData)) 'managed PATH replacement installed a competing default copy'

    # A running or locked executable still cannot be replaced after opting in.
    $locked = [IO.File]::Open($destination, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::None)
    try { $lockedResult = Invoke-Launcher $launcher $installRoot $pathState }
    finally { $locked.Dispose() }
    Assert-Condition ($lockedResult.ExitCode -ne 0 -and $lockedResult.Output.Contains('running or in use')) 'prompt mode bypassed the running-binary check'
    Assert-Condition ((Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash -eq $sourceHash) 'locked replacement changed the binary'

    $receiptBefore = [IO.File]::ReadAllText($receiptPath)
    [IO.File]::AppendAllText($source, 'corrupted')
    $failed = Invoke-Launcher $launcher $installRoot $pathState
    Assert-Condition ($failed.ExitCode -ne 0) 'batch hid a checksum error with a success exit code'
    Assert-Condition ($failed.Output.Contains('Installation failed.') -and $failed.Output.Contains('SHA-256 mismatch')) 'batch did not display the installer failure'
    Assert-Condition (-not $failed.Output.Contains('Installation complete.')) 'batch displayed success after failure'
    Assert-Condition ((Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash -eq $sourceHash) 'failed batch update changed the installed executable'
    Assert-Condition ([IO.File]::ReadAllText($receiptPath) -eq $receiptBefore) 'failed batch update changed the receipt'
    Assert-Condition ([IO.File]::ReadAllText($pathState) -eq $expectedPath) 'failed batch update changed PATH'

    $incomplete = Join-Path $tempRoot 'Unextracted launcher'
    [void][IO.Directory]::CreateDirectory($incomplete)
    Copy-Item -LiteralPath $launcher -Destination (Join-Path $incomplete 'install.bat')
    $missing = Invoke-Launcher (Join-Path $incomplete 'install.bat') $installRoot $pathState
    Assert-Condition ($missing.ExitCode -ne 0 -and $missing.Output.Contains('Extract all files')) 'incomplete extraction did not explain how to proceed'
    Assert-Condition ([Environment]::GetEnvironmentVariable('Path', 'User') -eq $realUserPath) 'launcher test changed real user PATH'

    [pscustomobject]@{ ok = $true; cases = @('install', 'confirmed-update', 'decline-and-default-no', 'unreceipted-replacement', 'modified-binary-replacement', 'standalone-path-replacement', 'managed-path-replacement', 'running-binary', 'metadata-protection', 'spaces-and-shell-characters', 'unrelated-working-directory', 'packaged-source', 'path-preservation', 'checksum-failure', 'failure-exit-code', 'incomplete-extraction') } | ConvertTo-Json
}
finally {
    $resolvedRoot = [IO.Path]::GetFullPath($tempRoot)
    if (-not $resolvedRoot.StartsWith($tempBase + '\', [StringComparison]::OrdinalIgnoreCase) -or
        -not (Test-Path -LiteralPath $marker -PathType Leaf)) {
        throw "Refusing to remove an unowned test directory: $resolvedRoot"
    }
    Remove-Item -LiteralPath $resolvedRoot -Recurse -Force
}
