#!/usr/bin/env python3
"""Counts what eval/claims/RESULTS.md reports: lines of logic (not blank,
not comments) and control-flow constructs, for each version."""
import re, sys, pathlib

root = pathlib.Path(__file__).parent

def logic(path, comment):
    out = []
    for line in path.read_text().splitlines():
        s = line.strip()
        if s and not s.startswith(comment):
            out.append(s)
    return out

FLOW = r"\b(if|else|match|for|while|loop|return)\b|\?;|=>"

def report(name, files, comment):
    lines = [l for f in files for l in logic(f, comment)]
    flow = sum(len(re.findall(FLOW, l)) for l in lines) if comment == "//" else 0
    print(f"{name:10} {len(lines):5} lines  {flow:4} control-flow constructs  ({', '.join(str(f.relative_to(root)) for f in files)})")

report("suture", [root / "suture/claims.st"], "--")
report("baseline", [root / "baseline/src/lib.rs"], "//")
report("scenarios", [root / "suture/claims.scenario"], "--")
report("tests", [root / "baseline/tests/scenarios.rs"], "//")
