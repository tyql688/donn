mod commands;
mod tui;

use clap::{Parser, Subcommand};

/// donn: single-binary Claude Code profile manager.
/// Run without arguments to open the TUI (create, edit, remove, aliases, presets).
#[derive(Parser)]
#[command(name = "donn", version, about, arg_required_else_help = false)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Sync the profile, then exec Claude with it (what generated wrappers call)
    Run {
        /// Profile name
        name: String,
        /// Arguments passed through to claude (after `--`)
        #[arg(last = true)]
        args: Vec<String>,
    },
    /// Compact table: name / preset / base_url / sonnet model / aliases
    List,
    /// Health checks; exit status is non-zero when any check fails
    Doctor,
}

fn main() {
    let cli = Cli::parse();
    let code = match cli.command {
        Some(Command::Run { name, args }) => commands::run::execute(&name, &args),
        Some(Command::List) => commands::list::execute(),
        Some(Command::Doctor) => commands::doctor::execute(),
        None => tui::run(),
    };
    std::process::exit(code);
}
