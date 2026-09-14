use clap::{Args, Parser, Subcommand, ValueEnum};
use std::{ffi::OsString, path::PathBuf};

#[derive(Debug, Parser)]
#[command(
    name = "cap",
    version,
    long_version = env!("CAP_LONG_VERSION"),
    about = "A little ceremony for everyday Capsule memories",
    disable_help_subcommand = true,
    after_help = "Write an entry: cap Had a lovely walk\nLiteral command names: cap add -- today was wonderful\nFile capture: cap add --file today.md\nUse --json before a command for machine-readable output."
)]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalOptions,
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Default, Args)]
pub struct GlobalOptions {
    #[arg(long, global = true)]
    pub db: Option<PathBuf>,
    #[arg(long, global = true)]
    pub json: bool,
    #[arg(long, global = true)]
    pub quiet: bool,
    #[arg(long, global = true)]
    pub plain: bool,
    #[arg(long, global = true, value_enum)]
    pub color: Option<ColorMode>,
    #[arg(long, global = true, value_enum)]
    pub motion: Option<MotionMode>,
    #[arg(long, global = true)]
    pub theme: Option<String>,
    #[arg(long, global = true)]
    pub offline: bool,
    #[arg(long, global = true)]
    pub no_context: bool,
    #[arg(long, global = true)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub enum ColorMode {
    Auto,
    Always,
    Never,
}
#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub enum MotionMode {
    Auto,
    Full,
    Reduced,
    Off,
}
#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub enum ContentFormat {
    Markdown,
    Plain,
}
#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub enum Period {
    Week,
    Month,
    Year,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Save a journal entry.
    Add(AddArgs),
    /// Open the draft-backed writer.
    Write(WriteArgs),
    /// Show one entry by stable UUID or current entry number.
    Show(ShowArgs),
    /// Read today's visible entries.
    Today(ReadArgs),
    /// Read recent entries, newest first.
    Recent(ReadArgs),
    /// Search using Capsule's keyword and structured syntax.
    Search(SearchArgs),
    /// Discover existing tags.
    Tags(Pagination),
    /// Discover existing moods.
    Moods(Pagination),
    /// Inspect effective location/weather settings without network calls.
    Context,
    /// Inspect paths, schema and terminal capabilities read-only.
    Doctor,
    /// Reconcile a capture's saved/pending state.
    Status {
        #[arg(long)]
        capture_id: String,
    },
    /// Inspect or explicitly retry local pending captures.
    Recover {
        #[command(subcommand)]
        action: RecoverAction,
    },
    /// Retry missing location/weather for an existing entry.
    Enrich { identifier: String },
    /// Preview and choose a terminal theme.
    Theme {
        #[command(subcommand)]
        action: ThemeAction,
    },
    /// Play a short synthetic effects demo without opening the journal.
    Fx { name: Option<String> },
    /// Inspect or change cap-only preferences.
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Emit a PowerShell completion script without installing it.
    Completions {
        #[arg(value_parser = ["powershell"])]
        shell: String,
    },
    /// Revisit one random visible memory.
    Recall {
        #[arg(long)]
        tag: Option<String>,
        #[arg(long)]
        include_hidden: bool,
    },
    /// Revisit this date in earlier years.
    OnThisDay(ReadArgs),
    /// Show a writing activity calendar.
    Calendar {
        #[arg(long)]
        month: Option<String>,
        #[arg(long)]
        include_hidden: bool,
    },
    /// Summarize writing activity.
    Stats {
        #[arg(long, value_enum, default_value = "month")]
        period: Period,
        #[arg(long)]
        include_hidden: bool,
    },
    /// Grow a seven-day garden from writing activity.
    Garden {
        #[arg(long)]
        include_hidden: bool,
    },
    #[command(external_subcommand)]
    Entry(Vec<OsString>),
}

#[derive(Debug, Default, Args)]
pub struct AddArgs {
    #[arg(long, conflicts_with_all = ["stdin", "text"])]
    pub file: Option<PathBuf>,
    #[arg(long, conflicts_with = "text")]
    pub stdin: bool,
    #[arg(long)]
    pub mood: Option<String>,
    #[arg(long = "tag")]
    pub tags: Vec<String>,
    #[arg(long)]
    pub title: Option<String>,
    #[arg(long)]
    pub summary: Option<String>,
    #[arg(long)]
    pub star: bool,
    #[arg(long)]
    pub pin: bool,
    #[arg(long, value_enum, default_value = "markdown")]
    pub format: Option<ContentFormat>,
    #[arg(long = "continue")]
    pub continue_from: Option<String>,
    #[arg(long)]
    pub capture_id: Option<String>,
    #[arg(trailing_var_arg = true)]
    pub text: Vec<String>,
}

#[derive(Debug, Args)]
pub struct WriteArgs {
    #[arg(long)]
    pub editor: bool,
}
#[derive(Debug, Args)]
pub struct ShowArgs {
    pub identifier: String,
    #[arg(long)]
    pub include_hidden: bool,
}
#[derive(Debug, Args)]
pub struct Pagination {
    #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u32).range(1..=200))]
    pub limit: u32,
    #[arg(long, default_value_t = 0)]
    pub offset: u64,
}
#[derive(Debug, Args)]
pub struct ReadArgs {
    #[command(flatten)]
    pub page: Pagination,
    #[arg(long)]
    pub include_hidden: bool,
}
#[derive(Debug, Args)]
pub struct SearchArgs {
    pub query: String,
    #[command(flatten)]
    pub read: ReadArgs,
}

#[derive(Debug, Subcommand)]
pub enum RecoverAction {
    List,
    Show {
        id: String,
    },
    Retry {
        id: String,
    },
    Discard {
        id: String,
        #[arg(long)]
        yes: bool,
    },
}
#[derive(Debug, Subcommand)]
pub enum ThemeAction {
    List,
    Preview { name: String },
    Set { name: String },
}
#[derive(Debug, Subcommand)]
pub enum ConfigAction {
    Show,
    Set { key: String, value: String },
}

impl Command {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Add(_) | Self::Entry(_) => "add",
            Self::Write(_) => "write",
            Self::Show(_) => "show",
            Self::Today(_) => "today",
            Self::Recent(_) => "recent",
            Self::Search(_) => "search",
            Self::Tags(_) => "tags",
            Self::Moods(_) => "moods",
            Self::Context => "context",
            Self::Doctor => "doctor",
            Self::Status { .. } => "status",
            Self::Recover { .. } => "recover",
            Self::Enrich { .. } => "enrich",
            Self::Theme { .. } => "theme",
            Self::Fx { .. } => "fx",
            Self::Config { .. } => "config",
            Self::Completions { .. } => "completions",
            Self::Recall { .. } => "recall",
            Self::OnThisDay(_) => "on-this-day",
            Self::Calendar { .. } => "calendar",
            Self::Stats { .. } => "stats",
            Self::Garden { .. } => "garden",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn free_text_and_escaped_reserved_names_are_entries() {
        for input in [
            vec!["cap", "lovely", "walk"],
            vec!["cap", "--", "today", "was", "lovely"],
            vec!["cap", "write-the-entry-here"],
        ] {
            let cli = Cli::try_parse_from(input).unwrap();
            assert!(matches!(cli.command, Some(Command::Entry(_))));
        }
    }
    #[test]
    fn recognized_command_errors_never_fall_back_to_capture() {
        assert!(Cli::try_parse_from(["cap", "doctor", "nonsense"]).is_err());
        assert!(Cli::try_parse_from(["cap", "--wat", "text"]).is_err());
        assert!(Cli::try_parse_from(["cap", "recent", "--limit", "201"]).is_err());
    }
    #[test]
    fn sources_conflict_and_flags_before_text_are_parsed() {
        assert!(Cli::try_parse_from(["cap", "add", "--file", "note.md", "text"]).is_err());
        assert!(Cli::try_parse_from(["cap", "add", "--stdin", "--file", "note.md"]).is_err());
        let cli = Cli::try_parse_from([
            "cap",
            "--json",
            "add",
            "--tag",
            "life",
            "--mood",
            "content",
            "--",
            "--literal",
            "words",
        ])
        .unwrap();
        assert!(cli.global.json);
        let Some(Command::Add(add)) = cli.command else {
            panic!("add")
        };
        assert_eq!(add.text, ["--literal", "words"]);
        assert_eq!(add.tags, ["life"]);
    }
}
