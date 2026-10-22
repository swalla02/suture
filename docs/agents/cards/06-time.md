# time: dates, durations, deadlines, periods, and what "on its own" can mean

There is no `now` in sentences: a fact never changes while nothing happens. Time enters a program only through events the runtime journals:

| Need | Form |
|---|---|
| a date computed from another | `due: Time = created + 14 days` (a formula) |
| the days from one date to another | `days: Count = the days from its from to its to`: calendar dates, both ends counted, so 2027-01-05 to 2027-01-08 is 4 |
| a deadline on outside work | `effect … within 1 day`: re-sent, then `timed out` |
| something that should be true by then | `desired within 3 days: every Order that is open is shipped` raises an alert for each one that is not |
| a fallback after a wait | `prefer Return: Courier, then Collection after 2 days` |
| a limit per period | `rule daily: for each Member and day, the number of their Bookings is at most 2` |
| a limit per day spanned | `for each Employee and day from their from to their to, …` |

**"Becomes overdue on its own"** has no direct form yet. The two ways today:

1. An alert, when someone should be told: `desired within 14 days: every Loan is returned`. It does not change a state, so an `admit` cannot read it.
2. A state, when the program must act on it: give the entity a part whose effect has the period as its deadline and one attempt, and react to its time-out. The request does go out to an adapter (a reminder service is a natural one), and once sent the part is not withdrawn, so guard the state with the fact that ends it:

```suture
module loans version 1, currency EUR, attempts 1

role librarian

kind Member   { name: Text }
kind Loan     { member: Member, returned: Bool = no, overdue: Bool = no }
kind Reminder { loan: Loan }

Loan needs Reminder while it is not returned

effect remind Reminder via reminders within 14 days
  sends    the loan's member's name
  reports  acknowledged

define: a Loan is late when it is overdue and it is not returned

when a Reminder is timed out: its Loan is overdue
when a librarian returns a Loan: the Loan is returned

admit add a new Member by librarian
admit lend a new Loan by librarian
  where the number of the Loan's member's Loans that are late is at most 0
admit return Loan by librarian
```

```scenario
scenario "a loan is late after 14 days, and a late member cannot borrow":
  given  a Member M with name "Ann"
  when   librarian Lee lends a new Loan L1 with member M
  and    15 days pass
  expect L1 is late
  when   librarian Lee lends a new Loan L2 with member M
         is refused at the gate
  when   librarian Lee returns L1
  and    librarian Lee lends a new Loan L3 with member M
  expect L1 is not late
```

In scenarios: `when 3 days pass`, `when it is 2026-03-01`, and dates as values (`with opens 2026-03-01`). The clock starts at 2026-01-01 00:00 UTC. Over the API, times are RFC 3339 and durations ISO 8601 (`suture help api`).
