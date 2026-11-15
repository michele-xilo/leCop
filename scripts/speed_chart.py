#!/usr/bin/env python3
"""Draw the speed tables of docs/comparison.md as SVG dot plots in assets/ (standard library only).

The tables stay the source of truth: paste the new rows from scripts/compare.py into the doc, then run
scripts/speed_chart.py to redraw assets/speed-novideo.svg and assets/speed-video.svg.
"""

import math
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUTPUTS = ["speed-novideo.svg", "speed-video.svg"]

LEFT, RIGHT, TOP, ROW, PLOT_W = 340, 70, 46, 28, 420
STYLE = """
text{font:12px ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;fill:#24292f}
.muted{fill:#656d76}.grid{stroke:#d0d7de}.span{stroke:#d0d7de;stroke-width:2}
.lecop{fill:#b74604}.doctor{fill:#57606a}.physlint{fill:#8c959f}
@media(prefers-color-scheme:dark){
text{fill:#e6edf3}.muted{fill:#8d96a0}.grid{stroke:#30363d}.span{stroke:#30363d}
.lecop{fill:#e8651a}.doctor{fill:#9ea7b3}.physlint{fill:#6e7681}}
"""


def tables(markdown):
    """Each table with a Dataset column, as (tool names, [(dataset, [ms or None per tool], last cell)])."""
    found, rows = [], None
    for line in markdown.splitlines():
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        if line.startswith("| Dataset |"):
            rows = []
            found.append((cells[1:-1], rows))
        elif rows is not None and line.startswith("|"):
            if set(cells[0]) <= set("-: "):
                continue
            name = re.search(r"`([^`]+)`", cells[0]).group(1)
            digits = [re.sub(r"\D", "", c) for c in cells[1:-1]]
            rows.append((name, [int(d) if d else None for d in digits], cells[-1].split()[0]))
        else:
            rows = None
    return found


def mark(tool, x, y):
    if tool == "leCop":
        return f'<circle class="lecop" cx="{x:.1f}" cy="{y}" r="6"/>'
    if tool == "physlint":
        return f'<rect class="physlint" x="{x - 5:.1f}" y="{y - 5}" width="10" height="10" rx="2"/>'
    return f'<circle class="doctor" cx="{x:.1f}" cy="{y}" r="5"/>'


def tick_label(ms):
    return f"{ms} ms" if ms < 1000 else f"{ms // 1000} s"


def svg(tools, rows):
    values = [v for _, vs, _ in rows for v in vs if v]
    lo, hi = math.log10(min(values) * 0.6), math.log10(max(values) * 1.6)
    width, height = LEFT + PLOT_W + RIGHT, TOP + ROW * len(rows) + 30

    # Log axis: the tools are one to three orders of magnitude apart, a linear one would hide leCop at zero.
    def x(ms):
        return LEFT + (math.log10(ms) - lo) / (hi - lo) * PLOT_W

    out = [
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {height}" width="{width}" height="{height}" role="img">',
        f"<style>{STYLE}</style>",
    ]
    legend_x = LEFT
    for tool in tools:
        out.append(mark(tool, legend_x + 6, 14) + f'<text x="{legend_x + 18}" y="18">{tool}</text>')
        legend_x += 30 + 7.3 * len(tool)
    out.append(f'<text class="muted" x="{width - 4}" y="18" text-anchor="end">leCop lead</text>')
    bottom = TOP + ROW * len(rows) - 6
    for power in range(math.ceil(lo), math.floor(hi) + 1):
        tx = x(10**power)
        out.append(f'<line class="grid" x1="{tx:.1f}" x2="{tx:.1f}" y1="{TOP - 14}" y2="{bottom}"/>')
        out.append(f'<text class="muted" x="{tx:.1f}" y="{bottom + 18}" text-anchor="middle">{tick_label(10**power)}</text>')
    for i, (name, vs, lead) in enumerate(rows):
        y = TOP + ROW * i
        present = [(t, v) for t, v in zip(tools, vs) if v]
        xs = [x(v) for _, v in present]
        out.append(f'<text x="{LEFT - 88}" y="{y + 4}" text-anchor="end">{name}</text>')
        out.append(f'<line class="span" x1="{min(xs):.1f}" x2="{max(xs):.1f}" y1="{y}" y2="{y}"/>')
        # leCop drawn last so its mark is never hidden under another tool's.
        out += [mark(t, x(v), y) for t, v in reversed(present)]
        out.append(f'<text x="{LEFT - 12}" y="{y + 4}" text-anchor="end">{vs[0]:,} ms</text>')
        out.append(f'<text x="{width - 4}" y="{y + 4}" text-anchor="end">{lead}</text>')
    return "\n".join(out) + "\n</svg>\n"


def main():
    found = tables((ROOT / "docs" / "comparison.md").read_text())
    assert len(found) == len(OUTPUTS), f"expected {len(OUTPUTS)} speed tables, found {len(found)}"
    for name, (tools, rows) in zip(OUTPUTS, found):
        assert tools[0] == "leCop" and all(r[1][0] for r in rows), "leCop must be the first column, with a time"
        (ROOT / "assets" / name).write_text(svg(tools, rows))
        print(f"assets/{name}: {len(rows)} datasets, {', '.join(tools)}")


if __name__ == "__main__":
    main()
