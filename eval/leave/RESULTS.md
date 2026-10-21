# Leave: Suture against frappe/hrms

The third measured comparison, and the first outside commerce. The leave application flow of [frappe/hrms](https://github.com/frappe/hrms) was ported to Suture (REQUIREMENTS.md says what was ported and what was not), and each of its 45 tests was either translated to a scenario or listed below with the reason it was not.

`python3 -I count.py ⟨hrms checkout⟩ [⟨hrms commit⟩]` gives the numbers below.

## Numbers

| | Suture | hrms |
|---|---|---|
| Lines of logic | 39 (41 with overlap, after slice 28) | 400, in the 26 functions that do what the port does (438 in 28, with overlap) |
| Control-flow statements (`if`, `for`, `return`, `try`, …) | none: the language has none | 77 (81) |
| Condition clauses and connectives (`where`, `when`, `and`, `or`) | 48 (51) | (inside the 77) |
| Tests | 14 scenarios, in 162 lines (15 in 180) | 45 tests in 1699 lines; 14 translated, 23 out of scope, 8 not expressible (15, 23 and 7 after slice 28) |
| Overlap, after slice 28 | **+2** (a `define` and a rule) | 38 counted lines: `validate_leave_overlap` and `throw_overlap_error` |

hrms's count drops, by pattern, its lines about holidays and half days, attendance and salary, carry-forward and expiry, block dates, optional leave, workflows, notifications and messages. A few lines either way may be off. Frappe gives hrms a database, forms, permissions and an API. Suture gives the port a durable log and replay, and the read-back of every rule.

## The tests

| Test | In the port |
|---|---|
| `overlap` (since slice 28), `validate_application_across_allocations`, `insufficient_leave_balance_validation`, `leaves_allowed`, `applicable_after`, `max_continuous_leaves`, `current_leave_on_submit`, `creation_of_leave_ledger_entry_on_submit`, `leave_approver_perms`, `leave_approver_mandatory`, `self_leave_approval_allowed`, `self_leave_approval_not_allowed`, `get_leave_details_for_dashboard`, `status_on_discard`, `leave_access_control_flow` | a scenario each. Two test something next to their hrms test, and say `after`: `leave_approver_perms` checks sharing with the approver, and the port checks that only the named approver approves; `leave_access_control_flow` checks read permissions (out of scope), and the port checks that another employee may not approve or discard |
| 7 attendance tests (`overwrite_attendance`, `…_half_day_attendance`, `attendance_for_include_holidays`, `attendance_update_for_exclude_holidays`, `modifying_attendance_…` ×2, `half_day_status_for_two_half_leaves`) | out of scope |
| `block_list`, `optional_leave` | out of scope |
| 12 carry-forward, expiry and ledger tests (`separate_leave_ledger_entry_for_boundary_applications`, `leave_balance_near_allocaton_expiry`, `ledger_entry_creation_on_intermediate_allocation_expiry`, `leave_application_creation_after_expiry`, `leave_details_with_…cf_expiry` ×4, `get_leave_allocation_records`, `filtered_old_cf_entries_…`, `leave_balance_when_allocation_is_expired_manually`, `backdated_application_after_expiry`) | out of scope |
| `leave_approver_with_restricted_employee_access`, `leave_access_for_workflow_reviewer_without_employee_permission` | out of scope: read permissions |
| `overlap_with_half_day_1`…`_3`, `overlap_with_half_day_on_today` | **not expressible**: two half days may share a day |
| `max_consecutive_leaves_across_leave_applications` | **not expressible**: a run across applications |
| `half_day_date_cannot_be_holiday`, `leave_days_across_two_holiday_lists` | **not expressible**: counting days |

## What we learned

**Where it wins.**
- The port is a tenth of the size, with no control flow. Most of hrms's lines are queries that find the allocation, the approved leaves and the balance. In the port these are one rule (`the total days of each Allocation's Applications that are counted is at most its leaves`) and two relation totals.
- Cancelling gives the days back with no code. hrms writes a reverse ledger entry; in the port a cancelled leave stops being `counted`, and its days leave the total.
- Every check in hrms's `validate` is one `define`. The gates read as the policy: an employee applies for their own leave, and only on time; HR applies for anyone, at any time.

**Two bugs in Suture, found by porting and fixed here.**
- **`applies` was not `apply`.** The third person of a verb ending in a consonant and `y` was `applys`, so `employee E applies …` matched no `admit`. Every such verb was broken: apply, deny, carry, copy, reply, verify. Neither earlier eval had one.
- **An `expect` about a total the program does not keep read 0.** `expect the total days of AT's Applications that are approved is 4` failed, saying 0, because no rule read that total, so no unit added to it. It is now an error that says so. The dashboard's totals are kept by two `define`s that nothing else uses, which is a workaround: a program should be able to say "keep this total".

**What Suture could not express.**
- **Overlap, the biggest gap (ported since slice 28, below).** "No two open or approved leaves of one employee overlap" compares a sibling with the reader: `the number of its employee's Applications that are approved and whose to is at least its from and whose from is at most its to`. A `that` filter on a relation reads only the sibling, so it cannot be written. The same gap blocks consecutive leave across applications. Six tests need it. Booking, scheduling and rostering have the same shape.
- **Counting days.** Days between two dates, holidays and half days. Suture has no Duration ÷ Duration and no fractional counts, so the port takes `days` as given, and two tests cannot be written.
- **Finding the allocation.** hrms finds the allocation whose period holds the dates. The port has the caller name it, and checks that it fits. A `uses` could choose it, but `uses` belongs on a part, and an application is a root.

**Smaller frictions.**
- A required reference could be left out. `approver` has no default, but creating an application without one succeeded, and the gate refused it only because `its approver's user` was unknown. Since slice 29 an attribute with no default is required, and the port drops that condition.
- No admit can mix `and` and `or`, so each "unless the company allows it" became its own `define` (`permitted`, `timely`), and self-approval needs two admits.
- An expectation about a calculation (`expect AT's leaves - the total days … is 26`) is refused: an `expect` is about one unit.

## After slice 28: spans

Slice 28 lets an entity count toward every day it spans: `rule apart: for each Employee and day from their from to their to, the number of their Applications that are held is at most 1`, with `held` meaning open or approved. That is hrms's overlap check, in two lines against 38. An overlapping application is refused at the gate by rule `apart`, and a rejected or cancelled one gives its days back. The same form is a capacity (at most 2 bookings of a room a day). `test_overlap` is now a scenario. The four half-day overlap tests still wait for half days, and consecutive leave across applications still waits: a run is not a total of any one day.

## Next

Slices from these gaps, most valuable first:
1. runs across entities (consecutive leave);
2. a way to keep a total that only a reader or an `expect` uses;
3. days between dates.
