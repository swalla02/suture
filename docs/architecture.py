"""Draws docs/architecture.svg: where Suture fits.

    python3 docs/architecture.py

Plain SVG with no CSS and no fonts embedded, so it renders the same everywhere.
"""

from pathlib import Path
from xml.sax.saxutils import escape

W, H = 1200, 760
INK, MUTED, LINE = "#1f2933", "#52606d", "#7b8794"
ORANGE, CREAM = "#d9822b", "#fff8ec"
GREY, GREY_LINE = "#f5f7fa", "#9aa5b1"
FONT = "font-family=\"-apple-system, 'Segoe UI', Helvetica, Arial, sans-serif\""

out: list[str] = []


def box(x, y, w, h, fill, stroke, width=1.5, r=10):
    out.append(f'<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="{r}" fill="{fill}" stroke="{stroke}" stroke-width="{width}"/>')


def text(x, y, s, size=15, weight="normal", color=INK, anchor="middle"):
    out.append(f'<text x="{x}" y="{y}" {FONT} font-size="{size}" font-weight="{weight}" fill="{color}" text-anchor="{anchor}">{escape(s)}</text>')


def lines(x, y, title, rest, size=14, gap=20):
    text(x, y, title, size=size + 2, weight="bold")
    for i, s in enumerate(rest):
        text(x, y + gap * (i + 1) + 4, s, size=size, color=MUTED)


def arrow(x1, y1, x2, y2, label=None, lx=None, ly=None, anchor="start"):
    out.append(f'<line x1="{x1}" y1="{y1}" x2="{x2}" y2="{y2}" stroke="{LINE}" stroke-width="2" marker-end="url(#head)"/>')
    if label:
        text(lx if lx is not None else (x1 + x2) / 2 + 10, ly if ly is not None else (y1 + y2) / 2 + 5, label, size=13, color=MUTED, anchor=anchor)


out.append(f'<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" viewBox="0 0 {W} {H}">')
out.append(f'<defs><marker id="head" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="8" markerHeight="8" orient="auto-start-reverse"><path d="M0,0 L10,5 L0,10 z" fill="{LINE}"/></marker></defs>')
out.append(f'<rect width="{W}" height="{H}" fill="#ffffff"/>')

# Infrastructure.
box(40, 24, W - 80, 76, "#eef2f7", "#616e7c")
text(W / 2, 56, "Infrastructure", size=17, weight="bold")
text(W / 2, 80, "Terraform provisions it · Kubernetes runs it · your cloud", color=MUTED)
arrow(W / 2, 100, W / 2, 148, "deploys and scales")

# Suture.
box(40, 150, W - 80, 300, CREAM, ORANGE, width=3, r=14)
text(W / 2, 186, "Suture: the business logic", size=21, weight="bold")
text(W / 2, 210, "what has to happen, in which order, under which rules, by whom, until which goal", color=MUTED)
inner = [
    ("program (.st)", ["what exists, what it needs,", "rules, goals, who may", "do what"]),
    ("compiler", ["read-backs for every rule,", "every open choice listed,", "everything checked"]),
    ("runtime", ["orders the steps, holds back", "what a rule forbids, retries,", "gives back, meets deadlines"]),
    ("journal", ["every input, answer,", "timer and model pick:", "replayable, explainable"]),
]
bw, by, bh, gap = 245, 240, 170, 33
for i, (title, rest) in enumerate(inner):
    x = 70 + i * (bw + gap)
    box(x, by, bw, bh, "#ffffff", "#e0a85a", r=8)
    lines(x + bw / 2, by + 52, title, rest)
    if i < 3:
        arrow(x + bw + 2, by + bh / 2, x + bw + gap - 2, by + bh / 2)

# What Suture talks to, each through a declared boundary.
bottom = [
    ("Outside services", ["databases · caches", "secret managers · Prometheus", "payment, mail, carrier providers"]),
    ("Decision models", ["served by vLLM, SGLang,", "or a hosted API such as Jev", "optional: `prefer` decides without"]),
    ("Callers", ["apps, staff,", "other services", "admitted by role"]),
]
cw, cy, ch, cgap = 356, 590, 140, 26
labels = [("effect · source", "adapters"), ("decide", "picks among allowed options"), ("admit", "inputs")]
for i, (title, rest) in enumerate(bottom):
    x = 40 + i * (cw + cgap)
    box(x, cy, cw, ch, GREY, GREY_LINE)
    lines(x + cw / 2, cy + 42, title, rest)
    mid = x + cw / 2
    if i < 2:
        arrow(mid, 452, mid, cy - 2)
    else:
        arrow(mid, cy - 2, mid, 452)
    text(mid + 14, 515, labels[i][0], size=14, weight="bold", color=INK, anchor="start")
    text(mid + 14, 535, labels[i][1], size=13, color=MUTED, anchor="start")

out.append("</svg>")
Path(__file__).with_name("architecture.svg").write_text("\n".join(out) + "\n")
