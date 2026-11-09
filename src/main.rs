use anyhow::{Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use lecop::{
    config::Config,
    dataset::Dataset,
    discover::find_datasets,
    report::{self, DatasetReport, Format, Options},
    validate::validate_paths,
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
        /// Use this config file instead of the nearest lecop.toml
        #[arg(long, conflicts_with = "isolated")]
        config: Option<PathBuf>,
        /// Ignore every config file
        #[arg(long)]
        isolated: bool,
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

    let reports: Vec<DatasetReport> = validate_paths(&datasets, a.jobs)?
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
    };
    print!("{}", report::render(&reports, &options));
    Ok(report::exit_code(&reports))
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
