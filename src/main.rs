use anyhow::{Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use lecop::{
    config::Config,
    dataset::Dataset,
    discover::find_datasets,
    fix,
    report::{self, DatasetReport, Format, Options},
    validate::{Checks, Issue, task_count, validate_paths},
};
use std::{io::IsTerminal, path::PathBuf, process::ExitCode};

#[derive(Parser)]
#[command(about = "Fast health checks for robot-learning datasets")]
struct Cli {
    /// Log to stderr: -v for progress, -vv for per-file timings. RUST_LOG overrides.
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    verbose: u8,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Clone, Copy, ValueEnum)]
enum Output {
    /// One line per issue
    Text,
    /// Text plus where each issue is and what its code means
    Full,
    /// A single JSON object: counts, ignored codes and an issues array
    Json,
    /// GitHub Actions workflow commands (annotations)
    Github,
}

#[derive(Subcommand)]
enum Cmd {
    /// Print dataset summary (metadata only, instant)
    Inspect { path: PathBuf },
    /// Check metadata, files, timestamps and values. Exit 0 clean, 1 on errors, 2 if a dataset or the config
    /// cannot be read.
    Validate {
        /// Dataset directories, or directories to search for datasets (default: the current directory)
        #[arg(default_value = ".")]
        paths: Vec<PathBuf>,
        /// Worker threads (default: one per core). The report is identical for any value.
        #[arg(short, long, default_value_t = 0)]
        jobs: usize,
        /// Output format
        #[arg(short, long, visible_alias = "output-format", value_enum, default_value_t = Output::Text)]
        format: Output,
        /// Never colour the output
        #[arg(long)]
        no_color: bool,
        /// Print the number of findings per issue code
        #[arg(long)]
        statistics: bool,
        /// Print only the summary, and nothing on success
        #[arg(short, long)]
        quiet: bool,
        /// Decode the video frames and report undecodable, frozen and black cameras. Slow; needs a build with
        /// `--features decode`.
        #[arg(long)]
        decode: bool,
        /// Use this config file instead of the nearest lecop.toml
        #[arg(long, conflicts_with = "isolated")]
        config: Option<PathBuf>,
        /// Ignore every config file
        #[arg(long)]
        isolated: bool,
        /// Apply the safe repairs: info.json totals (`total_frames`, `total_episodes`, `total_tasks`) recomputed from
        /// the episode and task metadata. The original is kept as `meta/info.json.bak`.
        #[arg(long)]
        fix: bool,
        /// Show the repairs `--fix` would make, and change nothing
        #[arg(long, visible_alias = "diff")]
        preview: bool,
        /// Issue codes to ignore, comma separated
        #[arg(long, value_delimiter = ',')]
        ignore: Vec<String>,
        /// Only report these issue codes, comma separated
        #[arg(long, value_delimiter = ',')]
        select: Vec<String>,
    },
}

struct ValidateArgs {
    paths: Vec<PathBuf>,
    jobs: usize,
    format: Output,
    no_color: bool,
    statistics: bool,
    quiet: bool,
    decode: bool,
    fix: bool,
    preview: bool,
    config: Option<PathBuf>,
    isolated: bool,
    ignore: Vec<String>,
    select: Vec<String>,
}

fn color_enabled(no_color: bool, format: Format) -> bool {
    let wanted = matches!(format, Format::Text | Format::Full);
    wanted
        && !no_color
        && std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty())
        && std::io::stdout().is_terminal()
}

fn validate(a: ValidateArgs) -> Result<u8> {
    if a.decode && !cfg!(feature = "decode") {
        bail!("--decode needs a build with `--features decode`");
    }
    let mut datasets = Vec::new();
    for p in &a.paths {
        datasets.extend(find_datasets(p)?);
    }
    datasets.sort();
    datasets.dedup();
    if datasets.is_empty() {
        let shown: Vec<_> = a.paths.iter().map(|p| p.display().to_string()).collect();
        bail!(
            "no LeRobot datasets (meta/info.json) found under {}",
            shown.join(", ")
        );
    }

    let mut config = match (&a.config, a.isolated) {
        (_, true) => Config::default(),
        (Some(p), _) => Config::load(p)?,
        (None, _) => match Config::find(&a.paths[0]) {
            Some(p) => Config::load(&p)?,
            None => Config::default(),
        },
    };
    config.add_cli(&a.ignore, &a.select)?;

    let checks = Checks {
        limits: config.limits().clone(),
        decode: a.decode,
        chunk_size: config.chunk_size(),
        clipped_fraction: config.clipped_fraction(),
    };
    let mut results = validate_paths(&datasets, a.jobs, &checks)?;
    match (a.fix, a.preview) {
        (false, false) => hint_fixable(&results),
        (fix, preview) => {
            if repair(&results, fix && !preview)? {
                results = validate_paths(&datasets, a.jobs, &checks)?;
            }
        }
    }
    let reports: Vec<DatasetReport> = results
        .into_iter()
        .map(|(path, outcome)| match outcome {
            Ok(issues) => {
                let (issues, ignored) = config.apply(&path, issues);
                DatasetReport {
                    path,
                    issues,
                    ignored,
                    open_error: None,
                }
            }
            Err(e) => {
                let mut r = DatasetReport::new(path, Vec::new());
                r.open_error = Some(format!("{e:#}"));
                r
            }
        })
        .collect();

    let format = match a.format {
        Output::Text => Format::Text,
        Output::Full => Format::Full,
        Output::Json => Format::Json,
        Output::Github => Format::Github,
    };
    let options = Options {
        format,
        color: color_enabled(a.no_color, format),
        statistics: a.statistics,
        quiet: a.quiet,
        decoded: a.decode,
    };
    print!("{}", report::render(&reports, &options));
    Ok(report::exit_code(&reports))
}

/// The safe repairs for one validated dataset. The dataset is opened again only when a finding could be repaired:
/// on large v3 datasets that costs more than the validation.
fn safe_repairs(path: &std::path::Path, outcome: &Result<Vec<Issue>>) -> Vec<fix::Fix> {
    let Ok(issues) = outcome else {
        return Vec::new();
    };
    if !fix::may_apply(issues) {
        return Vec::new();
    }
    match Dataset::open(path) {
        Ok(ds) => fix::plan(&ds, issues, task_count(&ds)),
        Err(_) => Vec::new(),
    }
}

/// Plan the safe repairs for every dataset that opened and print them to stderr, so `--format json` stays
/// parseable. Writes them with `write`. True when something was written.
fn repair(results: &[(PathBuf, Result<Vec<Issue>>)], write: bool) -> Result<bool> {
    let (mut planned, mut wrote) = (false, false);
    for (path, outcome) in results {
        let fixes = safe_repairs(path, outcome);
        for f in &fixes {
            let verb = if write { "fixed" } else { "would fix" };
            eprintln!(
                "{verb} {}: {} {} -> {} ({})",
                path.join("meta/info.json").display(),
                f.key,
                f.from,
                f.to,
                f.code
            );
        }
        planned |= !fixes.is_empty();
        if write && !fixes.is_empty() {
            fix::apply(path, &fixes)?;
            wrote = true;
        }
    }
    if !planned {
        eprintln!("no safe repair available");
    } else if !write {
        eprintln!(
            "rerun with --fix to apply them; the original info.json is kept as info.json.bak"
        );
    }
    Ok(wrote)
}

/// Tell, on stderr, how many datasets have a safe repair, as ruff does for its fixable findings.
fn hint_fixable(results: &[(PathBuf, Result<Vec<Issue>>)]) {
    let fixable = results
        .iter()
        .filter(|(path, outcome)| !safe_repairs(path, outcome).is_empty())
        .count();
    if fixable > 0 {
        let s = if fixable == 1 { "" } else { "s" };
        eprintln!("[*] {fixable} dataset{s} fixable with --fix (--preview shows how)");
    }
}

fn inspect(path: PathBuf) -> Result<u8> {
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
    Ok(0)
}

fn init_logging(verbose: u8) {
    let default = match verbose {
        0 => "off",
        1 => "info",
        _ => "debug",
    };
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(default));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(std::io::stderr().is_terminal())
        .init();
}

/// `lecop .` means `lecop validate .`: when no argument names a subcommand or asks for help,
/// `validate` is inserted so its flags and paths parse as usual.
fn with_default_command(mut args: Vec<std::ffi::OsString>) -> Vec<std::ffi::OsString> {
    const KNOWN: [&str; 5] = ["inspect", "validate", "help", "-h", "--help"];
    let named = args
        .iter()
        .skip(1)
        .any(|a| KNOWN.contains(&a.to_string_lossy().as_ref()));
    if args.len() > 1 && !named {
        args.insert(1, "validate".into());
    }
    args
}

fn main() -> ExitCode {
    let cli = Cli::parse_from(with_default_command(std::env::args_os().collect()));
    init_logging(cli.verbose);
    let result = match cli.cmd {
        Cmd::Inspect { path } => inspect(path),
        Cmd::Validate {
            paths,
            jobs,
            format,
            no_color,
            statistics,
            quiet,
            decode,
            fix,
            preview,
            config,
            isolated,
            ignore,
            select,
        } => validate(ValidateArgs {
            paths,
            jobs,
            format,
            no_color,
            statistics,
            quiet,
            decode,
            fix,
            preview,
            config,
            isolated,
            ignore,
            select,
        }),
    };
    match result {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::from(2)
        }
    }
}
