//! Fast health checks for robot-learning datasets in the [LeRobot](https://github.com/huggingface/lerobot) format
//! (v2.x and v3.x). This is the library behind the `lecop` command; use it to check a dataset from Rust code.
//!
//! [`dataset::Dataset::open`] reads a dataset's metadata and [`validate::validate`] runs every check on it, returning
//! the [`validate::Issue`]s it found:
//!
//! ```no_run
//! use lecop::dataset::Dataset;
//! use lecop::validate::{Severity, validate};
//!
//! let ds = Dataset::open("path/to/dataset")?;
//! let issues = validate(&ds)?;
//! for issue in &issues {
//!     println!("{:?} {} {}", issue.severity, issue.code, issue.message);
//! }
//! let errors = issues.iter().filter(|i| i.severity == Severity::Error).count();
//! # Ok::<(), anyhow::Error>(())
//! ```
//!
//! To check several datasets on one thread pool, with options such as decoding the video frames, find them with
//! [`discover::find_datasets`] and call [`validate::validate_paths`]. A dataset that cannot be opened comes back as
//! an `Err` for that path, and the others are still checked:
//!
//! ```no_run
//! use lecop::{discover::find_datasets, validate::{Checks, validate_paths}};
//! use std::path::Path;
//!
//! let paths = find_datasets(Path::new("datasets"))?;
//! let checks = Checks { decode: true, ..Checks::default() }; // needs the `decode` feature
//! for (path, result) in validate_paths(&paths, 0, &checks)? {
//!     match result {
//!         Ok(issues) => println!("{}: {} findings", path.display(), issues.len()),
//!         Err(e) => println!("{}: cannot open: {e:#}", path.display()),
//!     }
//! }
//! # Ok::<(), anyhow::Error>(())
//! ```
//!
//! Every issue has a stable `code`; [`validate::ALL_CODES`] lists them and [`validate::explain`] says what one means:
//!
//! ```
//! use lecop::validate::{ALL_CODES, explain};
//!
//! assert!(ALL_CODES.contains(&"timestamp_drift"));
//! assert!(!explain("timestamp_drift").is_empty());
//! ```
//!
//! # Stability
//!
//! The issue codes, the CLI's output formats and its exit codes are a public contract: a code is never renamed or
//! repurposed without a version bump. The Rust API is not stable before 1.0: signatures and public fields can change in
//! any 0.x release, and the changelog says when they do. Pin an exact version if you depend on it.
//!
//! # Features
//!
//! - `decode` (on by default) adds video frame decoding for H.264 and AV1; `--no-default-features` leaves it out.
//! - `asm` (off by default) builds rav1d's hand-written assembly, which decodes AV1 about 3 times faster (19 s
//!   against 60 s on one 1.2 GB dataset, Apple M-series; not measured on x86_64). ARM (aarch64) builds and
//!   x86_64 Linux, Windows and macOS builds always have it, with no extra tools, so the feature only matters on
//!   other targets, where it needs [nasm](https://www.nasm.us) on the build machine.
//!
//! A program that decodes video and wants the fast build enables it in its own `Cargo.toml`:
//!
//! ```toml
//! lecop = { version = "0.2", features = ["asm"] }
//! ```
pub mod config;
pub mod dataset;
pub mod decode;
pub mod discover;
pub mod fix;
pub mod mp4;
pub mod report;
pub mod validate;
