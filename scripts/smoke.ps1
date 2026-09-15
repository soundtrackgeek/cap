[CmdletBinding()]
param(
    [string]$CapPath,
    [string]$FixtureGeneratorPath,
    [string]$InstallRoot,
    [switch]$RunInstallLifecycle,
    [switch]$KeepLab,
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

function Get-PowerShellPath {
    $pwsh = Get-Command pwsh -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($null -ne $pwsh) { return (ConvertTo-AbsolutePath $pwsh.Source -MustExist) }
    $powershell = Get-Command powershell -ErrorAction Stop | Select-Object -First 1
    return (ConvertTo-AbsolutePath $powershell.Source -MustExist)
}

function New-IsolationEnvironment {
    param(
        [Parameter(Mandatory = $true)] [hashtable]$Values,
        [Parameter(Mandatory = $true)] [string]$BinaryDirectory
    )

    $environment = @{}
    foreach ($key in $Values.Keys) { $environment[[string]$key] = [string]$Values[$key] }
    $machinePath = [Environment]::GetEnvironmentVariable('Path', 'Machine')
    $environment['PATH'] = if ([string]::IsNullOrWhiteSpace($machinePath)) { $BinaryDirectory } else { "$BinaryDirectory;$machinePath" }
    return $environment
}

function Invoke-FreshPowerShell {
    param(
        [Parameter(Mandatory = $true)] [string]$ScriptText,
        [Parameter(Mandatory = $true)] [string]$WorkingDirectory,
        [Parameter(Mandatory = $true)] [hashtable]$Environment
    )

    $shell = Get-PowerShellPath
    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = $shell
    $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($ScriptText))
    $start.Arguments = "-NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -EncodedCommand $encoded"
    $start.WorkingDirectory = $WorkingDirectory
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true

    $systemEnvironment = @{}
    foreach ($name in @('SYSTEMROOT', 'WINDIR', 'PATHEXT')) {
        $value = [Environment]::GetEnvironmentVariable($name, 'Process')
        if (-not [string]::IsNullOrWhiteSpace($value)) { $systemEnvironment[$name] = [string]$value }
    }
    $start.EnvironmentVariables.Clear()
    foreach ($entry in $systemEnvironment.GetEnumerator()) {
        $start.EnvironmentVariables[[string]$entry.Key] = [string]$entry.Value
    }
    foreach ($entry in $Environment.GetEnumerator()) {
        $start.EnvironmentVariables[[string]$entry.Key] = [string]$entry.Value
    }

    $process = [Diagnostics.Process]::new()
    try {
        $process.StartInfo = $start
        if (-not $process.Start()) { throw "Unable to start fresh PowerShell: $shell" }
        $stdoutTask = $process.StandardOutput.ReadToEndAsync()
        $stderrTask = $process.StandardError.ReadToEndAsync()
        $timeoutMilliseconds = 120000
        if (-not $process.WaitForExit($timeoutMilliseconds)) {
            try { $process.Kill() } catch { }
            try { [void]$process.WaitForExit(5000) } catch { }
            $stdout = if ($stdoutTask.IsCompleted) { $stdoutTask.Result } else { '<stdout unavailable after timeout>' }
            $stderr = if ($stderrTask.IsCompleted) { $stderrTask.Result } else { '<stderr unavailable after timeout>' }
            throw "Fresh PowerShell timed out after $timeoutMilliseconds ms.`nSTDOUT:`n$stdout`nSTDERR:`n$stderr"
        }
        $process.WaitForExit()
        $stdout = $stdoutTask.Result
        $stderr = $stderrTask.Result
        return [pscustomobject]@{ ExitCode = $process.ExitCode; Stdout = $stdout; Stderr = $stderr }
    }
    finally {
        $process.Dispose()
    }
}

function Invoke-IsolatedCap {
    param(
        [Parameter(Mandatory = $true)] [string[]]$Arguments,
        [Parameter(Mandatory = $true)] [string]$WorkingDirectory,
        [Parameter(Mandatory = $true)] [hashtable]$Environment
    )

    $payload = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes(($Arguments | ConvertTo-Json -Compress)))
    $Environment['CAP_SMOKE_ARGS_B64'] = $payload
    $script = @'
$arguments = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($env:CAP_SMOKE_ARGS_B64)) | ConvertFrom-Json
$command = Get-Command cap.exe -ErrorAction Stop
& $command.Source @arguments
exit $LASTEXITCODE
'@
    return Invoke-FreshPowerShell -ScriptText $script -WorkingDirectory $WorkingDirectory -Environment $Environment
}

function Assert-SmokeResult {
    param(
        [Parameter(Mandatory = $true)] $Result,
        [Parameter(Mandatory = $true)] [string]$Label
    )
    if ($Result.ExitCode -ne 0) {
        throw "$Label failed with exit code $($Result.ExitCode).`nSTDOUT:`n$($Result.Stdout)`nSTDERR:`n$($Result.Stderr)"
    }
}

function Get-EntryIdentifier {
    param([Parameter(Mandatory = $true)] $Value)

    if ($null -eq $Value) { return $null }
    if ($Value -is [System.Collections.IDictionary]) {
        foreach ($key in @('uuid', 'entryUuid', 'entryUUID', 'captureId', 'captureID', 'entryId', 'entry_id')) {
            if ($Value.Contains($key) -and -not [string]::IsNullOrWhiteSpace([string]$Value[$key])) { return [string]$Value[$key] }
        }
        foreach ($item in $Value.Values) {
            $found = Get-EntryIdentifier $item
            if (-not [string]::IsNullOrWhiteSpace($found)) { return $found }
        }
    }
    elseif ($Value -is [System.Collections.IEnumerable] -and $Value -isnot [string]) {
        foreach ($item in $Value) {
            $found = Get-EntryIdentifier $item
            if (-not [string]::IsNullOrWhiteSpace($found)) { return $found }
        }
    }
    else {
        foreach ($property in @('uuid', 'entryUuid', 'entryUUID', 'captureId', 'captureID', 'entryId', 'entry_id')) {
            $candidate = $Value.PSObject.Properties[$property]
            if ($null -ne $candidate -and -not [string]::IsNullOrWhiteSpace([string]$candidate.Value)) { return [string]$candidate.Value }
        }
        foreach ($candidate in $Value.PSObject.Properties) {
            $found = Get-EntryIdentifier $candidate.Value
            if (-not [string]::IsNullOrWhiteSpace($found)) { return $found }
        }
    }
    return $null
}

function Remove-OwnedTree {
    param(
        [Parameter(Mandatory = $true)] [string]$Path,
        [Parameter(Mandatory = $true)] [string]$TempRoot,
        [Parameter(Mandatory = $true)] [string]$Marker
    )
    $pathKey = Get-PathKey $Path
    $tempKey = Get-PathKey $TempRoot
    if (-not $pathKey.StartsWith($tempKey + [char]92, [StringComparison]::OrdinalIgnoreCase)) { throw "Refusing to remove an unowned smoke path: $pathKey" }
    if (-not (Test-Path -LiteralPath (Join-Path $pathKey $Marker) -PathType Leaf)) { throw "Refusing to remove an unmarked smoke path: $pathKey" }
    Remove-Item -LiteralPath $pathKey -Recurse -Force
}

$scriptRoot = Get-PathKey $PSScriptRoot
$repoRoot = Get-PathKey ([IO.Path]::GetDirectoryName([IO.Path]::GetFullPath($scriptRoot)))
if ([string]::IsNullOrWhiteSpace($CapPath)) {
    $CapPath = Join-Path $repoRoot 'target\release\cap.exe'
}
$cap = ConvertTo-AbsolutePath $CapPath -MustExist
if ([string]::IsNullOrWhiteSpace($FixtureGeneratorPath)) {
    foreach ($candidate in @(
        (Join-Path $repoRoot 'target\release\examples\fixture_lab.exe'),
        (Join-Path $repoRoot 'target\debug\examples\fixture_lab.exe')
    )) {
        if (Test-Path -LiteralPath $candidate -PathType Leaf) { $FixtureGeneratorPath = $candidate; break }
    }
}
$generator = ConvertTo-AbsolutePath $FixtureGeneratorPath -MustExist

$tempRoot = Get-PathKey ([IO.Path]::GetTempPath())
$runRoot = Get-PathKey (Join-Path $tempRoot ("cap-smoke-" + [guid]::NewGuid().ToString('N')))
$unrelated = Join-Path $runRoot 'unrelated working directory'
[IO.Directory]::CreateDirectory($unrelated) | Out-Null
[IO.File]::WriteAllText((Join-Path $runRoot '.cap-smoke-owned'), '')
$labRoot = $null
$installWasCreated = $false
try {
    $generatorEnvironment = New-IsolationEnvironment -Values @{
        TEMP = $runRoot
        TMP = $runRoot
        TMPDIR = $runRoot
        USERPROFILE = $runRoot
        HOME = $runRoot
        LOCALAPPDATA = (Join-Path $runRoot 'localappdata')
    } -BinaryDirectory ([IO.Path]::GetDirectoryName([IO.Path]::GetFullPath($generator)))
    [IO.Directory]::CreateDirectory((Join-Path $runRoot 'localappdata')) | Out-Null
    $labResult = Invoke-FreshPowerShell -ScriptText ("& '" + $generator.Replace("'", "''") + "' 5; exit `$LASTEXITCODE") -WorkingDirectory $unrelated -Environment $generatorEnvironment
    Assert-SmokeResult -Result $labResult -Label 'fixture_lab generator'
    $lab = $labResult.Stdout.Trim() | ConvertFrom-Json
    if ($lab.synthetic -ne $true) { throw 'fixture_lab did not report a synthetic lab.' }
    $labRoot = Get-PathKey ([string]$lab.root)
    if (-not $labRoot.StartsWith($tempRoot + [char]92, [StringComparison]::OrdinalIgnoreCase) -or -not (Test-Path -LiteralPath (Join-Path $labRoot 'lab.json') -PathType Leaf)) {
        throw "fixture_lab returned an unowned root: $labRoot"
    }

    $capForSmoke = $cap
    $pathState = Join-Path $runRoot 'user PATH.txt'
    [IO.File]::WriteAllText($pathState, "C:\\Windows\\System32;C:\\Tools With Spaces")
    if ($RunInstallLifecycle) {
        if ([string]::IsNullOrWhiteSpace($InstallRoot)) { $InstallRoot = Join-Path $runRoot 'Programs with spaces\cap' }
        $installAbsolute = Get-PathKey $InstallRoot
        if (-not $installAbsolute.StartsWith($runRoot + [char]92, [StringComparison]::OrdinalIgnoreCase)) {
            throw "RunInstallLifecycle requires an install root below the owned smoke directory: $installAbsolute"
        }
        $installResult = & (Join-Path $scriptRoot 'install.ps1') -SourcePath $cap -InstallRoot $installAbsolute -PathStatePath $pathState -PassThru | ConvertFrom-Json
        if (-not $installResult.ok) { throw 'The temporary install lifecycle did not report success.' }
        $installWasCreated = $true
        $capForSmoke = Get-PathKey (Join-Path $installAbsolute 'bin\cap.exe')
    }

    $binaryDirectory = [IO.Path]::GetDirectoryName([IO.Path]::GetFullPath($capForSmoke))
    $environment = @{}
    foreach ($property in $lab.environment.PSObject.Properties) { $environment[$property.Name] = [string]$property.Value }
    $environment = New-IsolationEnvironment -Values $environment -BinaryDirectory $binaryDirectory
    $help = Invoke-IsolatedCap -Arguments @('--help') -WorkingDirectory $unrelated -Environment $environment
    Assert-SmokeResult -Result $help -Label 'installed cap --help'
    if ($help.Stdout -notmatch '(?m)cap') { throw 'cap --help did not produce command help.' }

    $unknown = Invoke-IsolatedCap -Arguments @('test') -WorkingDirectory $unrelated -Environment $environment
    if ($unknown.ExitCode -ne 2 -or $unknown.Stdout -ne '' -or
        $unknown.Stderr -notmatch 'Command not recognized' -or -not $unknown.Stderr.EndsWith($help.Stdout)) {
        throw "cap test must fail with the full help instead of capturing text: $($unknown.Stderr)"
    }

    $doctor = Invoke-IsolatedCap -Arguments @('--json', 'doctor') -WorkingDirectory $unrelated -Environment $environment
    Assert-SmokeResult -Result $doctor -Label 'installed cap --json doctor'
    $doctorJson = $doctor.Stdout.Trim() | ConvertFrom-Json
    if ($doctorJson.ok -ne $true -or $doctorJson.schemaVersion -ne 1) { throw 'cap --json doctor returned an invalid envelope.' }

    $db = [string]$lab.environment.CAPSULE_DB_PATH
    $add = Invoke-IsolatedCap -Arguments @('--json', '--db', $db, '--no-context', 'add', '--', 'cap Windows delivery smoke entry') -WorkingDirectory $unrelated -Environment $environment
    Assert-SmokeResult -Result $add -Label 'installed cap add'
    $addJson = $add.Stdout.Trim() | ConvertFrom-Json
    if ($addJson.ok -ne $true) { throw 'cap add did not return a successful envelope.' }
    $entryId = Get-EntryIdentifier $addJson
    if ([string]::IsNullOrWhiteSpace($entryId)) { throw "cap add returned no durable entry identifier: $($add.Stdout)" }

    $show = Invoke-IsolatedCap -Arguments @('--json', '--db', $db, 'show', $entryId) -WorkingDirectory $unrelated -Environment $environment
    Assert-SmokeResult -Result $show -Label 'installed cap show'
    $showJson = $show.Stdout.Trim() | ConvertFrom-Json
    if ($showJson.ok -ne $true -or $show.Stdout -notmatch 'cap Windows delivery smoke entry') { throw 'cap show did not return the smoke entry.' }

    $result = [pscustomobject]@{
        ok = $true
        capPath = $capForSmoke
        fixtureRoot = $labRoot
        unrelatedWorkingDirectory = (Get-PathKey $unrelated)
        entryId = $entryId
        doctorSchemaVersion = $doctorJson.schemaVersion
        lifecycle = $RunInstallLifecycle.IsPresent
    }
    if ($PassThru) { $result | ConvertTo-Json -Depth 8 } else {
        Write-Output "Smoke passed from unrelated directory: $unrelated"
        Write-Output "Synthetic entry: $entryId"
    }
}
finally {
    if ($installWasCreated) {
        try { & (Join-Path $scriptRoot 'uninstall.ps1') -InstallRoot (Get-PathKey $InstallRoot) -PathStatePath (Get-PathKey (Join-Path $runRoot 'user PATH.txt')) | Out-Null } catch { Write-Warning "Temporary install cleanup failed: $($_.Exception.Message)" }
    }
    if (-not $KeepLab) {
        if ($null -ne $labRoot -and (Test-Path -LiteralPath $labRoot -PathType Container)) {
            Remove-OwnedTree -Path $labRoot -TempRoot $tempRoot -Marker 'lab.json'
        }
        if (Test-Path -LiteralPath $runRoot -PathType Container) {
            Remove-OwnedTree -Path $runRoot -TempRoot $tempRoot -Marker '.cap-smoke-owned'
        }
    }
    else {
        Write-Warning "Keeping synthetic fixture and launch directory at $runRoot"
    }
}
