#!/usr/bin/env python3
"""Counts what eval/claims/RESULTS.md reports, for the working tree or for a
commit (`count.py 1a2b3c`): lines that are not blank or comments; in Rust,
control-flow statements; in Suture, which has none, the clauses that make a
condition or a fallback."""
import re, subprocess, sys, pathlib

root = pathlib.Path(__file__).parent
rev = sys.argv[1] if len(sys.argv) > 1 else None

def text(rel):
    if rev is None:
        return (root / rel).read_text()
    return subprocess.run(["git", "show", f"{rev}:eval/claims/{rel}"], capture_output=True, text=True, check=True).stdout

def logic(rel, comment):
    return [s for s in (l.strip() for l in text(rel).splitlines()) if s and not s.startswith(comment)]

RUST_FLOW = r"\b(if|else|match|for|while|loop|return)\b"
SUTURE_CONDITIONS = r"\b(unless|where|whose|while|otherwise|after|fails with)\b"

def report(name, rel, comment, pattern, what):
    lines = logic(rel, comment)
    n = sum(len(re.findall(pattern, l)) for l in lines)
    print(f"{name:10} {len(lines):5} lines  {n:4} {what}")

report("suture", "suture/claims.st", "--", SUTURE_CONDITIONS, "condition and fallback clauses (no control flow)")
report("baseline", "baseline/src/lib.rs", "//", RUST_FLOW, "control-flow statements")
report("scenarios", "suture/claims.scenario", "--", r"^(when|and|expect)\b", "steps")
report("tests", "baseline/tests/scenarios.rs", "//", r"\bassert", "asserts")
