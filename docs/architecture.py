"""Draws docs/architecture.svg: where Suture runs, and what it links.

    python3 docs/architecture.py

Plain SVG with no CSS and no fonts embedded, so it renders the same everywhere.
"""

from pathlib import Path
from xml.sax.saxutils import escape

W, H = 1400, 1060
INK, MUTED, LINE = "#1f2933", "#52606d", "#7b8794"
ORANGE, CREAM, ORANGE_SOFT = "#d9822b", "#fff8ec", "#e0a85a"
BLUE, BLUE_SOFT = "#326ce5", "#eef3fd"
GREEN, GREEN_SOFT = "#2f8f5b", "#ecf7f1"
PURPLE, PURPLE_SOFT = "#7c5cc4", "#f3effb"
GREY, GREY_LINE = "#f5f7fa", "#9aa5b1"
FONT = "font-family=\"-apple-system, 'Segoe UI', Helvetica, Arial, sans-serif\""

out: list[str] = []


def box(x, y, w, h, fill, stroke, width=1.5, r=10, dash=None):
    d = f' stroke-dasharray="{dash}"' if dash else ""
    out.append(f'<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="{r}" fill="{fill}" stroke="{stroke}" stroke-width="{width}"{d}/>')


def text(x, y, s, size=14, weight="normal", color=INK, anchor="middle"):
    out.append(f'<text x="{x}" y="{y}" {FONT} font-size="{size}" font-weight="{weight}" fill="{color}" text-anchor="{anchor}">{escape(s)}</text>')


def line(x1, y1, x2, y2, color=LINE, head=True, both=False, width=2):
    m = ' marker-end="url(#head)"' if head else ""
    m += ' marker-start="url(#head)"' if both else ""
    out.append(f'<line x1="{x1}" y1="{y1}" x2="{x2}" y2="{y2}" stroke="{color}" stroke-width="{width}"{m}/>')


def pod(x, y, w, h, name, what, stroke, fill="#ffffff"):
    """A pod: its name, what it is, and a small `pod` tag."""
    box(x, y, w, h, fill, stroke, r=8)
    text(x + w / 2 - 10, y + h / 2 - 3, name, size=14, weight="bold")
    text(x + w / 2 - 10, y + h / 2 + 16, what, size=12, color=MUTED)
    box(x + w - 34, y + 6, 28, 15, stroke, stroke, width=1, r=4)
    text(x + w - 20, y + 17, "pod", size=10, weight="bold", color="#ffffff")


out.append(f'<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" viewBox="0 0 {W} {H}">')
out.append(f'<defs><marker id="head" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M0,0 L10,5 L0,10 z" fill="{LINE}"/></marker></defs>')
out.append(f'<rect width="{W}" height="{H}" fill="#ffffff"/>')

# Outside the cluster: who calls in, and the providers called out to.
box(500, 20, 400, 78, GREY, GREY_LINE)
text(700, 50, "Callers", size=16, weight="bold")
text(700, 74, "web and mobile apps · staff tools · partner systems", size=13, color=MUTED)
box(1000, 20, 360, 78, GREY, GREY_LINE)
text(1180, 50, "External providers", size=16, weight="bold")
text(1180, 74, "payments · mail · carriers · credit bureaus", size=13, color=MUTED)

# The cluster.
box(40, 130, 1320, 660, "#fbfcfe", BLUE, width=2, r=14)
text(64, 158, "Kubernetes cluster", size=16, weight="bold", color=BLUE, anchor="start")

# Ingress, in front of Suture.
pod(570, 175, 260, 58, "ingress / gateway", "TLS, authentication", BLUE)
line(700, 98, 700, 173)
line(700, 233, 700, 283)
text(712, 262, "admitted inputs (admit)", size=12, color=MUTED, anchor="start")

# Left: platform services.
box(70, 200, 300, 400, GREEN_SOFT, GREEN, r=12)
text(220, 226, "Platform services", size=15, weight="bold", color=GREEN)
left = [("PostgreSQL", "customer and order data"), ("Redis", "cache"), ("Vault", "secrets, API keys"), ("Prometheus", "metrics and alerts")]
left_why = ["source", "source (cache)", "API keys (env)", "metrics"]

# Right: your own services, and the adapters to providers.
box(1030, 200, 300, 400, GREEN_SOFT, GREEN, r=12)
text(1180, 226, "Your services and adapters", size=15, weight="bold", color=GREEN)
right = [("payments-adapter", "talks to the payment provider"), ("mail-adapter", "sends mail"), ("carrier-adapter", "books shipments"), ("inventory service", "an existing service")]
right_why = ["effect", "effect", "effect", "source · effect"]

for i in range(4):
    y = 245 + i * 86
    pod(90, y, 260, 66, *left[i], GREEN)
    pod(1050, y, 260, 66, *right[i], GREEN)

# Suture in the middle: its runtime pods and its journal.
SX, SY, SW, SH = 470, 285, 460, 280
box(SX, SY, SW, SH, CREAM, ORANGE, width=3, r=14)
text(SX + SW / 2, SY + 32, "Suture", size=22, weight="bold", color="#a85d12")
text(SX + SW / 2, SY + 54, "the business logic: the one place that links every pod", size=13, color=MUTED)
for i in range(3):
    pod(SX + 22 + i * 142, SY + 76, 132, 70, f"suture-{i}", "runtime", ORANGE)
# The journal, as a persistent volume.
jx, jy, jw = SX + 130, SY + 182, 200
out.append(f'<path d="M{jx},{jy} v52 a{jw / 2},12 0 0 0 {jw},0 v-52" fill="#ffffff" stroke="{ORANGE_SOFT}" stroke-width="1.5"/>')
out.append(f'<ellipse cx="{jx + jw / 2}" cy="{jy}" rx="{jw / 2}" ry="12" fill="#ffffff" stroke="{ORANGE_SOFT}" stroke-width="1.5"/>')
text(jx + jw / 2, jy + 30, "journal", size=14, weight="bold")
text(jx + jw / 2, jy + 48, "persistent volume, replayable", size=12, color=MUTED)

# Suture's links to each pod, each through a declared boundary.
for i in range(4):
    y = 245 + i * 86 + 33
    sy = min(max(y, SY + 20), SY + SH - 20)
    line(SX - 2, sy, 352, y, both=True)
    text(411, (sy + y) / 2 - 9, left_why[i], size=12, color=MUTED)
    line(SX + SW + 2, sy, 1048, y, both=True)
    text(989, (sy + y) / 2 - 9, right_why[i], size=12, color=MUTED)
line(1180, 200, 1180, 100)
text(1192, 120, "calls out", size=12, color=MUTED, anchor="start")

# GPU nodes: the compute engines that serve the decision models.
box(250, 625, 900, 145, PURPLE_SOFT, PURPLE, r=12)
text(270, 650, "GPU node pool", size=15, weight="bold", color=PURPLE, anchor="start")
text(1130, 650, "the engine behind decisions", size=12, color=MUTED, anchor="end")
pod(300, 672, 250, 70, "vLLM", "serves a decision model (Jev)", PURPLE)
pod(575, 672, 250, 70, "SGLang", "serves a decision model", PURPLE)
for i in range(4):
    gx = 870 + i * 64
    box(gx, 682, 50, 50, "#ffffff", PURPLE, r=6)
    text(gx + 25, 712, "GPU", size=11, weight="bold", color=PURPLE)
line(700, SY + SH + 2, 700, 670)
text(712, 598, "decide: pick one of the options the rules allow", size=12, color=MUTED, anchor="start")

# The base everything stands on.
box(40, 806, 1320, 52, BLUE_SOFT, BLUE, r=8)
text(700, 828, "Kubernetes", size=15, weight="bold", color=BLUE)
text(700, 848, "schedules every pod above · services and networking · volumes · autoscaling · GPU scheduling", size=12, color=MUTED)
box(40, 866, 1320, 52, "#f3f0fb", "#5c4ee5", r=8)
text(700, 888, "Terraform", size=15, weight="bold", color="#5c4ee5")
text(700, 908, "provisions the cluster, its CPU and GPU node pools, networks and managed databases", size=12, color=MUTED)
box(40, 926, 1320, 36, GREY, GREY_LINE, r=8)
text(700, 949, "cloud or bare metal", size=13, color=MUTED)

# What Suture is, and is not.
text(700, 1000, "Suture links, it does not prompt: it calls services through declared adapters, and a model only ever picks among options its rules already allow.", size=13, weight="bold")
text(700, 1024, "No prompts and no generated code. Every pick is checked and journaled; with every model off, `prefer` decides and the system still runs.", size=13, color=MUTED)

out.append("</svg>")
Path(__file__).with_name("architecture.svg").write_text("\n".join(out) + "\n")
