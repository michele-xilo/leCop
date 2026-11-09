use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use lecop::{
    dataset::Dataset,
    report::{self, Format},
    validate::validate_with,
};
use std::{path::PathBuf, process::ExitCode};

#[derive(Parser)]
#[command(about = "Fast health checks for robot-learning datasets")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}
#[derive(Clone, Copy, ValueEnum)]
enum Output {
    /// One line per issue
    Text,
    /// A single JSON object: counts and an issues array
    Json,
    /// GitHub Actions workflow commands (annotations)
    Github,
}
#[derive(Subcommand)]
enum Cmd {
    /// Print dataset summary (metadata only, instant)
    Inspect { path: PathBuf },
    /// Check metadata, files, timestamps and values; exit 1 on errors
    Validate {
        path: PathBuf,
        /// Worker threads (default: one per core). The report is identical for any value.
        #[arg(short, long, default_value_t = 0)]
        jobs: usize,
        /// Output format
        #[arg(short, long, value_enum, default_value_t = Output::Text)]
        format: Output,
    },
}

fn main() -> Result<ExitCode> {
    match Cli::parse().cmd {
        Cmd::Inspect { path } => {
            let ds = Dataset::open(path)?;
            let i = &ds.info;
            println!("version   {}", i.codebase_version);
            println!("robot     {}", i.robot_type.as_deref().unwrap_or("?"));
            println!("episodes  {}", i.total_episodes);
            println!(
                "frames    {} ({:.2} h @ {} fps)",
                i.total_frames,
                i.total_frames as f64 / i.fps / 3600.0,
                i.fps
            );
            for (k, f) in &i.features {
                println!("feature   {k:<34} {:<8} {:?}", f.dtype, f.shape);
            }
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Validate { path, jobs, format } => {
            let issues = validate_with(&Dataset::open(path)?, jobs)?;
            let format = match format {
                Output::Text => Format::Text,
                Output::Json => Format::Json,
                Output::Github => Format::Github,
            };
            print!("{}", report::render(&issues, format));
            Ok(if report::counts(&issues).0 > 0 {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            })
        }
    }
}
