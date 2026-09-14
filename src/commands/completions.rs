//! Completion script emission.  Nothing is installed or written to a shell
//! profile by this command.

use crate::app::{AppError, CommandOutput};
use crate::cli::GlobalOptions;
use serde_json::json;

/// Emit an opt-in, profile-free PowerShell native completer.  The metadata
/// branch is guarded by `CAP_COMPLETIONS_METADATA=1` and invokes only the
/// read-only `cap --json tags` command.
pub const POWERSHELL_COMPLETION: &str = r#"# cap PowerShell completions (generated; pipe to a file and review before opting in)
$capCommands = @('add','write','show','today','recent','search','tags','moods','context','doctor','status','recover','enrich','theme','fx','config','completions','recall','on-this-day','calendar','stats','garden')
$capGlobalOptions = @('--db','--json','--quiet','--plain','--color','--motion','--theme','--offline','--no-context','--dry-run','--help','--version')
$capOptionValues = @('--db','--color','--motion','--theme')
$capOptionCandidates = @{
    '--theme' = @('aurora','neon','c64','amber','paper')
    '--color' = @('auto','always','never')
    '--motion' = @('auto','full','reduced','off')
}

function ConvertTo-CapCompletionText([string]$value) {
    # CompletionText is a literal token, never an expression.  Single-quote
    # values containing shell punctuation and escape embedded single quotes
    # using PowerShell's doubled-quote rule.
    if ($value -match '[\s"'';&|<>`$()]') {
        return "'" + $value.Replace("'", "''") + "'"
    }
    return $value
}

function ConvertFrom-CapAstToken([string]$value) {
    if ($value.Length -ge 2) {
        if (($value.StartsWith("'") -and $value.EndsWith("'")) -or ($value.StartsWith('"') -and $value.EndsWith('"'))) {
            return $value.Substring(1, $value.Length - 2).Replace("''", "'")
        }
    }
    return $value
}

function Get-CapCommandTokens([array]$tokens) {
    $commands = @()
    $literal = $false
    for ($i = 0; $i -lt $tokens.Count; $i++) {
        $token = ConvertFrom-CapAstToken ([string]$tokens[$i])
        if ($literal) { break }
        if ($token -eq '--') { $literal = $true; break }
        if ($token.StartsWith('-')) {
            # Skip the value belonging to a known option, including a quoted
            # --db path with spaces.  Unknown options have no positional
            # value in cap's grammar and are simply ignored here.
            if ($capOptionValues -contains $token) { $i++ }
            continue
        }
        $commands += $token
    }
    return $commands
}

Register-ArgumentCompleter -Native -CommandName cap -ScriptBlock {
    param($wordToComplete, $commandAst, $cursorPosition)

    $elements = @($commandAst.CommandElements | ForEach-Object { $_.ToString() })
    # The AST includes the token currently being completed.  Remove exactly
    # that trailing token so `cap th`, `cap theme p` and `cap theme preview a`
    # all resolve their parent command/subcommand correctly.
    $completed = @($elements | Select-Object -Skip 1)
    if ($completed.Count -gt 0 -and [string]::Equals([string]$completed[-1], [string]$wordToComplete, [System.StringComparison]::Ordinal)) {
        if ($completed.Count -eq 1) { $completed = @() } else { $completed = @($completed[0..($completed.Count - 2)]) }
    }
    $commandTokens = @(Get-CapCommandTokens $completed)
    $firstCommand = if ($commandTokens.Count -gt 0) { [string]$commandTokens[0] } else { $null }
    $subcommand = if ($commandTokens.Count -gt 1) { [string]$commandTokens[1] } else { $null }
    $previousToken = if ($completed.Count -gt 0) { ConvertFrom-CapAstToken ([string]$completed[-1]) } else { $null }
    $optionContext = if ($capOptionValues -contains $previousToken) { $previousToken } else { $null }
    $candidates = @()

    if ($optionContext -and $capOptionCandidates.ContainsKey($optionContext)) {
        $candidates = $capOptionCandidates[$optionContext]
    } elseif ($wordToComplete.StartsWith('-')) {
        $candidates = $capGlobalOptions
    } elseif (-not $firstCommand) {
        $candidates = $capCommands
    } else {
        switch ($firstCommand) {
            'theme' {
                if ($subcommand -eq 'preview' -or $subcommand -eq 'set') {
                    $candidates = @('aurora','neon','c64','amber','paper')
                } elseif (-not $subcommand) {
                    $candidates = @('list','preview','set')
                }
            }
            'config' {
                if (-not $subcommand) {
                    $candidates = @('show','set')
                } elseif ($subcommand -eq 'set') {
                    $candidates = @('theme','color','motion','icon_mode','preview_visibility','writer_display','writer_target_override','editor_executable','editor_args')
                }
            }
            'recover' { if (-not $subcommand) { $candidates = @('list','show','retry','discard') } }
            'completions' { if (-not $subcommand) { $candidates = @('powershell') } }
            'fx' { if (-not $subcommand) { $candidates = @('all','neon','ocean','sunset','fire','aurora','ice','candy','mono','text','line','vertical','diagonal','rainbow') } }
        }
    }

    foreach ($candidate in $candidates) {
        if ($candidate.StartsWith($wordToComplete, [System.StringComparison]::OrdinalIgnoreCase)) {
            [System.Management.Automation.CompletionResult]::new($candidate, $candidate, 'ParameterValue', $candidate)
        }
    }

    # Metadata completion is opt-in per session and read-only.  It supports
    # `cap add --tag <prefix>` and `cap add --mood <prefix>`.  Explicit --db is
    # forwarded as an argument vector, never reparsed as a command string.
    # The effective read-only invocation is `cap --json tags` or
    # `cap --json moods` (with the selected --db forwarded below).
    $metadataKind = if ($previousToken -eq '--tag' -or $previousToken -eq '--mood') { $previousToken.Substring(2) } else { $null }
    $literalMode = $completed | ForEach-Object { ConvertFrom-CapAstToken ([string]$_) } | Where-Object { $_ -eq '--' }
    if (($env:CAP_COMPLETIONS_METADATA -eq '1') -and $metadataKind -and (-not $literalMode) -and ($wordToComplete -notlike '-*')) {
        try {
            $dbArgs = @()
            for ($i = 0; $i -lt $completed.Count - 1; $i++) {
                $token = ConvertFrom-CapAstToken ([string]$completed[$i])
                if ($token -eq '--db') { $dbArgs += @('--db', (ConvertFrom-CapAstToken ([string]$completed[$i + 1]))) }
            }
            $metadataCommand = if ($metadataKind -eq 'tag') { 'tags' } else { 'moods' }
            $metadataArgs = @('--json') + $dbArgs + @($metadataCommand, '--limit', '200')
            $metadata = & cap @metadataArgs 2>$null | ConvertFrom-Json
            foreach ($item in @($metadata.data.items)) {
                $name = if ($item.name) { [string]$item.name } elseif ($item.mood) { [string]$item.mood } else { '' }
                if ($name -and $name.StartsWith($wordToComplete, [System.StringComparison]::OrdinalIgnoreCase)) {
                    $completionText = ConvertTo-CapCompletionText $name
                    [System.Management.Automation.CompletionResult]::new($completionText, $name, 'ParameterValue', "Capsule $metadataKind")
                }
            }
        } catch {
            # A transient read/config failure must not fail the user's command.
        }
    }
}
"#;

pub fn run(shell: &str, _global: &GlobalOptions) -> Result<CommandOutput, AppError> {
    if !shell.trim().eq_ignore_ascii_case("powershell") {
        return Err(AppError::new(
            "INVALID_SHELL",
            "Only PowerShell completions are currently supported.",
            2,
        ));
    }
    let mut output = CommandOutput::new(
        json!({"shell": "powershell", "installed": false, "script": POWERSHELL_COMPLETION}),
        POWERSHELL_COMPLETION,
    );
    output.quiet = Some(String::new());
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_is_profile_free_and_metadata_is_explicitly_opt_in() {
        let output = run("powershell", &GlobalOptions::default()).unwrap();
        let script = output.data["script"].as_str().unwrap();
        assert!(script.contains("Register-ArgumentCompleter"));
        assert!(script.contains("CAP_COMPLETIONS_METADATA"));
        assert!(script.contains("--json tags"));
        assert!(!script.contains("Invoke-Expression"));
        assert!(!script.contains("$PROFILE"));
        assert!(!script.contains("Set-Content"));
        assert!(!script.contains('\x1b'));
    }

    #[test]
    fn only_powershell_is_emitted() {
        let error = run("bash", &GlobalOptions::default()).unwrap_err();
        assert_eq!(error.exit_code, 2);
    }

    #[cfg(windows)]
    #[test]
    fn emitted_script_completes_commands_and_theme_values_in_powershell() {
        use std::process::Command;
        use tempfile::tempdir;

        let directory = tempdir().expect("temporary completion directory");
        let script_path = directory.path().join("cap-completions.ps1");
        let checks = r#"
$first = [System.Management.Automation.CommandCompletion]::CompleteInput('cap th', 6, $null)
if (-not (@($first.CompletionMatches | Where-Object { $_.CompletionText -eq 'theme' }).Count -gt 0)) { exit 41 }
$second = [System.Management.Automation.CommandCompletion]::CompleteInput('cap theme p', 11, $null)
if (-not (@($second.CompletionMatches | Where-Object { $_.CompletionText -eq 'preview' }).Count -gt 0)) { exit 42 }
$third = [System.Management.Automation.CommandCompletion]::CompleteInput('cap theme preview a', 19, $null)
if (-not (@($third.CompletionMatches | Where-Object { $_.CompletionText -eq 'aurora' }).Count -gt 0)) { exit 43 }
$withDb = "cap --db 'C:/lab db.db' theme p"
$fourth = [System.Management.Automation.CommandCompletion]::CompleteInput($withDb, $withDb.Length, $null)
if (-not (@($fourth.CompletionMatches | Where-Object { $_.CompletionText -eq 'preview' }).Count -gt 0)) { exit 44 }
$themeOption = [System.Management.Automation.CommandCompletion]::CompleteInput('cap --theme a', 13, $null)
if (-not (@($themeOption.CompletionMatches | Where-Object { $_.CompletionText -eq 'aurora' }).Count -gt 0)) { exit 45 }
"#;
        std::fs::write(
            &script_path,
            format!("{}\n{}\n", POWERSHELL_COMPLETION, checks),
        )
        .expect("write completion script");
        let output = Command::new("pwsh")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ])
            .arg(&script_path)
            .output()
            .expect("PowerShell runtime");
        assert!(
            output.status.success(),
            "PowerShell completion check failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[cfg(windows)]
    #[test]
    fn metadata_completion_is_opt_in_and_quotes_shell_punctuation() {
        use std::process::Command;
        use tempfile::tempdir;

        let directory = tempdir().expect("temporary completion directory");
        let script_path = directory.path().join("cap-metadata-completions.ps1");
        let fake_cap = directory.path().join("cap.cmd");
        // The fake executable is read-only test data.  Its response contains
        // spaces, a semicolon and ampersand so CompletionText quoting is
        // exercised without touching a real database.
        std::fs::write(
            &fake_cap,
            "@echo {\"schemaVersion\":1,\"ok\":true,\"data\":{\"items\":[{\"name\":\"red tag; &\"}]}}\r\n",
        )
        .expect("fake cap command");
        let checks = r#"
$env:CAP_COMPLETIONS_METADATA = '1'
$result = [System.Management.Automation.CommandCompletion]::CompleteInput('cap add --tag r', 15, $null)
$match = @($result.CompletionMatches | Where-Object { $_.ListItemText -eq 'red tag; &' })
if ($match.Count -ne 1) { exit 51 }
if ($match[0].CompletionText -ne "'red tag; &'") { exit 52 }
"#;
        std::fs::write(
            &script_path,
            format!("{}\n{}\n", POWERSHELL_COMPLETION, checks),
        )
        .expect("write metadata completion script");
        let old_path = std::env::var_os("PATH").unwrap_or_default();
        let path = format!(
            "{};{}",
            directory.path().to_string_lossy(),
            old_path.to_string_lossy()
        );
        let output = Command::new("pwsh")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ])
            .arg(&script_path)
            .env("PATH", path)
            .output()
            .expect("PowerShell runtime");
        assert!(
            output.status.success(),
            "PowerShell metadata completion check failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
