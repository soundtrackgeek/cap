use cap::{
    app,
    cli::{Cli, Command},
    contracts::{CliError, OutputEnvelope},
    output,
};
use clap::{CommandFactory, FromArgMatches};
use std::io::{self, Write};

fn main() {
    std::process::exit(run());
}

fn run() -> i32 {
    let arguments = std::env::args_os().collect::<Vec<_>>();
    let json_requested = arguments
        .iter()
        .skip(1)
        .take_while(|v| *v != "--")
        .any(|v| v == "--json");
    let matches = match Cli::command()
        .color(clap::ColorChoice::Never)
        .try_get_matches_from(arguments)
    {
        Ok(matches) => matches,
        Err(error) => return print_parse_result(error, json_requested),
    };
    let cli = match Cli::from_arg_matches(&matches) {
        Ok(cli) => cli,
        Err(error) => return print_parse_result(error, json_requested),
    };
    let command = cli.command.as_ref().map(Command::name).unwrap_or("write");
    match app::execute(&cli) {
        Ok(result) => {
            let mut envelope = OutputEnvelope::success(command, result.data);
            envelope.warnings = result.warnings;
            if cli.global.json {
                if output::write_json(&mut io::stdout().lock(), &envelope).is_err() {
                    return if result.committed { 0 } else { 1 };
                }
            } else {
                let text = if cli.global.quiet {
                    result.quiet.unwrap_or_default()
                } else {
                    result.human
                };
                if !text.is_empty() && writeln!(io::stdout().lock(), "{text}").is_err() {
                    return if result.committed { 0 } else { 1 };
                }
                for warning in envelope.warnings {
                    let _ = writeln!(io::stderr().lock(), "Warning: {warning}");
                }
            }
            0
        }
        Err(error) => {
            if cli.global.json {
                let mut envelope = OutputEnvelope::failure(command, error.detail);
                envelope.data = error.data;
                let _ = output::write_json(&mut io::stdout().lock(), &envelope);
            } else {
                let _ = writeln!(io::stderr().lock(), "{}", error.detail.message);
            }
            error.exit_code
        }
    }
}

fn print_parse_result(error: clap::Error, json: bool) -> i32 {
    let help = matches!(
        error.kind(),
        clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
    );
    let command = if error.kind() == clap::error::ErrorKind::DisplayVersion {
        "version"
    } else if help {
        "help"
    } else {
        "parse"
    };
    if json {
        let envelope = if help {
            OutputEnvelope::success(command, serde_json::json!({ "text": error.to_string() }))
        } else {
            OutputEnvelope::failure(
                command,
                CliError::new("INVALID_INPUT", error.to_string(), false),
            )
        };
        let _ = output::write_json(&mut io::stdout().lock(), &envelope);
    } else {
        let _ = error.print();
    }
    if help {
        0
    } else {
        2
    }
}
