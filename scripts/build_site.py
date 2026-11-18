#!/usr/bin/env python3
"""Build the results site from site/data/runs/*.json into site/dist/ (standard library only).

    scripts/build_site.py            writes site/dist/index.html, index.json, datasets/<name>.html, assets/

Every page is plain HTML with inline CSS. The home page lists every record and the dataset pages list every
finding without JavaScript; a small script adds search and a status filter, with the state in the URL.
"""

import html
import json
import os
import shutil
import sys
from urllib.parse import urlencode

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
RUNS = os.path.join(ROOT, "site", "data", "runs")
DIST = os.path.join(ROOT, "site", "dist")
REPO_URL = "https://github.com/michele-xilo/leCop"
DOCS_URL = REPO_URL + "/tree/main/docs"
CAP = 200  # findings shown per code on a dataset page; a few datasets have thousands of one code

THEMES = {
    "light": dict(bg="#ffffff", line="#d8dee4", text="#1f2328", muted="#59636e", link="#0969da",
                  ok="#1a7f37", warn="#9a6700", err="#cf222e"),
    "dark": dict(bg="#0d1117", line="#30363d", text="#f0f6fc", muted="#9198a1", link="#4493f8",
                 ok="#3fb950", warn="#d29922", err="#f85149"),
}
# One line per issue code, shown under each finding group; see docs/checks.md for the full rules.
DESCRIPTIONS = {
    "action_constant": "The action never changes in the whole dataset.",
    "action_frozen": "The action stays identical for seconds while the episode is still running.",
    "action_jump": "The action changes between two frames far more than its usual step.",
    "cannot_open": "A required file or field is missing or unreadable, so the dataset cannot be loaded.",
    "clipped_dimension": "A signal sits at its minimum or maximum for many frames.",
    "constant_dimension": "A dimension of the action or observations never changes.",
    "data_missing": "A data file that an episode points to does not exist.",
    "distribution_shift": "The signal's mean in the last quarter of the episodes is far from the first quarter's.",
    "duplicate_episode": "An episode repeats the action sequence of an earlier one.",
    "episode_count": "The episode total in info.json disagrees with the episode metadata.",
    "episode_index": "The episode indices do not run from 0 to n-1.",
    "episode_length_outlier": "An episode is far longer or shorter than the others.",
    "episode_rows_missing": "A data file has no rows for the episode that points to it.",
    "episode_short": "An episode is shorter than a second.",
    "feature_dtype": "A data column does not have the type that info.json declares.",
    "feature_missing": "A feature declared in info.json has no data column.",
    "frame_count": "The frame total in info.json disagrees with the episode lengths.",
    "idle_episode": "More than half of the frames repeat the previous action.",
    "index_mismatch": "The global index column does not match the episode frame ranges.",
    "nonstandard_features": "The dataset has no action or observation.state feature under the name policies expect.",
    "outlier_values": "A value is extremely far from the mean of its dimension.",
    "range_gap": "Episode frame ranges are not contiguous.",
    "stats_feature_missing": "The statistics file has no entry for a feature.",
    "stats_mismatch": "Stored statistics do not match the data.",
    "stats_zero_std": "A dimension has zero standard deviation in the statistics, so normalising divides by zero.",
    "video_frame_count": "A video has a different number of frames than its episode.",
    "video_frozen": "A video shows the same frame for many frames in a row (found by decoding).",
    "video_resolution": "A video's frame size differs from the shape in info.json.",
    "video_span_length": "An episode's span in a shared video file differs from the episode length.",
}
GITHUB_MARK = (
    '<svg width="22" height="22" viewBox="0 0 16 16" fill="currentColor" aria-hidden="true"><path d="M8 0C3.58 0 0 3.58 0 8c0 '
    "3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28"
    "-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87"
    ".31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82a7.6 7.6 0 0 1 4 0c1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08"
    " 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01"
    ' 8.01 0 0 0 16 8c0-4.42-3.58-8-8-8z"/></svg>'
)


def css():
    def block(p):
        return "".join(f"--{k}:{v};" for k, v in p.items())

    return (
        f":root{{{block(THEMES['light'])}}}@media(prefers-color-scheme:dark){{:root{{{block(THEMES['dark'])}}}}}"
        "*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--text);font:14px/1.5 -apple-system,"
        "BlinkMacSystemFont,'Segoe UI','Noto Sans',Helvetica,Arial,sans-serif}a{color:inherit;text-decoration:none}"
        "a:hover{text-decoration:underline}a:focus-visible,button:focus-visible,input:focus-visible,summary:focus-visible"
        "{outline:2px solid var(--link);outline-offset:2px}.mono{font:12px ui-monospace,SFMono-Regular,Menlo,Consolas,monospace}"
        ".top{display:flex;align-items:center;gap:20px;padding:16px 32px;border-bottom:1px solid var(--line)}"
        ".top .brand{display:flex;align-items:center;gap:10px;font-size:16px;font-weight:600}.top img{width:32px;height:32px;border-radius:50%;display:block}"
        ".top .nav{color:var(--muted);display:flex}.grow{flex-grow:1}"
        ".page{max-width:880px;margin:0 auto;padding:40px 32px 64px}.page.ds{padding-top:32px}"
        ".muted{color:var(--muted)}.link{color:var(--link)}"
        ".clean{color:var(--ok)}.warnings,.warning{color:var(--warn)}.errors,.error{color:var(--err)}"
        ".hero{display:flex;flex-wrap:wrap;gap:24px 32px;align-items:center;margin-bottom:40px}"
        ".hero img{display:block;flex:0 0 260px;width:260px;height:auto;aspect-ratio:2/1;border-radius:8px}.hero>div{flex:1 1 320px;min-width:0}"
        ".cmd{display:flex;gap:12px;padding:8px 14px;border:1px solid var(--line);border-radius:6px;font-size:13px;white-space:nowrap;overflow-x:auto}"
        ".search{position:relative;margin-bottom:16px}.search svg{position:absolute;left:12px;top:10px;color:var(--muted)}"
        ".search input{width:100%;height:36px;padding:0 12px 0 36px;font:inherit;color:var(--text);background:var(--bg);border:1px solid var(--line);border-radius:6px}"
        ".sr{position:absolute;width:1px;height:1px;overflow:hidden;clip:rect(0 0 0 0)}"
        ".filters{display:flex;flex-wrap:wrap;align-items:baseline;gap:4px 20px;padding-bottom:12px;border-bottom:1px solid var(--line)}"
        ".filters button{font:inherit;background:none;border:0;padding:0;color:var(--muted);cursor:pointer}.filters button[aria-pressed=true]{color:var(--text);font-weight:600}"
        ".filters .median{font-size:12px;color:var(--muted)}.js-only{display:none}.js .js-only{display:inline}"
        ".row{display:flex;align-items:center;gap:16px;padding:14px 0;border-bottom:1px solid var(--line)}.row[hidden]{display:none}"
        ".row .id{flex:1 1 auto;min-width:0}.row .id span{display:block}.row .name{font-weight:600;overflow-wrap:anywhere}.meta{font-size:12px;color:var(--muted)}"
        ".dot{flex:0 0 8px;width:8px;height:8px;border-radius:50%;background:currentColor}"
        ".row .found{flex:0 0 96px;text-align:right;font-size:13px}.row .time{flex:0 0 130px;text-align:right;font-size:13px;color:var(--muted)}"
        ".empty{padding:24px 0;color:var(--muted)}.fine{margin:32px 0 0;font-size:12px;color:var(--muted);max-width:640px}"
        ".mono.muted{overflow-wrap:anywhere}h1{margin:12px 0 4px;font-size:26px;font-weight:600;overflow-wrap:anywhere}.back{font-size:13px;color:var(--muted)}"
        ".strip{display:flex;flex-wrap:wrap;gap:4px 24px;align-items:baseline;margin-top:24px;padding:16px 0;border-top:1px solid var(--line);border-bottom:1px solid var(--line)}"
        ".strip .big{font-size:20px;font-weight:600}.note{margin:12px 0 32px;font-size:13px;color:var(--muted)}"
        ".group{padding:16px 0;border-top:1px solid var(--line)}.ghead{display:flex;flex-wrap:wrap;align-items:baseline;gap:4px 12px}.ghead .dot{align-self:center}"
        ".ghead .check{font-size:12px}.what{margin:2px 0 8px 20px;font-size:13px;color:var(--muted)}"
        ".find{margin-left:20px;font:12px ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;overflow-wrap:anywhere}.find .muted{margin-right:8px}"
        "details.more{margin:4px 0 0 20px;font-size:12px;color:var(--muted)}details.more summary{cursor:pointer;width:max-content}details.more .find{margin:2px 0 0}"
        ".sub{margin:32px 0 8px;font-size:14px;font-weight:600}"
        ".repro{margin-top:16px;padding-top:24px;border-top:1px solid var(--line)}.repro .meta{margin-top:12px}"
        "pre{margin:0;padding:10px 14px;border:1px solid var(--line);border-radius:6px;overflow-x:auto;font:12px/1.7 ui-monospace,SFMono-Regular,Menlo,Consolas,monospace}"
        "@media(max-width:640px){.top,.page{padding-left:16px;padding-right:16px}.hero img{flex-basis:100%;width:100%}"
        ".row{flex-wrap:wrap;gap:2px 12px}.row .id{flex-basis:calc(100% - 20px)}.row .found,.row .time{flex:0 0 auto;text-align:left;margin-left:20px}.row .found{flex-basis:80px}}"
    )


def e(text):
    return html.escape(str(text), quote=True)


def status(rec):
    return "errors" if rec["errors"] else "warnings" if rec["warnings"] else "clean"


def fmt_ms(ms):
    if ms >= 1000:
        return f"{ms / 1000:.2f} s"
    return f"{ms:.1f} ms" if ms < 10 else f"{round(ms)} ms"


def group_issues(issues):
    """Issues by code: errors before warnings, then the larger group first; each keeps the first CAP findings."""
    by_code = {}
    for i in issues:
        by_code.setdefault(i["code"], []).append(i)
    groups = [
        {"code": c, "severity": items[0]["severity"], "count": len(items), "shown": items[:CAP], "more": max(0, len(items) - CAP)}
        for c, items in by_code.items()
    ]
    return sorted(groups, key=lambda g: (g["severity"] != "error", -g["count"], g["code"]))


def report_url(rec):
    q = {"template": "wrong-finding.yml", "title": f"Wrong finding on {rec['repo']}",
         "dataset": f"{rec['repo']} @ {rec['revision'][:7]}", "lecop_version": rec["run"]["lecop_version"]}
    return REPO_URL + "/issues/new?" + urlencode(q)


def head(title, depth, description):
    up = "../" * depth
    return (
        f'<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">'
        f"<title>{e(title)}</title><meta name=\"description\" content=\"{e(description)}\">"
        f'<meta property="og:title" content="{e(title)}"><meta property="og:description" content="{e(description)}">'
        f'<meta property="og:image" content="{up}assets/social-preview.png"><link rel="icon" href="{up}assets/logo.png">'
        f"<style>{css()}</style></head><body>"
        f'<header class="top"><a class="brand" href="{up}index.html"><img src="{up}assets/logo.png" alt="" width="32" height="32">leCop</a>'
        f'<span class="grow"></span><a class="nav" href="{DOCS_URL}">Docs</a><a class="nav" href="{REPO_URL}" aria-label="leCop on GitHub">{GITHUB_MARK}</a></header>'
    )


def plural(n, word):
    return f"{n} {word}" + ("" if n == 1 else "s")


def row(rec):
    s, d, r = status(rec), rec["dataset"], rec["run"]
    codes = sorted({i["code"] for i in rec["issues"]})
    label = {"errors": plural(rec["errors"], "error"), "warnings": plural(rec["warnings"], "warning"), "clean": "Clean"}[s]
    search = " ".join([rec["repo"], rec["name"], *codes]).lower()
    meta = f"{d['format']} &middot; {d['episodes']:,} episodes &middot; {d['frames']:,} frames"
    return (
        f'<a class="row" href="datasets/{e(rec["name"])}.html" data-search="{e(search)}" data-status="{s}">'
        f'<span class="dot {s}" aria-hidden="true"></span>'
        f'<span class="id"><span class="name">{e(rec["repo"])}</span><span class="meta">{meta}</span></span>'
        f'<span class="found {s}">{label}</span><span class="time">Scanned in {fmt_ms(r["wall_ms"])}</span></a>'
    )


SCRIPT = """
document.documentElement.className+=' js';
const rows=[...document.querySelectorAll('.row')],q=document.getElementById('q'),tabs=[...document.querySelectorAll('.filters button')],empty=document.getElementById('empty');
const p=new URLSearchParams(location.search);let status=p.get('status')||'all';q.value=p.get('q')||'';
function apply(){const t=q.value.trim().toLowerCase();let shown=0;
 rows.forEach(r=>{const ok=(status==='all'||r.dataset.status===status)&&r.dataset.search.includes(t);r.hidden=!ok;shown+=ok;});
 empty.hidden=shown>0;
 tabs.forEach(b=>b.setAttribute('aria-pressed',b.dataset.status===status));
 const n=new URLSearchParams();if(t)n.set('q',q.value.trim());if(status!=='all')n.set('status',status);
 history.replaceState(null,'',n.toString()?'?'+n:location.pathname);}
q.addEventListener('input',apply);
tabs.forEach(b=>b.addEventListener('click',()=>{status=b.dataset.status;apply()}));
apply();
"""


def home_page(records):
    times = sorted(r["run"]["wall_ms"] for r in records)
    rows = "".join(row(r) for r in sorted(records, key=lambda r: r["repo"].lower()))
    n = len(records)
    tab = lambda key, text, on=False: f'<button type="button" class="js-only" data-status="{key}" aria-pressed="{str(on).lower()}">{text}</button>'
    lens = '<svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor" aria-hidden="true"><path d="M10.68 11.74a6 6 0 0 1-7.922-8.982 6 6 0 0 1 8.982 7.922l3.04 3.04a.749.749 0 0 1-.326 1.275.749.749 0 0 1-.734-.215ZM11.5 7a4.499 4.499 0 1 0-8.997 0A4.499 4.499 0 0 0 11.5 7Z"/></svg>'
    desc = f"What leCop reported on {n} LeRobot datasets, and how fast it scanned them."
    return (
        head("leCop results", 0, desc)
        + '<main class="page"><div class="hero"><img src="assets/social-preview.png" alt="leCop: the police of your robot dataset" width="1280" height="640">'
        '<div><div class="muted" style="margin-bottom:8px">Run it on your own dataset</div>'
        '<div class="cmd mono"><span class="muted">$</span><span>uvx lecop path/to/dataset</span></div>'
        '<div class="meta" style="margin-top:8px">or <code class="mono">pip install lecop</code>, <code class="mono">cargo binstall lecop</code></div></div></div>'
        f'<div class="search"><label for="q" class="sr">Search datasets</label>{lens}'
        f'<input id="q" type="search" placeholder="Search {n} datasets"></div>'
        f'<div class="filters">{tab("all", "All", True)}{tab("clean", "Clean")}{tab("warnings", "Warnings")}{tab("errors", "Errors")}'
        f'<span class="grow"></span><span class="median">Median scan {fmt_ms(times[len(times) // 2])}</span></div>'
        f'<div id="list">{rows}<div id="empty" class="empty" hidden>No dataset matches.</div></div>'
        '<p class="fine">Each result is what leCop reported at a pinned revision, not a verdict, and can be reproduced with one command. '
        'Scan times are the median of warm-cache runs on the machine named on each dataset page. '
        'leCop is unofficial and not affiliated with Hugging Face or LeRobot. '
        f'<a class="link" href="{REPO_URL}/issues/new?template=wrong-finding.yml">Report a wrong finding</a></p>'
        f"</main><script>{SCRIPT}</script></body></html>"
    )


def finding(i):
    where = "dataset" if i["episode"] is None else f"episode {i['episode']}"
    return f'<div class="find"><span class="muted">{where}</span>{e(i["message"])}</div>'


def group_html(g):
    first, rest = g["shown"][0], g["shown"][1:]
    desc = DESCRIPTIONS.get(g["code"])
    what = f'<div class="what">{e(desc)}</div>' if desc else ""
    more = ""
    if g["count"] > 1:
        tail = f'<div class="find muted">+ {g["more"]} not shown, run lecop on the dataset to see them all</div>' if g["more"] else ""
        more = f'<details class="more"><summary>+ {g["count"] - 1} more</summary>{"".join(finding(i) for i in rest)}{tail}</details>'
    return (
        f'<section class="group"><div class="ghead"><span class="dot {g["severity"]}" aria-hidden="true"></span><strong class="mono" style="font-size:14px">{e(g["code"])}</strong>'
        f'<span class="muted">{g["count"]}</span><span class="grow"></span><a class="link check" href="{DOCS_URL}/checks.md">About this check</a></div>'
        f'{what}{finding(first)}{more}</section>'
    )


def decode_html(rec, decode):
    """The --decode run of the same revision: its time and counts, and the findings only decoding produced."""
    if decode is None:
        return ""
    seen = {i["code"] for i in rec["issues"]}
    extra = "".join(group_html(g) for g in group_issues([i for i in decode["issues"] if i["code"] not in seen]))
    return (
        f'<h2 class="sub">With --decode</h2><div class="muted" style="margin-bottom:8px">Scanned in {fmt_ms(decode["run"]["wall_ms"])} with frames decoded: '
        f'{plural(decode["errors"], "error")} and {plural(decode["warnings"], "warning")}.</div>{extra}'
    )


def dataset_page(rec, decode=None):
    d, r = rec["dataset"], rec["run"]
    body = "".join(group_html(g) for g in group_issues(rec["issues"])) or '<p class="muted">leCop reported nothing for this dataset at this revision.</p>'
    counts = ""
    if rec["errors"]:
        counts += f'<span class="error">{plural(rec["errors"], "error")}</span>'
    if rec["warnings"]:
        counts += f'<span class="warning">{plural(rec["warnings"], "warning")}</span>'
    if not rec["issues"]:
        counts = '<span class="clean">Clean</span>'
    repro = f"hf download {rec['repo']} \\\n  --repo-type dataset --revision {rec['revision']} \\\n  --local-dir {rec['name']}\nlecop validate {rec['name']}"
    details = (f"leCop {r['lecop_version']} ({r['lecop_commit']}) &middot; rustc {e(r['rustc'])} &middot; {e(r['machine'])}, {r['threads']} threads "
               f"&middot; {e(r['cache'])} cache, median of {r['runs']} runs &middot; {e(r['date'])}")
    desc = f"leCop reported {rec['errors']} errors and {rec['warnings']} warnings on {rec['repo']}, scanned in {fmt_ms(r['wall_ms'])}."
    hub = f"https://huggingface.co/datasets/{rec['repo']}/tree/{rec['revision']}"
    return (
        head(f"{rec['repo']}: leCop results", 1, desc)
        + f'<main class="page ds"><a class="back" href="../index.html">&larr; All datasets</a><h1>{e(rec["repo"])}</h1>'
        f'<div class="mono muted"><a class="link" href="{e(hub)}">huggingface.co/datasets/{e(rec["repo"])}</a> @ {e(rec["revision"][:7])}</div>'
        f'<div class="strip"><span class="big">Scanned in {fmt_ms(r["wall_ms"])}</span><span class="muted">{d["episodes"]:,} episodes &middot; {d["frames"]:,} frames</span>'
        f'<span class="grow"></span>{counts}</div>'
        f'<p class="note">What leCop reported at this revision, not a verdict. <a class="link" href="{e(report_url(rec))}">Report a wrong finding</a></p>'
        f'{body}{decode_html(rec, decode)}'
        f'<div class="repro"><div class="muted" style="font-size:13px;margin-bottom:8px">Reproduce</div><pre>{e(repro)}</pre><div class="meta">{details}</div></div>'
        "</main></body></html>"
    )


def index_entry(rec):
    codes = {}
    for i in rec["issues"]:
        codes[i["code"]] = codes.get(i["code"], 0) + 1
    return {"name": rec["name"], "repo": rec["repo"], "revision": rec["revision"], "format": rec["dataset"]["format"],
            "episodes": rec["dataset"]["episodes"], "frames": rec["dataset"]["frames"], "wall_ms": rec["run"]["wall_ms"],
            "errors": rec["errors"], "warnings": rec["warnings"], "status": status(rec), "codes": codes}


def build(runs=RUNS, dist=DIST):
    records, decodes = [], {}
    for name in sorted(os.listdir(runs)):
        with open(os.path.join(runs, name)) as f:
            rec = json.load(f)
        if rec["run"].get("decode"):
            decodes[rec["name"]] = rec
        else:
            records.append(rec)
    records.sort(key=lambda r: r["name"])
    shutil.rmtree(dist, ignore_errors=True)
    os.makedirs(os.path.join(dist, "datasets"))
    os.makedirs(os.path.join(dist, "assets"))
    for asset in ("logo.png", "social-preview.png"):
        shutil.copy(os.path.join(ROOT, "assets", asset), os.path.join(dist, "assets", asset))
    with open(os.path.join(dist, "index.html"), "w") as f:
        f.write(home_page(records))
    with open(os.path.join(dist, "index.json"), "w") as f:
        json.dump([index_entry(r) for r in records], f, indent=1)
    for r in records:
        with open(os.path.join(dist, "datasets", r["name"] + ".html"), "w") as f:
            f.write(dataset_page(r, decodes.get(r["name"])))
    return len(records)


if __name__ == "__main__":
    print(f"{build()} datasets -> {os.path.relpath(DIST, ROOT)}")
    sys.exit(0)
