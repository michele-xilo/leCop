use anyhow::Result;
use clap::{Parser, Subcommand};
use lecop::{
    dataset::Dataset,
    validate::{Severity, validate_with},
};
use std::{path::PathBuf, process::ExitCode};

#[derive(Parser)]
#[command(about = "Fast health checks for robot-learning datasets")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
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
        Cmd::Validate { path, jobs } => {
            let issues = validate_with(&Dataset::open(path)?, jobs)?;
            for x in &issues {
                let ep = x.episode.map_or("-".into(), |e| e.to_string());
                println!(
                    "{:<7} ep {ep:<6} {:<24} {}",
                    format!("{:?}", x.severity).to_uppercase(),
                    x.code,
                    x.message
                );
            }
            let errors = issues
                .iter()
                .filter(|x| x.severity == Severity::Error)
                .count();
            println!("{errors} errors, {} warnings", issues.len() - errors);
            Ok(if errors > 0 {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            })
        }
    }
}
