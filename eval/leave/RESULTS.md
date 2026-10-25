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
| `overlap_with_half_day_1`…`_3`, `overlap_with_half_day_on_today` | translated, since slice 62 (below); `_1` as `after` |
| `max_consecutive_leaves_across_leave_applications` | translated, since slice 32 (below) |
| `half_day_date_cannot_be_holiday`, `leave_days_across_two_holiday_lists` | translated, since slice 61 (below) |

## What we learned

**Where it wins.**
- The port is a tenth of the size, with no control flow. Most of hrms's lines are queries that find the allocation, the approved leaves and the balance. In the port these are one rule (`the total days of each Allocation's Applications that are counted is at most its leaves`) and two relation totals.
- Cancelling gives the days back with no code. hrms writes a reverse ledger entry; in the port a cancelled leave stops being `counted`, and its days leave the total.
- Every check in hrms's `validate` is one `define`. The gates read as the policy: an employee applies for their own leave, and only on time; HR applies for anyone, at any time.

**Two bugs in Suture, found by porting and fixed here.**
- **`applies` was not `apply`.** The third person of a verb ending in a consonant and `y` was `applys`, so `employee E applies …` matched no `admit`. Every such verb was broken: apply, deny, carry, copy, reply, verify. Neither earlier eval had one.
- **An `expect` about a total the program does not keep read 0.** `expect the total days of AT's Applications that are approved is 4` failed, saying 0, because no rule read that total, so no unit added to it. It is now an error that says so. The dashboard's totals were kept by two `define`s that nothing else used, as a workaround. Since slice 33 they are two `keep`s (below).

**What Suture could not express.**
- **Overlap, the biggest gap (ported since slice 28, below).** "No two open or approved leaves of one employee overlap" compares a sibling with the reader: `the number of its employee's Applications that are approved and whose to is at least its from and whose from is at most its to`. A `that` filter on a relation reads only the sibling, so it cannot be written. The same gap blocks consecutive leave across applications. Six tests need it. Booking, scheduling and rostering have the same shape.
- **Counting days.** Days between two dates, holidays and half days. Suture had no Duration ÷ Duration and no fractional counts, so the port took `days` as given, and two tests cannot be written. Days between dates came in slice 38 (below); holidays and half days still wait.
- **Finding the allocation.** hrms finds the allocation whose period holds the dates. The port has the caller name it, and checks that it fits. A `uses` could choose it, but `uses` belongs on a part, and an application is a root.

**Smaller frictions.**
- A required reference could be left out. `approver` has no default, but creating an application without one succeeded, and the gate refused it only because `its approver's user` was unknown. Since slice 29 an attribute with no default is required, and the port drops that condition.
- No admit can mix `and` and `or`, so each "unless the company allows it" became its own `define` (`permitted`, `timely`), and self-approval needs two admits.
- An expectation about a calculation (`expect AT's leaves - the total days … is 26`) is refused: an `expect` is about one unit.

## After slice 28: spans

Slice 28 lets an entity count toward every day it spans: `rule apart: for each Employee and day from their from to their to, the number of their Applications that are held is at most 1`, with `held` meaning open or approved. That is hrms's overlap check, in two lines against 38. An overlapping application is refused at the gate by rule `apart`, and a rejected or cancelled one gives its days back. The same form is a capacity (at most 2 bookings of a room a day). `test_overlap` is now a scenario. The four half-day overlap tests still wait for half days, and consecutive leave across applications still waits: a run is not a total of any one day.

## After slice 32: days in a row

Slice 32 adds `the days in a row of their Applications that are held`, read in a rule over the days a span covers (spec appendix AH). hrms's consecutive leave is one rule:

```
rule consecutive: for each Allocation and day from their from to their to, the days in a row of their Applications that are held is at most its type's longest or its type's longest is 0
```

| | Suture | hrms |
|---|---|---|
| Consecutive leave | **+1 −1**: the rule, and the `short` definition it replaces | 49 counted lines: `get_consecutive_leave_details`, with its two recursive lookups |
| After slice 32 | 41 lines of logic, 16 scenarios in 195 lines | 487 lines in 29 functions; 16 tests translated, 23 out of scope, 6 not expressible |

The rule also covers one application longer than its type allows (`test_max_continuous_leaves`), as hrms's `validate_max_days` does. The port groups the run by allocation, because a rule's group is one unit, and hrms groups it by employee and leave type. A run across two allocations of the same type is therefore not joined. The run counts calendar days, and hrms counts leave days less holidays. In the hrms test that is 11 days, and in the port it is 12. Both are over the limit of 10.

## After slice 33: kept totals

The two `define`s that kept the dashboard's totals are now `keep the total days of each Allocation's Applications that are open` and `… that are approved`. That is +2 −2 lines, and the port no longer has two states that nothing reads. `suture explain` now shows an allocation's running totals.

## Next

Slices from these gaps, most valuable first:
1. ~~runs across entities (consecutive leave)~~ slice 32;
2. ~~a way to keep a total that only a reader or an `expect` uses~~ slice 33;
3. ~~days between dates~~ slice 38;
4. holiday lists, so a leave counts working days;
5. half days.

## After slice 38: days between dates

The application's `days` is now computed from its dates, so a caller can no longer give `from 2027-01-05, to 2027-01-08 and days 10`:

```text
kind Application { …, from: Time, to: Time, days: Count = the days from its from to its to, … }
```

Both ends are counted, by calendar date. No line is added: the attribute's line only gets longer. The scenarios no longer give `days`, and all 16 still pass. Of the 33 applications in them, 31 had given the calendar count. The other two, in `test_validate_application_across_allocations`, gave 6 days for 2025-12-26 to 2026-01-05, which hrms counts less holidays; the port counts 11. Both are refused at the gate either way. The two tests that count days (`half_day_date_cannot_be_holiday`, `leave_days_across_two_holiday_lists`) still need holiday lists. hrms's own count is not added to its side, because the port does not do what that function does with holidays.

## After slice 47: weekdays

`the weekdays from … to …` counts Monday to Friday only (spec appendix AW). The port keeps `the days from`, because hrms takes weekends from the holiday list, not from the calendar, and the port has no holiday lists. A port for a company that does not work weekends would write `the weekdays from its from to its to`. Holiday lists and half days still wait: one needs dates as data across units (a holiday calendar), the other a count that is not whole.

## After slice 61: calendars

Slice 61 measures time against a calendar (spec appendix BL). The port's `days` is now the time its calendar works between the dates. It is a `Duration`, and so is the allocation's `leaves`:

```text
calendar leave: every day, except 2026-07-01, 2026-08-09 and 2026-08-10
kind Application { …, days: Duration = the leave time from its from to its to, … }
```

| | Suture | hrms |
|---|---|---|
| Leave days less holidays and half days | **+1**: the `calendar` line; `days` and `leaves` change type, and `more than 0` becomes `more than 0 days` | 39 lines: `get_number_of_leave_days`, `get_holidays`, `validate_half_day_date`, with messages dropped as in the main count (not counting the holiday list lookups in other files) |
| After slice 61 | 42 lines of logic, 18 scenarios in 213 lines | 487 lines in 29 functions, plus those 39; 17 tests translated and 1 after its test, 23 out of scope, 4 not expressible |

- **The two holiday tests are now scenarios.**
  - `half_day_date_cannot_be_holiday`, translated as `after`: a half day on a day off works no time, so `valid` refuses it at the gate, as it refuses any leave that works no time. This is weaker than hrms. The port has no half-day date inside a longer leave, so a leave from 2026-06-30 to 2026-07-01 12:00 is accepted, as one day. hrms refuses it, because its half day falls on a holiday.
  - `leave_days_across_two_holiday_lists`: five days with two days off counts 3.
- **No hours.** A calendar without hours works whole days, so a leave day is 24 hours of `Duration` and `leaves 30 days` reads as hrms's 30 leave days. A half day is half of one: `from 2026-07-02 to 2026-07-02 12:00` is 12 hours, shown as `12 hours`, not `0.5 days`. A company that counts by working hours writes `Monday to Friday, 9:00 to 17:00`, and a half day is then 4 hours.
- **The scenarios changed in their units only.** `leaves 30` is now `leaves 30 days`, and `expect … is 4` is now `… is 4 days`. All 16 earlier scenarios pass without other changes.
- **The holiday list is in the program.** hrms keeps it as data that HR edits and assigns per employee, and by period: the second test assigns two lists. The port has one calendar with fixed dates, so a new year's holidays need a deploy. That is the gap that still waits: days off as data across units.
- **Still not expressible.** The four `overlap_with_half_day` tests. Two half days may share a date in hrms, because a half day is half of a date, not a span of hours. The port's `apart` rule counts applications per day. Counting the time taken per day instead needs a per-day total of working time, not of applications.

## Next, from this round

1. Days off as data (a holiday list as entities, read by a calendar), so they change without a deploy and can differ per employee.
2. A per-day total of working time in span rules (`for each Employee and day from … to …, the total leave time of their Applications … is at most 1 day`). This would make the four half-day overlap tests expressible.
3. Showing a whole-day calendar's durations in days (`0.5 days`).

## After slice 62: working time per day

Slice 62 lets a span rule add each leave's working time within the day, not one per leave (spec appendix BM). `apart` becomes:

```text
rule apart: for each Employee and day from their from to their to, the total leave time of their Applications that are held is at most 8 hours
```

The four `overlap_with_half_day` tests are now scenarios. That makes every hrms test either translated or out of scope: **20 translated, 2 as `after`, 23 out of scope, none not expressible.** The rule's line only changes, so the port is still 42 lines.

- **The calendar has hours now.** A half day has to be a span of the date. With a calendar of whole days, an afternoon would end at midnight, and an end at midnight takes the whole day. So `leave` works 9:00 to 17:00, a half day is 9:00–13:00 or 13:00–17:00, and a leave day is 8 hours. The cost is in the units: `leaves 30` became `leaves 240 hours`, and `expect … is 4` became `… is 32 hours`. HR counts in days. This is the strongest case yet for gap 3 below, a working day as a unit.
- **A half day is at a span's end, not in its middle.** hrms puts `half_day_date` anywhere from the first date to the last. The port's leave is one span, so a half day can only be its first afternoon or its last morning. `_3` keeps its outcomes: its second leave, 4–7 with a half day on the 5th, is refused for the 4th either way, so the port gives it as whole days. `_1` has a leave from the 1st to the 5th with a half day on the 3rd, which is not one span. It is translated `after`: two half days on the 3rd are accepted, and a third is refused.

## Next, from this round

1. A working day as a unit (`30 leave days`, shown as days): the units above are the friction now.
2. Days off as data across units, changed without a deploy.

