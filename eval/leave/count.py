#!/usr/bin/env python3
"""Counts what eval/leave/RESULTS.md reports.

    python3 -I count.py ⟨hrms checkout⟩ [⟨hrms commit⟩]

Lines that are not blank, comments or docstrings. In Python, control-flow
statements; in Suture, which has none, the clauses that make a condition.

hrms is counted over the functions of leave_application.py that do what the
port covers (REQUIREMENTS.md), at the given commit (default 7fc314bdb, the one
ported). The source is parsed, never run. Lines in those functions about what
the port leaves out are dropped by pattern: holidays and half days, attendance
and salary, carry-forward and expiry, block dates, optional leave, workflows,
notifications, messages and links. The filter is by pattern, so a few lines
either way may be off.
"""
import ast, re, subprocess, sys, pathlib

root = pathlib.Path(__file__).parent
hrms = pathlib.Path(sys.argv[1])
rev = sys.argv[2] if len(sys.argv) > 2 else "7fc314bdb"

FILE = "hrms/hr/doctype/leave_application/leave_application.py"
FUNCTIONS = {
    "validate", "on_submit", "before_cancel", "on_discard", "on_cancel",
    "validate_applicable_after", "validate_dates", "validate_dates_across_allocation",
    "get_allocation_based_on_application_dates", "validate_balance_leaves",
    "show_insufficient_balance_message", "validate_max_days", "validate_leave_approver",
    "validate_for_self_approval", "create_leave_ledger_entry", "get_leave_details",
    "get_leave_balance_on", "get_leave_allocation_records", "get_leaves_pending_approval_for_period",
    "get_remaining_leaves", "_get_remaining_leaves", "get_leaves_for_period", "get_leave_entries",
    "is_lwp", "get_leave_approver", "get_employee_leave_approver", "validate_leave_overlap", "throw_overlap_error",
    "get_consecutive_leave_details",
}
SKIP_LINE = re.compile(r"holiday|half_day|attendance|salary|carry_forward|is_carry|cf_|expir|block|optional|workflow"
                       r"|notif|publish|msgprint|_\(\"|get_link_to_form|frappe\.bold|precision|title=|raise_exception", re.I)

def source(rel):
    return subprocess.run(["git", "-C", str(hrms), "show", f"{rev}:{rel}"], capture_output=True, text=True, check=True).stdout

# Counting leave days less holidays and half days, which the port does since
# slice 61 (its calendar): counted apart, dropping messages as the main count
# does, but keeping the lines about holidays and half days.
DAYS = {"get_number_of_leave_days", "get_holidays", "validate_half_day_date"}
MESSAGES = re.compile(r"notif|publish|msgprint|_\(\"|get_link_to_form|frappe\.bold|precision|title=|raise_exception", re.I)

def python_logic(src, functions=FUNCTIONS, skip=SKIP_LINE):
    tree = ast.parse(src)
    lines = src.splitlines()
    keep = set()
    for node in ast.walk(tree):
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)) and node.name in functions:
            keep.update(range(node.lineno, node.end_lineno + 1))
    for node in ast.walk(tree):
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)) and node.body:
            first = node.body[0]
            if isinstance(first, ast.Expr) and isinstance(first.value, ast.Constant) and isinstance(first.value.value, str):
                keep.difference_update(range(first.lineno, first.end_lineno + 1))
    out = []
    for n in sorted(keep):
        s = lines[n - 1].strip()
        if s and not s.startswith("#") and not s.startswith("@") and not (skip and skip.search(s)):
            out.append(s)
    return out

def suture_logic(rel):
    return [s for s in (l.strip() for l in (root / rel).read_text().splitlines()) if s and not s.startswith("--")]

PY_FLOW = r"\b(if|elif|else|for|while|return|break|continue|try|except)\b"
SUTURE_CONDITIONS = r"\b(where|when|whose|while|unless|or|and)\b"

py = python_logic(source(FILE))
st = suture_logic("suture/leave.st")
conds = sum(len(re.findall(SUTURE_CONDITIONS, l)) for l in st if l.startswith(("admit", "define", "rule")))
print(f"{'suture':10} {len(st):5} lines  {conds:4} condition clauses and connectives (no control flow)")
print(f"{'hrms':10} {len(py):5} lines  {sum(len(re.findall(PY_FLOW, l)) for l in py):4} control-flow statements, in {len(FUNCTIONS)} functions")
days = python_logic(source(FILE), DAYS, MESSAGES)
print(f"{'days':10} {len(days):5} lines of hrms counting leave days, in {len(DAYS)} functions; the port: its `calendar` line")
sc = suture_logic("suture/leave.scenario")
print(f"{'scenarios':10} {len(sc):5} lines  {sum(1 for l in sc if re.match(r'(when|and|expect|given)\b', l)):4} steps")
