"""figures.py [ELO.json] [RECORDS]: the charts and the table of ../RESULTS.md.

ELO.json is what `chess-ladder elo RECORDS/*.jsonl` prints. Both default to
the files beside this script: elo.json and 269e2871/, the records of the
frozen strategy. It writes climb.svg and beyond.svg beside itself and prints
the results table as Markdown.
"""
import glob, json, math, os, sys
from xml.sax.saxutils import escape

HERE = os.path.dirname(os.path.abspath(__file__))
GOAL = 2000
RUNGS = (1320, 1500, 1700)
RULES = "none"  # no model: Suture's rules alone

# The scorer's name in the records -> its name here, and its family.
SCORERS = [
    ("maia1100", "Maia 1100", "chess"),
    ("maia1500", "Maia 1500", "chess"),
    ("maia1900", "Maia 1900", "chess"),
    ("nnue", "NNUE, 1 ply", "chess"),
    ("typesafe/jev-1.14", "Jev 1.14 (TypeSafe)", "decision"),
    ("perplexity/pplx-decider-v1.2", "Decider V1.2 (Perplexity)", "decision"),
    ("cloudflare/clef", "Clef (Cloudflare)", "decision"),
    ("openai/gpt-6-luna-decisions", "GPT-6 Luna Decisions (OpenAI)", "decision"),
    ("google/gemma-4-31b-it", "Gemma 4 31B (Google)", "llm"),
    ("nvidia/nemotron-3-ultra-550b-a55b", "Nemotron 3 Ultra (NVIDIA)", "llm"),
    ("poolside/laguna-s-2.1:free", "Laguna S 2.1 (Poolside)", "llm"),
]
FAMILIES = {"chess": "Chess-trained model", "decision": "General decision model", "llm": "LLM"}

# The palette, light then dark: three categorical slots that pass the
# colour-blind checks pairwise in both modes, and their surfaces and inks.
STYLE = """
:root { --surface: #fcfcfb; --ink: #0b0b0b; --ink2: #52514e; --muted: #898781; --grid: #e1e0d9; --axis: #c3c2b7; --band: #ecebe6;
  --chess: #2a78d6; --decision: #eb6834; --llm: #1baf7a; }
@media (prefers-color-scheme: dark) { :root { --surface: #1a1a19; --ink: #f0efec; --ink2: #c3c2b7; --grid: #2c2c2a; --axis: #383835; --band: #2a2a28;
  --chess: #3987e5; --decision: #d95926; --llm: #199e70; } }
text { font-family: system-ui, -apple-system, "Segoe UI", sans-serif; font-size: 12px; fill: var(--ink); }
.title { font-size: 16px; font-weight: 600; }
.sub { fill: var(--ink2); }
.muted { fill: var(--muted); font-size: 11px; font-variant-numeric: tabular-nums; }
.val { font-weight: 600; font-variant-numeric: tabular-nums; }
.halo { paint-order: stroke; stroke: var(--surface); stroke-width: 3px; stroke-linejoin: round; }
.bg { fill: var(--surface); }
.grid { stroke: var(--grid); }
.rung { stroke: var(--axis); }
.zero { stroke: var(--ink2); }
.goal { stroke: var(--ink); stroke-width: 1.5; stroke-dasharray: 5 4; }
.band { fill: var(--band); }
.alone { fill: var(--surface); stroke: var(--ink2); stroke-width: 1.5; }
.key { fill: var(--ink2); }
.ring { stroke: var(--surface); stroke-width: 2; }
.wash { stroke-width: 8; stroke-linecap: round; stroke-opacity: .25; }
.link { stroke-width: 2; stroke-linecap: round; fill: none; }
.lost { stroke-dasharray: 3 4; }
.keyline { stroke: var(--ink2); stroke-width: 2; }
.f-chess { fill: var(--chess); } .s-chess { stroke: var(--chess); }
.f-decision { fill: var(--decision); } .s-decision { stroke: var(--decision); }
.f-llm { fill: var(--llm); } .s-llm { stroke: var(--llm); }
"""


def width(s, size=12):
    """A rough width of system-ui text, to lay labels out without a browser."""
    w = 0.0
    for c in s:
        w += 0.3 if c in "il.,:;'|!() " else 0.85 if c in "mwMW" else 0.65 if c.isupper() or c.isdigit() else 0.55
    return w * size


def signed(n):
    return ("+" if n > 0 else "−" if n < 0 else "") + str(abs(round(n)))


def load(elo_path, records):
    fit = {r["player"]: r for r in json.load(open(elo_path))}
    levels = {}
    for f in glob.glob(os.path.join(records, "*.jsonl")):
        for line in open(f):
            g = json.loads(line)
            for me, them in ((g["white"], g["black"]), (g["black"], g["white"])):
                if them.startswith("stockfish "):
                    levels.setdefault(me, set()).add(int(them.split()[1]))
    rows = []
    for key, name, fam in SCORERS:
        a, s = fit.get(f"{key} alone"), fit.get(f"{key} with Suture")
        if s:
            rows.append({"name": name, "fam": fam, "alone": a, "suture": s, "lost": bool(a) and a["score"] == 0,
                         "levels": (sorted(levels.get(f"{key} alone", ())), sorted(levels.get(f"{key} with Suture", ())))})
    rules = fit[f"{RULES} with Suture"]
    rules["levels"] = sorted(levels.get(f"{RULES} with Suture", ()))
    for r in rows:
        r["over"] = r["suture"]["elo"] - rules["elo"]
        r["noise"] = math.hypot(r["suture"]["ci"], rules["ci"])
    return sorted(rows, key=lambda r: -r["suture"]["elo"]), rules


def svg(w, h, title, desc, body):
    return "\n".join([
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}" role="img" aria-labelledby="t d">',
        f'<title id="t">{escape(title)}</title><desc id="d">{escape(desc)}</desc>',
        f"<style>{STYLE}</style>",
        f'<rect class="bg" width="{w}" height="{h}" rx="8"/>',
        *body, "</svg>", ""])


def text(x, y, s, cls=None, anchor=None):
    c = f' class="{cls}"' if cls else ""
    a = f' text-anchor="{anchor}"' if anchor else ""
    return f'<text x="{x:.1f}" y="{y:.1f}"{c}{a}>{escape(s)}</text>'


def line(x1, y1, x2, y2, cls, extra=""):
    return f'<line x1="{x1:.1f}" y1="{y1:.1f}" x2="{x2:.1f}" y2="{y2:.1f}" class="{cls}"{extra}/>'


def legend(x, y, items):
    """Items of (kind, label) in one row from x; kind is a family, or a key."""
    out = []
    for kind, label in items:
        if kind in FAMILIES:
            out.append(f'<circle cx="{x + 5:.1f}" cy="{y - 4:.1f}" r="5" class="f-{kind}"/>')
            x += 14
        elif kind == "alone":
            out.append(f'<circle cx="{x + 5:.1f}" cy="{y - 4:.1f}" r="4.5" class="alone"/>')
            x += 14
        elif kind == "suture":
            out.append(f'<circle cx="{x + 5:.1f}" cy="{y - 4:.1f}" r="5" class="key"/>')
            x += 14
        elif kind == "lost":
            out.append(line(x, y - 4, x + 20, y - 4, "keyline lost"))
            x += 26
        elif kind == "band":
            out.append(f'<rect x="{x:.1f}" y="{y - 10:.1f}" width="16" height="12" rx="2" class="band"/>')
            x += 22
        out.append(text(x, y, label, "sub"))
        x += width(label) + 22
    return out


def climb(rows, rules):
    w, row_h, top = 920, 30, 146
    x0 = max(width(r["name"]) for r in rows) + 40
    x1 = w - 104
    lo, hi = 500, 2100
    x = lambda e: x0 + (e - lo) / (hi - lo) * (x1 - x0)
    bottom = top + len(rows) * row_h + 4
    h = bottom + 58
    b = [text(24, 30, "The climb: each model alone, then with Suture", "title"),
         text(24, 50, f"Elo against Stockfish with 95% intervals. Strategy 269e2871, {rows[0]['suture']['games']} games per arm.", "sub")]
    b += legend(24, 78, [(f, label + "s" if f != "llm" else "LLMs") for f, label in FAMILIES.items()])
    b += legend(24, 100, [("alone", "alone"), ("suture", "with Suture (shaded: its 95% range)"),("lost", "lost every game alone"), ("band", "Suture's rules, no model (95% range)")])
    lo_r, hi_r = rules["elo"] - rules["ci"], rules["elo"] + rules["ci"]
    b.append(f'<rect x="{x(lo_r):.1f}" y="{top - 6:.1f}" width="{x(hi_r) - x(lo_r):.1f}" height="{bottom - top + 6:.1f}" class="band"/>')
    for t in range(600, GOAL + 1, 200):
        b.append(line(x(t), top - 6, x(t), bottom, "grid"))
        b.append(text(x(t), bottom + 16, f"{t:,}", "muted", "middle"))
    for i, r in enumerate(RUNGS):
        b.append(line(x(r), top - 10, x(r), bottom, "rung"))
        b.append(text(x(r), top - (16 if i % 2 == 0 else 30), f"Stockfish {r}", "muted", "middle"))
    b.append(line(x(GOAL), top - 10, x(GOAL), bottom, "goal"))
    b.append(text(x(GOAL), top - 30, f"Goal {GOAL:,}", "val", "middle"))
    b.append(text(x(rules["elo"]), bottom + 38, f"Suture's rules with no model: {rules['elo']:.0f} ± {rules['ci']:.0f}", "muted", "middle"))
    for f in FAMILIES:
        b.append(f'<marker id="head-{f}" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="5" markerHeight="5" orient="auto-start-reverse">'
                 f'<path d="M0,0 L10,5 L0,10 z" class="f-{f}"/></marker>')
    for i, r in enumerate(rows):
        y = top + i * row_h + row_h / 2
        s, f = r["suture"], r["fam"]
        e = x(s["elo"])
        b.append(text(x0 - 14, y + 4, r["name"], None, "end"))
        b.append(line(x(s["elo"] - s["ci"]), y, x(s["elo"] + s["ci"]), y, f"wash s-{f}"))
        if r["alone"] and not r["lost"]:
            a = x(r["alone"]["elo"])
            step = 1 if e > a else -1
            b.append(line(a + 6 * step, y, e - 7 * step, y, f"link s-{f}", f' marker-end="url(#head-{f})"'))
            b.append(f'<circle cx="{a:.1f}" cy="{y:.1f}" r="4.5" class="alone"/>')
        elif r["lost"]:
            b.append(line(x0, y, e - 7, y, f"link lost s-{f}", f' marker-end="url(#head-{f})"'))
            b.append(text(x0 + 4, y - 7, "lost every game", "muted"))
        b.append(f'<circle cx="{e:.1f}" cy="{y:.1f}" r="5.5" class="f-{f} ring"/>')
        b.append(text(e + 11, y + 4, f"{s['elo']:.0f}", "val halo"))
        if s["games"] < rows[0]["suture"]["games"]:
            b.append(text(e + 13 + width(f"{s['elo']:.0f}"), y + 4, f"· {s['games']} games", "muted halo"))
    desc = "; ".join(f"{r['name']}: alone {'lost every game' if r['lost'] else int(r['alone']['elo'])}, with Suture {int(r['suture']['elo'])}" for r in rows)
    return svg(w, h, "Each model alone, then with Suture", desc + f"; Suture's rules with no model {int(rules['elo'])}.", b)


def beyond(rows, rules):
    rows = sorted(rows, key=lambda r: -r["over"])
    w, row_h, top = 920, 28, 104
    x0 = max(width(r["name"]) for r in rows) + 40
    x1 = w - 210
    ext = 200 * math.ceil(max(abs(r["over"]) + r["noise"] for r in rows) / 200)
    x = lambda v: x0 + (v + ext) / (2 * ext) * (x1 - x0)
    bottom = top + len(rows) * row_h + 4
    h = bottom + 58
    b = [text(24, 30, "Do the model's scores add anything to the rules?", "title"),
         text(24, 50, f"Elo with Suture minus Suture's rules with no model ({rules['elo']:.0f} ± {rules['ci']:.0f}), with 95% intervals.", "sub")]
    b += legend(24, 78, [(f, label + "s" if f != "llm" else "LLMs") for f, label in FAMILIES.items()])
    for t in range(-ext, ext + 1, 200):
        b.append(line(x(t), top - 6, x(t), bottom, "zero" if t == 0 else "grid"))
        b.append(text(x(t), bottom + 16, signed(t) if t else "0", "muted", "middle"))
    b.append(text(x(0) - 10, bottom + 38, "← the scores hurt the rules", "muted", "end"))
    b.append(text(x(0) + 10, bottom + 38, "the scores help the rules →", "muted"))
    for i, r in enumerate(rows):
        y = top + i * row_h + row_h / 2
        f, d, n = r["fam"], r["over"], r["noise"]
        b.append(text(x0 - 14, y + 4, r["name"], None, "end"))
        b.append(line(x(d - n), y, x(d + n), y, f"link s-{f}"))
        for v in (d - n, d + n):
            b.append(line(x(v), y - 4, x(v), y + 4, f"link s-{f}"))
        b.append(f'<circle cx="{x(d):.1f}" cy="{y:.1f}" r="5.5" class="f-{f} ring"/>')
        verdict = "helps" if d - n > 0 else "hurts" if d + n < 0 else "no clear difference"
        b.append(text(x1 + 24, y + 4, signed(d), "val"))
        b.append(text(x1 + 70, y + 4, verdict, "sub"))
    desc = "; ".join(f"{r['name']}: {signed(r['over'])} ± {r['noise']:.0f}" for r in rows)
    return svg(w, h, "With Suture, compared with Suture's rules and no model", desc, b)


def table(rows, rules):
    def lv(v):
        return ", ".join(map(str, v)) or "–"
    out = ["| Model | Family | Alone | With Suture | Lift | vs rules alone | Games, alone / with | Stockfish levels, alone / with |",
           "|---|---|---|---|---|---|---|---|",
           f"| Suture's rules, no model | – | – | **{rules['elo']:.0f} ± {rules['ci']:.0f}** | – | – | – / {rules['games']} | – / {lv(rules['levels'])} |"]
    for r in rows:
        a, s = r["alone"], r["suture"]
        alone = "–" if not a else f"lost all {a['games']}" if r["lost"] else f"{a['elo']:.0f} ± {a['ci']:.0f}"
        lift = "–" if not a or r["lost"] else f"{signed(s['elo'] - a['elo'])} ± {math.hypot(s['ci'], a['ci']):.0f}"
        out.append(f"| {r['name']} | {FAMILIES[r['fam']]} | {alone} | **{s['elo']:.0f} ± {s['ci']:.0f}** | {lift} | "
                   f"{signed(r['over'])} ± {r['noise']:.0f} | {a['games'] if a else 0} / {s['games']} | {lv(r['levels'][0])} / {lv(r['levels'][1])} |")
    return "\n".join(out)


if __name__ == "__main__":
    rows, rules = load(sys.argv[1] if len(sys.argv) > 1 else os.path.join(HERE, "elo.json"),
                       sys.argv[2] if len(sys.argv) > 2 else os.path.join(HERE, "269e2871"))
    open(os.path.join(HERE, "climb.svg"), "w").write(climb(rows, rules))
    open(os.path.join(HERE, "beyond.svg"), "w").write(beyond(rows, rules))
    print(table(rows, rules))
