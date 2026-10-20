#!/usr/bin/env python3
"""Counts what eval/returns/RESULTS.md reports.

    count.py ⟨solidus checkout⟩ [⟨solidus commit⟩ [⟨suture commit⟩]]
    count.py ⟨solidus checkout⟩ --change ⟨solidus commit⟩

Lines that are not blank or comments. In Ruby, control-flow statements; in
Suture, which has none, the clauses that make a condition or a fallback.

Solidus is counted over the files that implement what the port covers
(REQUIREMENTS.md), at the given commit (default bafb57f, the one ported).
Lines and methods for what the port leaves out are not counted: exchanges,
taxes, simulation, the tolerance check and the `errored` state, splitting a
refund over several payments, the override type, display, generated numbers,
admin search and deprecations. The filter is by name and pattern, so a few
lines either way may be off (a `def total` whose tax line is dropped stays).
"""
import re, subprocess, sys, pathlib

root = pathlib.Path(__file__).parent
solidus = pathlib.Path(sys.argv[1])
solidus_rev = sys.argv[2] if len(sys.argv) > 2 else "bafb57f"
rev = sys.argv[3] if len(sys.argv) > 3 else None

M = "core/app/models/spree/"
FILES = [
    M + "return_item.rb",
    M + "return_authorization.rb",
    M + "customer_return.rb",
    M + "reimbursement.rb",
    M + "reimbursement_performer.rb",
    M + "reimbursement/reimbursement_type_engine.rb",
    M + "reimbursement/reimbursement_type_validator.rb",
    M + "reimbursement_type/reimbursement_helpers.rb",
    M + "reimbursement_type/original_payment.rb",
    M + "reimbursement_type/store_credit.rb",
    M + "return_item/eligibility_validator/base_validator.rb",
    M + "return_item/eligibility_validator/default.rb",
    M + "return_item/eligibility_validator/order_completed.rb",
    M + "return_item/eligibility_validator/time_since_purchase.rb",
    M + "return_item/eligibility_validator/rma_required.rb",
    M + "return_item/eligibility_validator/inventory_shipped.rb",
    M + "return_item/eligibility_validator/no_reimbursements.rb",
    M + "core/state_machines/return_item/acceptance_status.rb",
    M + "core/state_machines/return_item/reception_status.rb",
    M + "core/state_machines/return_authorization.rb",
    M + "core/state_machines/reimbursement.rb",
    M + "calculator/returns/default_refund_amount.rb",
    M + "returns_calculator.rb",
]

# Methods for what the port leaves out, skipped whole.
SKIP_METHODS = {
    "simulate", "display_total", "generate_number", "currency", "total_excluding_vat",
    "return_items_requiring_exchange", "any_exchanges?", "all_exchanges?", "exchange_requested?",
    "exchange_processed?", "exchange_required?", "eligible_exchange_variants",
    "build_exchange_inventory_unit", "exchange_shipment", "potential_reception_transitions",
    "part_of_exchange?", "sibling_intended_for_exchange", "check_unexchange",
    "eligible_exchange_variant", "set_exchange_amount",
    "unpaid_amount_within_tolerance?", "paid_amount", "unpaid_amount", "sorted_eligible_refund_payments",
}
# Single lines for what the port leaves out.
SKIP_LINE = re.compile(r"exchange|tax|display|ransack|Metadata|DisplayMoney|money_methods|generate_number|simulat|deprecator|frozen_string_literal"
                       r"|override|toleran|leniency|errored|IncompleteReimbursementError|credit_allowed|can_credit\?|unpaid_amount (<=|-=)|eligible_refund_methods", re.I)

def git(repo, rev, rel):
    return subprocess.run(["git", "-C", str(repo), "show", f"{rev}:{rel}"], capture_output=True, text=True, check=True).stdout

def ruby_logic(src):
    out, skip_to = [], None
    for line in src.splitlines():
        s = line.strip()
        indent = len(line) - len(line.lstrip())
        if skip_to is not None:
            if s == "end" and indent == skip_to:
                skip_to = None
            continue
        m = re.match(r"def (self\.)?([\w?!]+)", s)
        if m and m.group(2) in SKIP_METHODS:
            if not s.endswith(" end") and "=" not in s.split(")")[-1]:
                skip_to = indent
            continue
        if s and not s.startswith("#") and not SKIP_LINE.search(s):
            out.append(s)
    return out

def suture_logic(rel):
    src = (root / rel).read_text() if rev is None else git(root.parent.parent, rev, f"eval/returns/{rel}")
    return [s for s in (l.strip() for l in src.splitlines()) if s and not s.startswith("--")]

RUBY_FLOW = r"\b(if|unless|elsif|else|case|when|while|until|loop|return|break|next)\b|\.each\b"
SUTURE_CONDITIONS = r"\b(unless|where|whose|while|otherwise|after|fails with)\b"

if solidus_rev == "--change":
    # `count.py ⟨checkout⟩ --change ⟨commit⟩`: the counted lines a Solidus
    # commit adds and removes in core/app, by the same filter.
    import difflib
    commit = sys.argv[3]
    changed = subprocess.run(["git", "-C", str(solidus), "show", "--format=", "--name-only", commit, "--", "core/app"],
                             capture_output=True, text=True, check=True).stdout.split()
    def at(r, f):
        try:
            return ruby_logic(git(solidus, r, f))
        except subprocess.CalledProcessError:
            return []
    plus = minus = 0
    for f in changed:
        for d in difflib.ndiff(at(commit + "^", f), at(commit, f)):
            plus += d.startswith("+ ")
            minus += d.startswith("- ")
        print(f"  {f}")
    print(f"{commit}: +{plus} −{minus} counted lines")
    sys.exit(0)

ruby = [l for f in FILES for l in ruby_logic(git(solidus, solidus_rev, f))]
st = suture_logic("suture/returns.st")
print(f"{'suture':10} {len(st):5} lines  {sum(len(re.findall(SUTURE_CONDITIONS, l)) for l in st):4} condition and fallback clauses (no control flow)")
print(f"{'solidus':10} {len(ruby):5} lines  {sum(len(re.findall(RUBY_FLOW, l)) for l in ruby):4} control-flow statements, in {len(FILES)} files")
sc = suture_logic("suture/returns.scenario")
print(f"{'scenarios':10} {len(sc):5} lines  {sum(1 for l in sc if re.match(r'(when|and|expect)\b', l)):4} steps")
