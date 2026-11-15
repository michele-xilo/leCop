# Using leCop from code

leCop is a command-line tool first. Two APIs sit on top of it: a small Python module in the PyPI wheel and the Rust
library crate. Both return the same findings the CLI prints, with the same stable issue codes (see
[Checks](checks.md)).

## Python

`pip install lecop` (or `uv add lecop`) installs the `lecop` command and the `lecop` module. The module runs the
bundled binary with `--format json` and returns the parsed report, so it costs a process start (a few milliseconds)
on top of the check, and `decode=True` uses the same AV1 assembly as the CLI in the wheels.

```python
import lecop

report = lecop.validate("path/to/dataset")
if report["errors"]:
    for issue in report["issues"]:
        if issue["severity"] == "error":
            print(issue["dataset"], issue["episode"], issue["code"], issue["message"])
    raise SystemExit("dataset has errors")
```

### `lecop.validate(path, *, decode=False, jobs=None, ignore=(), select=())`

| Argument | Meaning |
|---|---|
| `path` | A dataset (a directory with `meta/info.json`) or a directory that holds many; `str` or `os.PathLike`. |
| `decode` | Decode the video frames and check them (`--decode`); see [Decoding frames](usage.md#decoding-frames). |
| `jobs` | Worker threads (`-j`); `None` is one per core. The report is the same for every value. |
| `ignore`, `select` | Issue codes to drop, or to keep only (`--ignore`, `--select`). An unknown code raises `LecopError`. |

A `lecop.toml` in the scanned directory or a parent applies, as on the command line.

The return value is a `dict`, the CLI's `--format json` object:

| Key | Value |
|---|---|
| `errors`, `warnings` | Counts of findings, after `ignore` and the config. |
| `ignored` | `{issue code: count}` of the findings dropped by `ignore` or the config. |
| `issues` | A list of dicts with `dataset` (path), `severity` (`"error"` or `"warning"`), `episode` (`int`, or `None` for the dataset as a whole), `code` and `message`. |

Findings in the dataset never raise: a dataset leCop cannot open is a `cannot_open` issue in `issues`, and `errors` says
how many there are. `lecop.LecopError` (a `RuntimeError`) is raised when there is no report at all: the path is not a
directory or holds no dataset, an option or the config is invalid, or the binary is missing or crashed.

The module has type annotations and a `py.typed` marker. It is pre-1.0: the report and the issue codes are the stable
part; the function signature can gain arguments and, before 1.0, change.

There is no Python module in a `cargo install lecop` build; run the CLI with `--format json` instead.

## Rust

`lecop` is also a library crate: `Dataset::open` reads a dataset's metadata and `validate` returns its `Issue`s. The
[crate documentation](https://docs.rs/lecop) has examples for one dataset, for many datasets on one thread pool with
`Checks` (decoding, limits), and the stability statement.

```rust
use lecop::dataset::Dataset;
use lecop::validate::validate;

let ds = Dataset::open("path/to/dataset")?;
for issue in validate(&ds)? {
    println!("{:?} {} {}", issue.severity, issue.code, issue.message);
}
```

The issue codes and the CLI output are the stable contract. The Rust API can change in any 0.x release, so pin the
version. AV1 decoding speed is the same as in the wheels on ARM and on x86_64 Linux, Windows and macOS; see
[Decoding frames](usage.md#decoding-frames).
