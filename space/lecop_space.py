"""The Space's logic that needs neither the Hub nor Gradio: reading the dataset name, choosing the files to fetch,
and turning leCop's JSON report into the page."""

from __future__ import annotations

import re
from collections import OrderedDict
from typing import Dict, Iterable, List, Optional, Tuple

CHECKS_URL = "https://github.com/michele-xilo/lecop/blob/main/docs/checks.md"

# The Space downloads no videos, so the checks that read them would report every file missing.
VIDEO_CODES = (
    "video_missing",
    "video_unreadable",
    "video_fps",
    "video_resolution",
    "video_frame_count",
    "video_span_length",
    "video_span_beyond_end",
)

FETCHED_DIRS = ("meta/", "data/")

_REPO = re.compile(r"^[A-Za-z0-9][\w.-]*/[\w.-]+$")
_URL_PREFIX = re.compile(r"^(https?://)?(www\.)?huggingface\.co/datasets/")


class Refused(ValueError):
    """The request is not checked; the message says why and is shown to the user."""


def parse_repo(text: str) -> Tuple[str, Optional[str]]:
    """Return (repo, revision) from `owner/name`, `owner/name@revision` or a Hub dataset URL."""
    text = text.strip()
    text = _URL_PREFIX.sub("", text)
    revision = None
    tree = re.match(r"^([^/]+/[^/]+)/tree/([^/]+)", text)
    if tree:
        text, revision = tree.group(1), tree.group(2)
    elif "@" in text:
        text, revision = text.split("@", 1)
        revision = revision.strip() or None
    text = text.strip().strip("/")
    if not _REPO.match(text) or ".." in text:
        raise Refused(f"`{text or '(empty)'}` is not a Hub dataset name; type it as `owner/name`, e.g. `lerobot/pusht`.")
    return text, revision


def refuse_access(private: Optional[bool], gated) -> None:
    """Raise `Refused` for a private or gated dataset: the Space holds no token and asks for none."""
    if private:
        raise Refused("This dataset is private. The Space checks public datasets only; run `lecop` locally instead.")
    if gated:
        raise Refused("This dataset is gated. The Space checks public, ungated datasets only; run `lecop` locally instead.")


def plan_download(files: Iterable[Tuple[str, Optional[int]]], cap_bytes: int) -> Tuple[List[str], int]:
    """Choose the files under `meta/` and `data/` and return them with their total size.

    Raises `Refused` when there is no `meta/info.json` or the files add up to more than `cap_bytes`.
    """
    chosen = [(path, size or 0) for path, size in files if path.startswith(FETCHED_DIRS)]
    if not any(path == "meta/info.json" for path, _ in chosen):
        raise Refused("This repository has no `meta/info.json`, so it is not a LeRobot dataset leCop can read.")
    total = sum(size for _, size in chosen)
    if total > cap_bytes:
        raise Refused(
            f"`meta/` and `data/` hold {human_size(total)}, over this Space's limit of {human_size(cap_bytes)}. "
            "Run leCop locally on this one: `pip install lecop`, then `lecop validate <path>`."
        )
    return sorted(path for path, _ in chosen), total


def human_size(n: int) -> str:
    size = float(n)
    for unit in ("B", "KB", "MB", "GB"):
        if size < 1000 or unit == "GB":
            return f"{size:.0f} {unit}" if unit == "B" else f"{size:.1f} {unit}"
        size /= 1000
    return f"{size:.1f} GB"


def group_by_code(issues: Iterable[Dict]) -> "OrderedDict[str, List[Dict]]":
    """Issues grouped by code: errors before warnings, then codes by name; within a code, leCop's order."""
    issues = list(issues)
    rank = {"error": 0, "warning": 1}
    codes = sorted({i["code"] for i in issues}, key=lambda c: (min(rank.get(i["severity"], 2) for i in issues if i["code"] == c), c))
    return OrderedDict((c, [i for i in issues if i["code"] == c]) for c in codes)


def render_markdown(repo: str, revision: str, report: Dict, version: str) -> str:
    """The report as Markdown: a summary line, then one section per issue code."""
    errors, warnings = report["errors"], report["warnings"]
    lines = [
        f"### leCop {version} on [`{repo}`](https://huggingface.co/datasets/{repo}/tree/{revision}) at `{revision[:12]}`",
        "",
        f"**{errors} error{'s' if errors != 1 else ''}, {warnings} warning{'s' if warnings != 1 else ''}.** "
        f"Video checks were skipped: this Space reads `meta/` and `data/` only. "
        f"What each code means: [checks]({CHECKS_URL}).",
        "",
    ]
    groups = group_by_code(report["issues"])
    if not groups:
        lines.append("leCop reported nothing on the metadata and data.")
    for code, issues in groups.items():
        severity = issues[0]["severity"]
        lines.append(f"#### `{code}` ({severity}, {len(issues)})")
        for i in issues:
            where = f"episode {i['episode']}: " if i.get("episode") is not None else ""
            lines.append(f"- {where}{i['message']}")
        lines.append("")
    return "\n".join(lines)
