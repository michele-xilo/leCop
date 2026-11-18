"""A Hugging Face Space: pick a public Hub dataset and get leCop's report on its metadata and data."""

from __future__ import annotations

import json
import os
import subprocess
import tempfile
import urllib.request
from typing import List, Optional, Tuple

import gradio as gr
from huggingface_hub import HfApi, snapshot_download
from huggingface_hub.utils import GatedRepoError, HfHubHTTPError, RepositoryNotFoundError, RevisionNotFoundError

import lecop
from lecop_space import (
    CHECKS_URL,
    SITE_URL,
    VIDEO_CODES,
    Refused,
    human_size,
    notice_html,
    parse_repo,
    plan_download,
    refuse_access,
    dataset_choices,
    render_html,
    site_page,
)

CAP_BYTES = int(os.environ.get("LECOP_SPACE_MAX_BYTES", 2_000_000_000))
VALIDATE_TIMEOUT_S = int(os.environ.get("LECOP_SPACE_TIMEOUT_S", 300))
REPO_URL = "https://github.com/michele-xilo/lecop"
LOGO_URL = "https://raw.githubusercontent.com/michele-xilo/leCop/main/assets/logo.png"
EXAMPLES = ["lerobot/pusht", "lerobot/aloha_sim_insertion_human", "lerobot/svla_so100_pickplace"]

# Without a token the Space reads only what an anonymous visitor can; a token set on the Space must not leak into
# downloads of gated or private repos.
api = HfApi(token=False)


def lecop_version() -> str:
    run = subprocess.run([lecop._binary(), "--version"], capture_output=True, text=True)
    return run.stdout.strip().split()[-1] if run.returncode == 0 else "?"


def site_index() -> list:
    """The results site's list of datasets, or an empty list when the site cannot be reached."""
    try:
        with urllib.request.urlopen(f"{SITE_URL}/index.json", timeout=5) as r:
            return json.load(r)
    except (OSError, ValueError):
        return []


def lerobot_datasets(limit: int) -> List[str]:
    """Public, ungated datasets the Hub tags as LeRobot, most downloaded first."""
    try:
        found = api.list_datasets(filter="LeRobot", sort="downloads", limit=limit)
        return [d.id for d in found if not d.private and not d.gated]
    except HfHubHTTPError:
        return []


VERSION = lecop_version()
SITE_INDEX = site_index()
# The dropdown filters its choices in the browser as you type, so it gets one list loaded at startup rather than a
# Hub search per keystroke (which Gradio shows a keystroke late). 2,000 names load in about 1.5 s; any other dataset
# can still be typed in full.
CHOICES = dataset_choices(lerobot_datasets(2000), SITE_INDEX)


def check(text: Optional[str], revision_text: str = "", progress=gr.Progress()) -> Tuple[str, Optional[dict]]:
    """Validate a Hub dataset; return the report page and the raw report (None when there is none)."""
    try:
        repo, revision = parse_repo(text or "")
        revision = (revision_text or "").strip() or revision or "main"
        progress(0, desc="Reading the file list from the Hub")
        try:
            info = api.dataset_info(repo, revision=revision, files_metadata=True)
        except GatedRepoError:
            raise Refused("This dataset is gated. The Space checks public, ungated datasets only.")
        except RevisionNotFoundError:
            raise Refused(f"`{repo}` has no revision `{revision}`.")
        except RepositoryNotFoundError:
            raise Refused(f"No public dataset `{repo}` on the Hub (it may be private).")
        refuse_access(info.private, info.gated)
        files, total = plan_download(((s.rfilename, s.size) for s in info.siblings or []), CAP_BYTES)
        progress(0.2, desc=f"Downloading meta/ and data/: {len(files):,} files, {human_size(total)}")
        with tempfile.TemporaryDirectory() as root:
            snapshot_download(
                repo, repo_type="dataset", revision=info.sha, local_dir=root, allow_patterns=files, token=False
            )
            progress(0.9, desc="Running leCop")
            run = subprocess.run(
                [lecop._binary(), "validate", "--format", "json", "--isolated", "--ignore", ",".join(VIDEO_CODES), root],
                capture_output=True,
                text=True,
                timeout=VALIDATE_TIMEOUT_S,
            )
        if not run.stdout.strip():
            raise Refused(f"leCop gave no report: {run.stderr.strip() or f'exit code {run.returncode}'}")
        report = json.loads(run.stdout)
        return render_html(repo, info.sha, report, VERSION, site_page(repo, SITE_INDEX)), report
    except Refused as e:
        return notice_html(str(e)), None
    except subprocess.TimeoutExpired:
        return notice_html(f"leCop took longer than {VALIDATE_TIMEOUT_S} s on this dataset; run it locally."), None
    except HfHubHTTPError as e:
        return notice_html(f"The Hub answered with an error: {e}"), None


GITHUB_MARK = (
    '<svg width="20" height="20" viewBox="0 0 16 16" fill="currentColor" aria-hidden="true"><path d="M8 0C3.58 0 0 3.58 0 8c0 '
    "3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28"
    "-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87"
    ".31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82a7.6 7.6 0 0 1 4 0c1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08"
    " 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01"
    ' 8.01 0 0 0 16 8c0-4.42-3.58-8-8-8z"/></svg>'
)
HEADER = (
    f'<div class="lc-top"><a class="lc-brand" href="{REPO_URL}" target="_blank"><img src="{LOGO_URL}" alt="">leCop</a>'
    f'<span class="lc-grow"></span><a href="{SITE_URL}/" target="_blank">Results</a>'
    f'<a href="{CHECKS_URL}" target="_blank">Checks</a>'
    f'<a href="{REPO_URL}" target="_blank" aria-label="GitHub">{GITHUB_MARK}</a></div>'
    '<p class="lc-lede">Find the broken episodes in a <a class="lc-link" href="https://github.com/huggingface/lerobot" '
    'target="_blank">LeRobot</a> dataset before they cost you a training run. Pick a public dataset on the Hub: the Space '
    "downloads its <code>meta/</code> and <code>data/</code> folders (no videos) and runs <code>lecop validate</code>.</p>"
)
FOOTER = (
    '<p class="lc-fine">On your own machine, with the video checks: <code>uvx lecop path/to/dataset</code>. '
    "Public, ungated datasets only, up to "
    f"{human_size(CAP_BYTES)} of <code>meta/</code> and <code>data/</code>. leCop is unofficial and not affiliated with "
    f'Hugging Face or LeRobot. <a class="lc-link" href="{REPO_URL}/issues/new?template=wrong-finding.yml" '
    'target="_blank">Report a wrong finding</a></p>'
)

# The results site's palette (GitHub's), with the logo's orange as the accent. On a Space, Gradio wraps the HTML in its
# `.prose` styles, which underline links and recolour text over plain class rules, hence the !important.
CSS = """
:root{--lc-ok:#1a7f37;--lc-warn:#9a6700;--lc-err:#cf222e;--lc-link:#0969da;--lc-accent:#b64605}
.dark{--lc-ok:#3fb950;--lc-warn:#d29922;--lc-err:#f85149;--lc-link:#4493f8}
.gradio-container{max-width:880px!important;margin:0 auto!important}
.lc-top{display:flex;align-items:center;gap:16px;padding:4px 0 12px;border-bottom:1px solid var(--border-color-primary)}
.lc-top a{color:var(--body-text-color-subdued)!important;text-decoration:none!important;display:flex;align-items:center}
.lc-top a:hover{color:var(--body-text-color)!important}
.lc-top .lc-brand{gap:10px;font-size:18px;font-weight:600;color:var(--body-text-color)!important}
.lc-top img{width:32px;height:32px;border-radius:50%}
.lc-grow{flex-grow:1}
.lc-lede{margin:16px 0 4px;color:var(--body-text-color-subdued)}
.lc-link,.lc-report a{color:var(--lc-link)!important;text-decoration:none!important;padding:0!important}
.lc-mono,.lc-find{font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace}
.lc-muted{color:var(--body-text-color-subdued)!important}
.lc-small{font-size:12px}
.lc-title{margin:8px 0 4px!important;font-size:22px;font-weight:600;overflow-wrap:anywhere}
.lc-report .lc-mono.lc-muted{font-size:12px;overflow-wrap:anywhere}
.lc-strip{display:flex;gap:16px;margin:16px 0 0;padding:12px 0;border-top:1px solid var(--border-color-primary);border-bottom:1px solid var(--border-color-primary)}
.lc-big{font-size:18px;font-weight:600;display:flex;gap:16px}
.lc-error{color:var(--lc-err)!important}.lc-warning{color:var(--lc-warn)!important}.lc-clean{color:var(--lc-ok)!important}
.lc-note{margin:12px 0 24px;font-size:13px;color:var(--body-text-color-subdued)}
.lc-group{margin:0 0 20px}
.lc-ghead{display:flex;align-items:center;gap:8px}
.lc-ghead strong{font-size:14px}
.lc-dot{width:8px;height:8px;border-radius:50%;flex:none}
.lc-dot.lc-error{background:var(--lc-err)}.lc-dot.lc-warning{background:var(--lc-warn)}
.lc-find{margin:2px 0 0 16px;font-size:12px;overflow-wrap:anywhere}
.lc-find .lc-muted{margin-right:8px}
.lc-more{margin:4px 0 0 16px;font-size:12px;color:var(--body-text-color-subdued)}
.lc-more summary{cursor:pointer;width:max-content}
.lc-more .lc-find{margin-left:0}
.lc-notice{padding:12px 14px;border:1px solid var(--border-color-primary);border-left:3px solid var(--lc-accent);border-radius:6px}
.lc-fine{margin-top:24px;font-size:12px;color:var(--body-text-color-subdued)}
"""

THEME = gr.themes.Base(
    primary_hue="orange",
    neutral_hue="gray",
    font=["-apple-system", "BlinkMacSystemFont", "Segoe UI", "Noto Sans", "Helvetica", "Arial", "sans-serif"],
    font_mono=["ui-monospace", "SFMono-Regular", "Menlo", "Consolas", "monospace"],
).set(
    button_primary_background_fill="#b64605",
    button_primary_background_fill_hover="#9a3b04",
    button_primary_background_fill_dark="#b64605",
    button_primary_background_fill_hover_dark="#d0560c",
    button_primary_text_color="#ffffff",
    button_primary_text_color_dark="#ffffff",
)

with gr.Blocks(title="leCop: check a LeRobot dataset") as demo:
    gr.HTML(HEADER)
    with gr.Row(equal_height=True):
        repo_box = gr.Dropdown(
            choices=CHOICES,
            value=None,
            label="Dataset",
            info=f"Type to filter {len(CHOICES):,} LeRobot datasets on the Hub, most downloaded first. Any other owner/name works too.",
            allow_custom_value=True,
            filterable=True,
            scale=4,
        )
        rev_box = gr.Textbox(label="Revision", placeholder="main", info="Branch, tag or commit.", scale=1)
    button = gr.Button("Check", variant="primary")
    examples = gr.Examples([[e] for e in EXAMPLES], inputs=[repo_box], label="Or try one from the results site")
    page = gr.HTML()
    with gr.Accordion("JSON report", open=False):
        raw = gr.JSON()
    gr.HTML(FOOTER)
    button.click(check, inputs=[repo_box, rev_box], outputs=[page, raw], concurrency_limit=2)
    examples.load_input_event.then(check, inputs=[repo_box, rev_box], outputs=[page, raw], concurrency_limit=2)

if __name__ == "__main__":
    demo.launch(theme=THEME, css=CSS)
