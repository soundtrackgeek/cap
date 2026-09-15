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
    param([string]$Launcher, [string]$InstallRoot, [string]$PathState)

    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = Join-Path $env:SystemRoot 'System32\cmd.exe'
    $start.Arguments = '/d /s /c ""{0}" -InstallRoot "{1}" -PathStatePath "{2}""' -f $Launcher, $InstallRoot, $PathState
    $start.WorkingDirectory = $tempRoot
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardInput = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    # Exclude any real installed cap from collision detection.
    $start.EnvironmentVariables['PATH'] = Join-Path $env:SystemRoot 'System32'
    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $start
    try {
        [void]$process.Start()
        $stdout = $process.StandardOutput.ReadToEndAsync()
        $stderr = $process.StandardError.ReadToEndAsync()
        $process.StandardInput.WriteLine(' ')
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

    # Re-running the double-click installer uses the existing managed install.
    $second = Invoke-Launcher $launcher $installRoot $pathState
    Assert-Condition ($second.ExitCode -eq 0 -and $second.Output.Contains('cap updated at')) "batch update failed: $($second.Output)"
    Assert-Condition ([IO.File]::ReadAllText($pathState) -eq $expectedPath) 'batch update duplicated PATH'

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

    [pscustomobject]@{ ok = $true; cases = @('install', 'update', 'spaces-and-shell-characters', 'unrelated-working-directory', 'packaged-source', 'path-preservation', 'checksum-failure', 'failure-exit-code', 'incomplete-extraction') } | ConvertTo-Json
}
finally {
    $resolvedRoot = [IO.Path]::GetFullPath($tempRoot)
    if (-not $resolvedRoot.StartsWith($tempBase + '\', [StringComparison]::OrdinalIgnoreCase) -or
        -not (Test-Path -LiteralPath $marker -PathType Leaf)) {
        throw "Refusing to remove an unowned test directory: $resolvedRoot"
    }
    Remove-Item -LiteralPath $resolvedRoot -Recurse -Force
}
