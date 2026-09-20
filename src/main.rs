//! five-lines: review a PR diff against the ten rules of *Five Lines of Code*.

mod diff;
mod evaluate;
mod jev;
mod lang;
mod report;
mod review;
mod rules;
mod units;

use anyhow::{Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "five-lines", version, about = "Review a PR diff against the ten rules of Five Lines of Code")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Review a diff
    #[command(
        after_help = "Examples:\n  gh pr diff 123 | five-lines review - --repo .\n  five-lines review --repo . --base origin/main\n  five-lines review change.diff --no-jev"
    )]
    Review {
        /// Diff file, or '-' for stdin. Omit to diff --base...HEAD inside --repo
        diff: Option<String>,
        /// Checkout of the PR head: lets the tool read whole methods and resolve base classes
        #[arg(long)]
        repo: Option<PathBuf>,
        /// Base ref, e.g. origin/main: tells 'introduced' from 'grown', and supplies the diff if none is given
        #[arg(long)]
        base: Option<String>,
        /// Mechanical rules only; makes no network request
        #[arg(long)]
        no_jev: bool,
        /// Jev probability at which a judgment is raised; 0.55 up to this is 'worth a look'
        #[arg(long, default_value_t = 0.80)]
        threshold: f64,
        #[arg(long, value_enum, default_value_t = Format::Md)]
        format: Format,
        /// Methods per Jev request. One request can carry several methods; larger is faster
        #[arg(long, default_value_t = review::DEFAULT_BATCH)]
        batch: usize,
        /// Include findings suppressed as framework or language idioms
        #[arg(long)]
        show_suppressed: bool,
        /// Exit 1 if anything is raised (for CI)
        #[arg(long)]
        fail_on_findings: bool,
    },
    /// Score Jev's judgments against labelled snippets (bundled, or --cases FILE)
    Eval {
        #[arg(long)]
        cases: Option<PathBuf>,
        /// Write the full rows as JSON
        #[arg(long)]
        out: Option<PathBuf>,
        /// Snippets per Jev request, to measure what batching costs in accuracy
        #[arg(long, default_value_t = review::DEFAULT_BATCH)]
        batch: usize,
    },
    /// List the languages compiled into this binary
    Languages,
}

#[derive(Clone, Copy, ValueEnum)]
enum Format {
    Md,
    Json,
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("five-lines: {error:#}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<ExitCode> {
    match Cli::parse().command {
        Commands::Languages => println!("Parsed exactly: {}\nAnything else is reviewed from the hunk text, by Jev only.", lang::SUPPORTED),
        Commands::Eval { cases, out, batch } => evaluate::run(cases.as_deref(), out.as_deref(), batch)?,
        Commands::Review { diff, repo, base, no_jev, threshold, format, batch, show_suppressed, fail_on_findings } => {
            let diff_text = match (diff.as_deref(), &repo, &base) {
                (Some("-"), _, _) => {
                    let mut text = String::new();
                    std::io::stdin().read_to_string(&mut text)?;
                    text
                }
                (Some(path), _, _) => String::from_utf8_lossy(&std::fs::read(path)?).into_owned(),
                (None, Some(repo), Some(base)) => review::git_diff(repo, base)?,
                _ => bail!("give a diff file, '-' for stdin, or both --repo and --base"),
            };
            let options = review::Options { repo, base, use_jev: !no_jev, threshold, batch };
            let result = review::review(&diff_text, &options)?;
            match format {
                Format::Md => print!("{}", report::markdown(&result, show_suppressed)),
                Format::Json => print!("{}", report::as_json(&result)),
            }
            if fail_on_findings && result.findings.iter().any(|f| f.raised()) {
                return Ok(ExitCode::from(1));
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}
