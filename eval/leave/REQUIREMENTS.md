# Leave: what is ported from frappe/hrms

The third comparison (after eval/claims and eval/returns), and the first outside commerce. The baseline is the leave application flow of [frappe/hrms](https://github.com/frappe/hrms), an open-source HR system, at `7fc314bdb` (2026-10-12): `hrms/hr/doctype/leave_application/leave_application.py`. The port is `suture/leave.st`, and its scenarios come from `test_leave_application.py` (45 tests), one scenario per test that the port covers, named after it. The tests were read, never run.

## What hrms does, and the port does too

Each line names the hrms code that does it.

1. **Applying.** An employee, or HR, applies for leave of a type, from one date to another, against an allocation of that type (`validate`). The to date may not be before the from date (`validate_dates`).
2. **Inside one allocation.** The leave must lie inside one allocation's period, unless its type allows a negative balance (`validate_dates_across_allocation`).
3. **The balance.** The days applied for may not be more than the allocation's leaves less the days already approved against it, unless the type allows a negative balance (`validate_balance_leaves`, `get_leave_balance_on`). hrms checks it on every save, so also when an approval is submitted. A cancelled leave gives its days back (`on_cancel`, `create_leave_ledger_entry`).
4. **Applicable after.** A type may be applicable only some days after the employee joined (`validate_applicable_after`). Leave before the joining date is not checked, as in hrms.
5. **Longest leave.** A type may limit how many days one leave lasts (`validate_max_days`), for a single application.
6. **Backdating.** Where the company restricts it, an employee may not apply for leave that starts before today. HR may (`validate_dates`, `restrict_backdated_leave_application` with the allowed role).
7. **The approver.** An application names its approver, who is mandatory when HR Settings say so (`validate_leave_approver`). Only that approver, or HR, approves or rejects it. When HR Settings prevent self-approval, an employee may not approve their own leave (`validate_for_self_approval`).
8. **Statuses.** An application is `open`, then `approved` or `rejected`. Discarding an open one, or cancelling an approved one, makes it `cancelled` (`on_discard`, `before_cancel`).
9. **The dashboard.** The days taken, pending approval and remaining are read off each allocation (`get_leave_details`).

## What the port leaves out

These are left out on both sides, and `count.py` drops hrms's lines for them by pattern:

- attendance, salary slips and check-ins;
- notifications and real-time updates;
- read permissions and document sharing (three access tests);
- block dates and optional holidays;
- carry-forward, expiry and the ledger split across allocations;
- half days, and holiday lists.

## Where the port differs, because of Suture v0

Each of these is a finding (RESULTS.md):

- **Days are given.** hrms counts the days between the dates, less holidays, and halves a half day. The port takes `days` as given: Suture cannot count the days between two times (no Duration ÷ Duration), and has no fractional counts.
- **The allocation is given.** hrms finds the allocation whose period holds the dates. The port has the caller name it and checks that it fits, because a unit cannot pick another unit by a range.
- **Overlap.** hrms refuses a leave that overlaps another open or approved leave of the employee (`validate_leave_overlap`). A relation filter reads only the sibling, never the reader, so `the number of its employee's Applications that end after its from` cannot be written. Ported since slice 28 as a total per day over the days each application spans; two half days on one date still overlap.
- **Consecutive leave across applications.** hrms adds the leaves just before and after when checking the longest leave. Not ported, for the same reason.
- **Settings.** hrms reads its switches from HR Settings, one for the site. The port puts the two it needs on the Company (`selfish`, `backdating`), and always requires an approver.
- **Approve is one step.** hrms sets the status, then submits; the port's `approve` does both.
- **The balance at application.** hrms checks it on save. The port checks it at the gate when applying, and the rule checks it again when approving.
- **A missing approver** was refused only because the gate read `its approver's user`, which was unknown: Suture let an attribute with no default be left out. Since slice 29 an attribute with no default is required, and the application is refused at the gate because it has no approver.
