use clap::Parser;

#[derive(Parser)]
#[command(version, about = "Capsule CLI — implementation in progress")]
struct Bootstrap {
    #[arg(long)]
    json: bool,
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    args: Vec<String>,
}

fn main() {
    let args = Bootstrap::parse();
    let error = cap::contracts::CliError::new(
        "NOT_IMPLEMENTED",
        "Capture is not connected yet. This is the foundation development build.",
        false,
    );
    if args.json {
        let envelope =
            cap::contracts::OutputEnvelope::<serde_json::Value>::failure("bootstrap", error);
        if let Err(error) = cap::output::write_json(&mut std::io::stdout().lock(), &envelope) {
            eprintln!("Unable to write result: {error}");
        }
    } else {
        eprintln!("{}", error.message);
    }
    std::process::exit(3);
}
