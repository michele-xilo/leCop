"""A Hugging Face Space: type a public Hub dataset and get leCop's report on its metadata and data."""

from __future__ import annotations

import json
import os
import subprocess
import tempfile
from typing import Optional, Tuple

import gradio as gr
from huggingface_hub import HfApi, snapshot_download
from huggingface_hub.utils import GatedRepoError, HfHubHTTPError, RepositoryNotFoundError, RevisionNotFoundError

import lecop
from lecop_space import VIDEO_CODES, Refused, parse_repo, plan_download, refuse_access, render_markdown

CAP_BYTES = int(os.environ.get("LECOP_SPACE_MAX_BYTES", 2_000_000_000))
VALIDATE_TIMEOUT_S = int(os.environ.get("LECOP_SPACE_TIMEOUT_S", 300))

# Without a token the Space reads only what an anonymous visitor can; a token set on the Space must not leak into
# downloads of gated or private repos.
api = HfApi(token=False)


def lecop_version() -> str:
    run = subprocess.run([lecop._binary(), "--version"], capture_output=True, text=True)
    return run.stdout.strip().split()[-1] if run.returncode == 0 else "?"


def check(text: str, revision_text: str = "") -> Tuple[str, Optional[dict]]:
    """Validate a Hub dataset; return the Markdown page and the raw report (None when there is none)."""
    try:
        repo, revision = parse_repo(text)
        revision = revision_text.strip() or revision or "main"
        try:
            info = api.dataset_info(repo, revision=revision, files_metadata=True)
        except GatedRepoError:
            raise Refused("This dataset is gated. The Space checks public, ungated datasets only.")
        except RevisionNotFoundError:
            raise Refused(f"`{repo}` has no revision `{revision}`.")
        except RepositoryNotFoundError:
            raise Refused(f"No public dataset `{repo}` on the Hub (it may be private).")
        refuse_access(info.private, info.gated)
        files, _ = plan_download(((s.rfilename, s.size) for s in info.siblings or []), CAP_BYTES)
        with tempfile.TemporaryDirectory() as root:
            snapshot_download(
                repo, repo_type="dataset", revision=info.sha, local_dir=root, allow_patterns=files, token=False
            )
            run = subprocess.run(
                [lecop._binary(), "validate", "--format", "json", "--isolated", "--ignore", ",".join(VIDEO_CODES), root],
                capture_output=True,
                text=True,
                timeout=VALIDATE_TIMEOUT_S,
            )
        if not run.stdout.strip():
            raise Refused(f"leCop gave no report: {run.stderr.strip() or f'exit code {run.returncode}'}")
        report = json.loads(run.stdout)
        return render_markdown(repo, info.sha, report, lecop_version()), report
    except Refused as e:
        return f"**Not checked.** {e}", None
    except subprocess.TimeoutExpired:
        return f"**Not checked.** leCop took longer than {VALIDATE_TIMEOUT_S} s on this dataset; run it locally.", None
    except HfHubHTTPError as e:
        return f"**Not checked.** The Hub answered with an error: {e}", None


with gr.Blocks(title="leCop: check a LeRobot dataset") as demo:
    gr.Markdown(
        "# leCop\n"
        "Fast health checks for [LeRobot](https://github.com/huggingface/lerobot) datasets (unofficial). "
        "Type a public Hub dataset; the Space downloads its `meta/` and `data/` folders (no videos) and runs "
        "`lecop validate`. [Source and docs](https://github.com/michele-xilo/lecop)."
    )
    with gr.Row():
        repo_box = gr.Textbox(label="Dataset", placeholder="lerobot/pusht", scale=3)
        rev_box = gr.Textbox(label="Revision (optional)", placeholder="main", scale=1)
    button = gr.Button("Check", variant="primary")
    page = gr.Markdown()
    with gr.Accordion("JSON report", open=False):
        raw = gr.JSON()
    gr.Examples([["lerobot/pusht", ""]], inputs=[repo_box, rev_box])
    button.click(check, inputs=[repo_box, rev_box], outputs=[page, raw], concurrency_limit=2)
    repo_box.submit(check, inputs=[repo_box, rev_box], outputs=[page, raw], concurrency_limit=2)

if __name__ == "__main__":
    demo.launch()
