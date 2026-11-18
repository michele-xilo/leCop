#!/usr/bin/env python3
"""Build the results site from site/data/runs/*.json into site/dist/ (standard library only).

    scripts/build_site.py            writes site/dist/index.html, index.json, datasets/<name>.html, assets/

Every page is plain HTML with inline CSS. The home page lists every record and the dataset pages list every
finding without JavaScript; a small script adds search, status filter and sort, with the state in the URL.
"""

import html
import json
import math
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
    "light": dict(bg="#ffffff", subtle="#f6f8fa", line="#d1d9e0", text="#1f2328", muted="#59636e", link="#0969da",
                  ok="#1a7f37", warn="#9a6700", err="#cf222e", warnbg="#fff8c5"),
    "dark": dict(bg="#0d1117", subtle="#151b23", line="#3d444d", text="#f0f6fc", muted="#9198a1", link="#4493f8",
                 ok="#3fb950", warn="#d29922", err="#f85149", warnbg="#2a2208"),
}
GITHUB_MARK = (
    '<svg width="24" height="24" viewBox="0 0 16 16" fill="currentColor" aria-hidden="true"><path d="M8 0C3.58 0 0 3.58 0 8c0 '
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
        "a:hover{text-decoration:underline}a:focus-visible,button:focus-visible,input:focus-visible,select:focus-visible"
        "{outline:2px solid var(--link);outline-offset:2px}code,.mono{font:12px ui-monospace,SFMono-Regular,Menlo,Consolas,monospace}"
        ".top{display:flex;align-items:center;gap:16px;padding:16px 32px;background:var(--subtle);border-bottom:1px solid var(--line)}"
        ".top .brand{display:flex;align-items:center;gap:10px;font-size:16px;font-weight:600}.top img{width:32px;height:32px;border-radius:50%}"
        ".top .sep,.top .muted{color:var(--muted)}.grow{flex-grow:1}"
        ".page{max-width:1280px;margin:0 auto;padding:24px 32px 48px;display:flex;flex-wrap:wrap;gap:24px;align-items:flex-start}"
        ".main{flex:1 1 640px;min-width:0}.side{flex:0 0 296px;max-width:100%;min-width:0}"
        ".muted{color:var(--muted)}.link{color:var(--link)}.box{border:1px solid var(--line);border-radius:6px;overflow:hidden}"
        ".boxhead{display:flex;flex-wrap:wrap;align-items:center;gap:4px 16px;padding:8px 16px;background:var(--subtle);min-height:48px}"
        ".label{display:inline-block;padding:0 8px;border:1px solid currentColor;border-radius:2em;font-size:12px;font-weight:500;line-height:18px}"
        ".count{font-size:12px;padding:0 6px;border-radius:2em;background:var(--line);color:var(--text)}"
        ".clean{color:var(--ok)}.warnings,.warning{color:var(--warn)}.errors,.error{color:var(--err)}.scheduled{color:var(--muted)}"
        ".search{position:relative;margin-bottom:16px}.search svg{position:absolute;left:10px;top:8px;color:var(--muted)}"
        ".search input{width:100%;height:32px;padding:0 12px 0 34px;font:inherit;color:var(--text);background:var(--bg);border:1px solid var(--line);border-radius:6px}"
        ".tabs button,.tabs select{font:inherit;background:none;border:0;color:var(--muted);cursor:pointer;padding:0}"
        ".tabs button[aria-pressed=true]{color:var(--text);font-weight:600}.tabs select{color:var(--text);border:1px solid var(--line);border-radius:6px;padding:2px 6px;background:var(--bg)}"
        ".js-only{display:none}.js .js-only{display:inline}"
        ".row{display:grid;grid-template-columns:20px minmax(0,3fr) minmax(0,2.4fr) 170px;gap:12px;align-items:center;padding:10px 16px;border-top:1px solid var(--line)}"
        ".row:hover{background:var(--subtle);text-decoration:none}.row[hidden]{display:none}"
        ".dot{width:16px;height:16px;border-radius:50%;border:1.5px solid currentColor;font:700 10px/13px sans-serif;text-align:center}"
        ".name{font-weight:600;overflow-wrap:anywhere}.meta{font-size:12px;color:var(--muted)}"
        ".codes{color:var(--muted);overflow:hidden;text-overflow:ellipsis;white-space:nowrap}"
        ".time{display:flex;align-items:center;justify-content:flex-end;gap:10px}.bar{width:70px;height:6px;border-radius:3px;background:var(--line)}"
        ".bar i{display:block;height:6px;border-radius:3px;background:var(--muted)}.time .mono{width:56px;text-align:right}"
        ".empty{padding:24px 16px;color:var(--muted);border-top:1px solid var(--line)}"
        ".side h2{font-size:16px;margin:0 0 12px}.side h3{font-size:14px;margin:0 0 8px}.section{margin-top:16px;padding-top:16px;border-top:1px solid var(--line)}"
        ".side img.banner{display:block;width:100%;height:auto;aspect-ratio:2/1;border-radius:6px;border:1px solid var(--line)}"
        ".cmd{display:flex;align-items:center;gap:8px;padding:6px 6px 6px 12px;border:1px solid var(--line);border-radius:6px;background:var(--subtle);overflow-x:auto;white-space:nowrap}"
        ".cmd button,.btn{font:500 12px/26px inherit;height:28px;padding:0 12px;border-radius:6px;border:1px solid var(--line);background:var(--subtle);color:var(--text);cursor:pointer;white-space:nowrap}"
        ".stack{display:flex;height:8px;border-radius:4px;overflow:hidden;gap:2px}.stack i{display:block}"
        ".legend{display:flex;flex-wrap:wrap;gap:4px 14px;margin-top:8px;font-size:12px}"
        ".notice{display:flex;flex-wrap:wrap;gap:8px 16px;align-items:center;margin:16px 0 24px;padding:12px 16px;border:1px solid var(--warn);background:var(--warnbg);border-radius:6px}"
        ".notice div{flex:1 1 480px;min-width:0}"
        "h1{margin:0;font-size:20px;font-weight:400}h1 strong{font-weight:600;overflow-wrap:anywhere}"
        ".group{margin-bottom:16px}.group .boxhead{border-bottom:1px solid var(--line);padding:8px 16px}"
        ".find{display:grid;grid-template-columns:90px minmax(0,1fr);gap:12px;padding:6px 16px}.find .mono:last-child{overflow-wrap:anywhere}"
        ".facts{display:grid;grid-template-columns:auto minmax(0,1fr);gap:4px 16px}"
        "pre{margin:0;padding:8px 12px;border:1px solid var(--line);border-radius:6px;background:var(--subtle);overflow-x:auto}"
        ".big{font-size:32px;font-weight:600;line-height:1.2}"
        "@media(max-width:720px){.row{grid-template-columns:20px minmax(0,1fr) 120px}.row .codes{display:none}.top,.page{padding-left:16px;padding-right:16px}}"
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


def report_url(rec, code=None):
    q = {"template": "wrong-finding.yml", "title": f"Wrong finding on {rec['repo']}",
         "dataset": f"{rec['repo']} @ {rec['revision'][:7]}", "lecop_version": rec["run"]["lecop_version"]}
    if code:
        q["code"] = code
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
        f'<span class="sep">/</span><a href="{up}index.html" style="font-size:16px">results</a><span class="grow"></span>'
        f'<a class="muted" href="{DOCS_URL}">Docs</a><a href="{REPO_URL}" aria-label="leCop on GitHub">{GITHUB_MARK}</a></header>'
    )


def row(rec, lo, hi):
    s, d, r = status(rec), rec["dataset"], rec["run"]
    codes = {}
    for i in rec["issues"]:
        codes[i["code"]] = codes.get(i["code"], 0) + 1
    code_text = ", ".join(c if n == 1 else f"{c} x{n}" for c, n in codes.items())
    label = {"errors": f"{rec['errors']} errors", "warnings": f"{rec['warnings']} warning" + ("s" if rec["warnings"] != 1 else ""), "clean": "clean"}[s]
    glyph = {"errors": "&#10005;", "warnings": "!", "clean": "&#10003;"}[s]
    bar = 8 + 92 * (math.log10(r["wall_ms"]) - lo) / (hi - lo) if hi > lo else 100
    search = " ".join([rec["repo"], rec["name"], *codes]).lower()
    meta = f"{d['format']} &middot; {d['episodes']:,} episodes &middot; {d['frames']:,} frames"
    return (
        f'<a class="row" href="datasets/{e(rec["name"])}.html" data-search="{e(search)}" data-status="{s}" data-ms="{r["wall_ms"]}" '
        f'data-name="{e(rec["repo"].lower())}" data-findings="{rec["errors"] + rec["warnings"]}">'
        f'<span class="dot {s}" aria-hidden="true">{glyph}</span>'
        f'<span><span class="name">{e(rec["repo"])}</span> <span class="label {s}">{label}</span><br><span class="meta">{meta}</span></span>'
        f'<span class="codes mono">{e(code_text)}</span>'
        f'<span class="time"><span class="bar" aria-hidden="true"><i style="width:{bar:.0f}%"></i></span><span class="mono">{fmt_ms(r["wall_ms"])}</span></span></a>'
    )


SCRIPT = """
document.documentElement.className+=' js';
const rows=[...document.querySelectorAll('.row')],q=document.getElementById('q'),sel=document.getElementById('sort'),list=document.getElementById('list'),
 tabs=[...document.querySelectorAll('.tabs button')],empty=document.getElementById('empty');
const p=new URLSearchParams(location.search);let status=p.get('status')||'all';q.value=p.get('q')||'';sel.value=p.get('sort')||'name';
const key={name:r=>r.dataset.name,time:r=>+r.dataset.ms,findings:r=>-r.dataset.findings};
function apply(){const t=q.value.trim().toLowerCase();let shown=0;
 rows.forEach(r=>{const ok=(status==='all'||r.dataset.status===status)&&r.dataset.search.includes(t);r.hidden=!ok;shown+=ok;});
 [...rows].sort((a,b)=>{const x=key[sel.value](a),y=key[sel.value](b);return x<y?-1:x>y?1:0}).forEach(r=>list.appendChild(r));
 list.appendChild(empty);empty.hidden=shown>0;
 tabs.forEach(b=>b.setAttribute('aria-pressed',b.dataset.status===status));
 const n=new URLSearchParams();if(t)n.set('q',q.value.trim());if(status!=='all')n.set('status',status);if(sel.value!=='name')n.set('sort',sel.value);
 history.replaceState(null,'',n.toString()?'?'+n:location.pathname);}
q.addEventListener('input',apply);sel.addEventListener('change',apply);
tabs.forEach(b=>b.addEventListener('click',()=>{status=b.dataset.status;apply()}));
document.getElementById('copy').addEventListener('click',e=>{navigator.clipboard.writeText('uvx lecop path/to/dataset');e.target.textContent='Copied'});
apply();
"""


def home_page(records):
    counts = {s: sum(status(r) == s for r in records) for s in ("clean", "warnings", "errors")}
    times = [r["run"]["wall_ms"] for r in records]
    lo, hi = math.log10(min(times) * 0.8), math.log10(max(times))
    rows = "".join(row(r, lo, hi) for r in sorted(records, key=lambda r: r["repo"].lower()))
    n = len(records)
    median = sorted(times)[len(times) // 2]
    tab = lambda key, text, c, on=False: f'<button type="button" class="js-only" data-status="{key}" aria-pressed="{str(on).lower()}">{text} <span class="count">{c}</span></button>'
    lens = '<svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor" aria-hidden="true"><path d="M10.68 11.74a6 6 0 0 1-7.922-8.982 6 6 0 0 1 8.982 7.922l3.04 3.04a.749.749 0 0 1-.326 1.275.749.749 0 0 1-.734-.215ZM11.5 7a4.499 4.499 0 1 0-8.997 0A4.499 4.499 0 0 0 11.5 7Z"/></svg>'
    legend = "".join(f'<span><span class="{s}" aria-hidden="true">&#9679;</span> <strong>{t}</strong> <span class="muted">{counts[s]}</span></span>' for s, t in (("clean", "Clean"), ("warnings", "Warnings"), ("errors", "Errors")))
    stack = "".join(f'<i class="{s}" style="flex:{counts[s]} 1 0;background:currentColor"></i>' for s in counts if counts[s])
    totals = f"{sum(r['dataset']['episodes'] for r in records):,} episodes, {sum(r['dataset']['frames'] for r in records) / 1e6:.2f}M frames, {sum(r['errors'] for r in records)} errors and {sum(r['warnings'] for r in records)} warnings. Median wall time {fmt_ms(median)}."
    desc = f"What leCop reported on {n} LeRobot datasets, and how fast."
    return (
        head("leCop results", 0, desc)
        + f'<div class="page"><main class="main"><div class="search"><label for="q" class="muted" style="position:absolute;width:1px;height:1px;overflow:hidden">Search datasets</label>{lens}'
        '<input id="q" type="search" placeholder="Search datasets, Hub repos or issue codes"></div>'
        f'<div class="box"><div class="boxhead tabs">{tab("all", "All", n, True)}{tab("clean", "Clean", counts["clean"])}{tab("warnings", "Warnings", counts["warnings"])}{tab("errors", "Errors", counts["errors"])}'
        '<span class="grow"></span><label class="js-only muted" for="sort">Sort </label><select id="sort" class="js-only"><option value="name">Name</option><option value="time">Wall time</option><option value="findings">Most findings</option></select></div>'
        f'<div id="list">{rows}<div id="empty" class="empty" hidden>No dataset matches.</div></div></div>'
        '<p class="meta" style="margin-top:12px">Wall time is the median of warm-cache runs of <code>lecop validate</code> on the machine named on each dataset page; the bar is log scale.</p></main>'
        f'<aside class="side"><h2>About</h2><img class="banner" src="assets/social-preview.png" alt="leCop: the police of your robot dataset" width="1280" height="640">'
        f'<p>Fast health checker for LeRobot datasets. One binary, no setup.</p><div style="display:flex;flex-direction:column;gap:6px">'
        f'<a class="link" href="{REPO_URL}">github.com/michele-xilo/leCop</a><a class="link" href="{DOCS_URL}">Documentation</a><a class="link" href="{REPO_URL}/releases">Releases</a></div>'
        '<div class="section"><h3>Run it on your dataset</h3><div class="cmd mono"><span class="grow">uvx lecop path/to/dataset</span><button type="button" id="copy" class="js-only">Copy</button></div>'
        '<div class="meta" style="margin-top:8px">Or <code>pip install lecop</code>, <code>cargo binstall lecop</code>.</div></div>'
        f'<div class="section"><h3>Results</h3><div class="stack">{stack}</div><div class="legend">{legend}</div><div class="meta" style="margin-top:10px">{totals}</div></div>'
        f'<div class="section meta">Unofficial; not affiliated with Hugging Face or LeRobot. Findings are what the checker reported at a pinned revision and may include false positives. '
        f'<a class="link" href="{REPO_URL}/issues/new?template=wrong-finding.yml">Report a wrong finding</a>.</div></aside></div><script>{SCRIPT}</script></body></html>'
    )


def finding(i):
    where = "dataset" if i["episode"] is None else f"episode {i['episode']}"
    return f'<div class="find mono"><span class="muted">{where}</span><span>{e(i["message"])}</span></div>'


def group_html(rec, g):
    shown = "".join(finding(i) for i in g["shown"])
    more = f'<div class="meta" style="padding:0 16px 8px">+ {g["more"]} more, run lecop on the dataset to see them all</div>' if g["more"] else ""
    return (
        f'<section class="box group"><div class="boxhead"><span class="{g["severity"]}" aria-hidden="true">&#9679;</span><strong class="mono">{e(g["code"])}</strong>'
        f'<span class="label {g["severity"]}">{g["severity"]}</span><span class="grow"></span><a class="link meta" href="{DOCS_URL}/checks.md">About this check</a>'
        f'<a class="link meta" href="{e(report_url(rec, g["code"]))}">Report as wrong</a><span class="count">{g["count"]}</span></div>{shown}{more}</section>'
    )


def decode_html(rec, decode):
    """The --decode run of the same revision: its time and counts, and the findings only decoding produced."""
    if decode is None:
        return "", ""
    seen = {i["code"] for i in rec["issues"]}
    extra = "".join(group_html(decode, g) for g in group_issues([i for i in decode["issues"] if i["code"] not in seen]))
    side = (
        f'<div class="section"><h3 class="muted">With --decode</h3><div class="big">{fmt_ms(decode["run"]["wall_ms"])}</div>'
        f'<div class="meta">{decode["errors"]} errors and {decode["warnings"]} warnings with frames decoded.</div></div>'
    )
    main = f'<h2 style="margin:24px 0 8px">Only with --decode</h2>{extra}' if extra else ""
    return side, main


def dataset_page(rec, decode=None):
    s, d, r = status(rec), rec["dataset"], rec["run"]
    decode_side, decode_main = decode_html(rec, decode)
    groups = group_issues(rec["issues"])
    labels = ""
    if rec["errors"]:
        labels += f'<span class="label errors">{rec["errors"]} errors</span> '
    if rec["warnings"]:
        labels += f'<span class="label warnings">{rec["warnings"]} warnings</span> '
    if not rec["issues"]:
        labels = '<span class="label clean">clean</span>'
    stack = "".join(f'<i class="{g["severity"]}" style="flex:{g["count"]} 1 0;background:currentColor" title="{e(g["code"])}"></i>' for g in groups)
    legend = "".join(f'<span><span class="{g["severity"]}" aria-hidden="true">&#9679;</span> <strong>{e(g["code"])}</strong> <span class="muted">{g["count"]}</span></span>' for g in groups)
    body = "".join(group_html(rec, g) for g in groups) or '<p class="muted">leCop reported nothing for this dataset at this revision.</p>'
    facts = [("leCop", f"{r['lecop_version']} ({r['lecop_commit']})"), ("Rust", r["rustc"]), ("Machine", r["machine"]), ("Threads", r["threads"]), ("OS", r["os"]),
             ("Cache", r["cache"]), ("Runs", f"median of {r['runs']}"), ("Date", r["date"]), ("Format", d["format"]), ("Frame rate", f"{d['fps']:g} fps"), ("Robot", d["robot"])]
    facts_html = "".join(f'<span class="muted">{k}</span><span>{e(v)}</span>' for k, v in facts)
    repro = f"hf download {rec['repo']} --repo-type dataset \\\n  --revision {rec['revision']} \\\n  --local-dir {rec['name']}\nlecop validate --format json {rec['name']}"
    desc = f"leCop reported {rec['errors']} errors and {rec['warnings']} warnings on {rec['repo']} in {fmt_ms(r['wall_ms'])}."
    hub = f"https://huggingface.co/datasets/{rec['repo']}/tree/{rec['revision']}"
    return (
        head(f"{rec['repo']}: leCop results", 1, desc)
        + f'<div class="page" style="display:block"><div style="display:flex;flex-wrap:wrap;align-items:center;gap:8px 12px"><h1><a class="link" href="../index.html">results</a> <span class="muted">/</span> <strong>{e(rec["repo"])}</strong></h1>{labels}</div>'
        f'<div class="mono muted" style="margin-top:6px"><a class="link" href="{e(hub)}">huggingface.co/datasets/{e(rec["repo"])}</a> @ {e(rec["revision"][:7])}</div>'
        f'<div class="notice"><div><strong>leCop reported these. It is a checker\'s report, not a verdict.</strong> Each finding was produced from the files at this revision and can be reproduced with the command on the right. If one is wrong, tell us and it gets fixed or removed.</div>'
        f'<a class="btn" href="{e(report_url(rec))}">Report a wrong finding</a></div>'
        f'<div style="display:flex;flex-wrap:wrap;gap:24px;align-items:flex-start"><div class="main">'
        f'<div class="stack" style="margin-bottom:8px">{stack}</div><div class="legend" style="margin:0 0 16px">{legend}</div>{body}{decode_main}</div>'
        f'<aside class="side"><div class="section" style="margin:0;padding:0;border:0"><h3 class="muted">Wall time</h3><div class="big">{fmt_ms(r["wall_ms"])}</div>'
        f'<div class="meta">{d["episodes"]:,} episodes, {d["frames"]:,} frames declared.</div></div>{decode_side}'
        f'<div class="section"><h3 class="muted">Run</h3><div class="facts mono">{facts_html}</div></div>'
        f'<div class="section"><h3 class="muted">Reproduce</h3><pre class="mono">{e(repro)}</pre></div>'
        '<div class="section"><h3 class="muted">Review status</h3><span class="label scheduled">unreviewed</span>'
        '<div class="meta" style="margin-top:8px">Findings are marked confirmed or false positive once a person has checked them against the data. None checked yet.</div></div>'
        "</aside></div></div></body></html>"
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
