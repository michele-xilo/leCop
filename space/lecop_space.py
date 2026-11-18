"""The Space's logic that needs neither the Hub nor Gradio: reading the dataset name, choosing the files to fetch,
and turning leCop's JSON report into the page."""

from __future__ import annotations

import json
import re
from collections import OrderedDict
from html import escape
from typing import Callable, Dict, Iterable, List, Optional, Tuple

CHECKS_URL = "https://github.com/michele-xilo/lecop/blob/main/docs/checks.md"
SITE_URL = "https://michele-xilo.github.io/leCop"
# Like the results site: a few findings per code in view, the rest folded, and a cap so a code with thousands of
# findings does not make the page unusable.
SHOWN_PER_CODE = 5
CAP_PER_CODE = 200

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

_REPO = re.compile(r"^[A-Za-z0-9][\w.-]*/[\w.-]+$")
_URL_PREFIX = re.compile(r"^(https?://)?(www\.)?huggingface\.co/datasets/")


class Refused(ValueError):
    """The request is not checked; the message says why and is shown to the user. `reason` names a cap or format
    refusal (`too_many_files`, `too_big`, `not_lerobot`) so the dataset list can say which ones will not fit."""

    def __init__(self, message: str, reason: Optional[str] = None):
        super().__init__(message)
        self.reason = reason


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


def plan_download(
    list_folder: Callable[[str], Iterable[Tuple[str, Optional[int]]]], cap_bytes: int, cap_files: int
) -> Tuple[List[str], int]:
    """Choose the files under `meta/` and `data/` and return them with their total size.

    `list_folder("meta")` yields the (path, size) of every file under that folder. The Hub's whole-repo file list
    stops at 100,000 names, which loses `meta/` in large repos, so each folder is listed on its own. The listing is
    read lazily and abandoned as soon as a cap is passed. Raises `Refused` when there is no `meta/info.json` or the
    files are more than `cap_files` or `cap_bytes`.
    """
    chosen: List[str] = []
    total = 0
    for folder in ("meta", "data"):
        for path, size in list_folder(folder):
            chosen.append(path)
            total += size or 0
            if len(chosen) > cap_files:
                raise Refused(
                    f"`meta/` and `data/` hold more than {cap_files:,} files, too many to download here. "
                    "Run leCop locally on this one: `pip install lecop`, then `lecop validate <path>`.",
                    "too_many_files",
                )
            if total > cap_bytes:
                raise Refused(
                    f"`meta/` and `data/` are over this Space's limit of {human_size(cap_bytes)}. "
                    "Run leCop locally on this one: `pip install lecop`, then `lecop validate <path>`.",
                    "too_big",
                )
        if folder == "meta" and "meta/info.json" not in chosen:
            raise Refused(
                "This repository has no `meta/info.json`, so it is not a LeRobot dataset leCop can read.", "not_lerobot"
            )
    return sorted(chosen), total


def fit_status(
    list_folder: Callable[[str], Iterable[Tuple[str, Optional[int]]]], cap_bytes: int, cap_files: int
) -> str:
    """`fits` when the check would download the dataset, else the reason it would refuse it."""
    try:
        plan_download(list_folder, cap_bytes, cap_files)
        return "fits"
    except Refused as e:
        return e.reason or "refused"


def load_fits(path: str, cap_bytes: int, cap_files: int) -> Dict[str, str]:
    """Which listed datasets fit the caps, as `build_fits.py` wrote them; empty when the file is missing, unreadable or
    was built for other caps."""
    try:
        with open(path) as f:
            fits = json.load(f)
    except (OSError, ValueError):
        return {}
    if fits.get("cap_bytes") != cap_bytes or fits.get("cap_files") != cap_files:
        return {}
    return dict(fits.get("status", {}))


def ordered_choices(
    repos: Iterable[str], status: Dict[str, str], cap_bytes: int, cap_files: int
) -> List[Tuple[str, str]]:
    """Dropdown (label, value) pairs: datasets that fit first, then those not classified yet, then the ones over a cap
    with the reason in the label. Repos with no `meta/info.json` are left out: the Hub tags them LeRobot, but leCop
    cannot read them. The given order is kept within each group."""
    why = {
        "too_big": f"over {human_size(cap_bytes)}",
        "too_many_files": f"over {cap_files:,} files",
    }
    fits, unknown, refused = [], [], []
    for repo in repos:
        state = status.get(repo)
        if state == "not_lerobot":
            continue
        if state == "fits":
            fits.append((repo, repo))
        elif state in why:
            refused.append((f"{repo}: {why[state]}", repo))
        else:
            unknown.append((repo, repo))
    return fits + unknown + sorted(refused, key=lambda c: list(why).index(status[c[1]]))


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


def plural(n: int, word: str) -> str:
    return f"{n:,} {word}{'' if n == 1 else 's'}"


def dataset_choices(hub_ids: Iterable[str], index: Iterable[Dict]) -> List[str]:
    """The dropdown's datasets: the Hub's, in its order, then the results site's that the Hub list missed."""
    seen = set()
    out = []
    for repo in list(hub_ids) + [e.get("repo") for e in index]:
        if repo and repo not in seen:
            seen.add(repo)
            out.append(repo)
    return out


def site_page(repo: str, index: Iterable[Dict]) -> Optional[str]:
    """The results-site page of `repo`, if the site has a record of it (from the site's index.json)."""
    for entry in index:
        if entry.get("repo") == repo and entry.get("name"):
            return f"{SITE_URL}/datasets/{entry['name']}.html"
    return None


def _finding(issue: Dict) -> str:
    where = "dataset" if issue.get("episode") is None else f"episode {issue['episode']}"
    return f'<div class="lc-find"><span class="lc-muted">{where}</span>{escape(issue["message"])}</div>'


def _group(code: str, issues: List[Dict]) -> str:
    shown = issues[:SHOWN_PER_CODE]
    rest = issues[SHOWN_PER_CODE:CAP_PER_CODE]
    more = ""
    if rest:
        hidden = len(issues) - SHOWN_PER_CODE - len(rest)
        tail = f'<div class="lc-find lc-muted">+ {hidden:,} not shown, run leCop locally to see them all</div>' if hidden else ""
        more = f'<details class="lc-more"><summary>+ {len(issues) - SHOWN_PER_CODE:,} more</summary>{"".join(map(_finding, rest))}{tail}</details>'
    severity = escape(issues[0]["severity"])
    return (
        f'<section class="lc-group"><div class="lc-ghead"><span class="lc-dot lc-{severity}" aria-hidden="true"></span>'
        f'<strong class="lc-mono">{escape(code)}</strong><span class="lc-muted">{len(issues):,}</span><span class="lc-grow"></span>'
        f'<a class="lc-link lc-small" href="{CHECKS_URL}" target="_blank">About this check</a></div>'
        f'{"".join(map(_finding, shown))}{more}</section>'
    )


def render_html(repo: str, revision: str, report: Dict, version: str, site_url: Optional[str] = None) -> str:
    """The report in the results site's look: a summary strip, then one group per issue code."""
    errors, warnings = report["errors"], report["warnings"]
    counts = ""
    if errors:
        counts += f'<span class="lc-error">{plural(errors, "error")}</span>'
    if warnings:
        counts += f'<span class="lc-warning">{plural(warnings, "warning")}</span>'
    if not counts:
        counts = '<span class="lc-clean">Clean</span>'
    hub = f"https://huggingface.co/datasets/{repo}/tree/{revision}"
    groups = group_by_code(report["issues"])
    body = "".join(_group(c, i) for c, i in groups.items()) or (
        '<p class="lc-muted">leCop reported nothing on the metadata and data.</p>'
    )
    site = (
        f' The <a class="lc-link" href="{escape(site_url)}" target="_blank">results site</a> has a full run of this dataset, videos included.'
        if site_url
        else ""
    )
    return (
        f'<div class="lc-report"><h2 class="lc-title">{escape(repo)}</h2>'
        f'<div class="lc-mono lc-muted"><a class="lc-link" href="{escape(hub)}" target="_blank">huggingface.co/datasets/{escape(repo)}</a>'
        f" @ {escape(revision[:7])} &middot; leCop {escape(version)}</div>"
        f'<div class="lc-strip"><span class="lc-big">{counts}</span></div>'
        f'<p class="lc-note">What leCop reported on <code>meta/</code> and <code>data/</code>, not a verdict. '
        f"The video checks were skipped: this Space does not download the videos.{site}</p>"
        f"{body}</div>"
    )


def notice_html(message: str) -> str:
    """A request that was not checked, with the reason. `message` may hold `code` spans in backticks."""
    text = re.sub(r"`([^`]*)`", r"<code>\1</code>", escape(message))
    return f'<div class="lc-report"><p class="lc-notice"><strong>Not checked.</strong> {text}</p></div>'
