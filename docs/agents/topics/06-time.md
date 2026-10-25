# time: deadlines, periods, goals, and times that pass

```text
rule ⟨name⟩: for each ⟨Kind⟩ and day|week|month|year [from their ⟨attr⟩ to their ⟨attr⟩], ⟨number rule about their ⟨Kinds⟩⟩
desired [within ⟨duration⟩]: every ⟨subject⟩ is ⟨state⟩[ and ⟨state⟩]   alert if not met in time
when a ⟨Kind⟩'s ⟨Time attr⟩ passes: ⟨action⟩[ and ⟨action⟩]          the clock reaches it
calendar ⟨name⟩: ⟨Day⟩ to ⟨Day⟩ | ⟨Day⟩, ⟨Day⟩ and ⟨Day⟩ | every day[, 9:00 to 17:00][, except ⟨date⟩, ⟨date⟩ and ⟨date⟩]
```

- **No `now`.** A fact never changes while nothing happens; time acts only through deadlines, `after`, `desired within`, periods and `… passes` (once per value; only an admitted change moves it).
- **Everything is journaled** and replays to the same state.
- **A time passes** when the clock reaches a `Time` attribute of an entity that is no part, given or a formula such as `due: Time = created + 14 days`. A time already past passes at creation. Only an admitted change may move it; the old time then does nothing. Guard what it sets with a `define` (`late when it is overdue and it is not returned`).
- **Periods** count by the calendar day, week (from Monday), month or year, in UTC, of each entity's `created`, or of every day `from their ⟨time⟩ to their ⟨time⟩` spans.
- **Goals.** `desired within 30 days: every Talk is decided` raises an alert for each Talk not decided 30 days after it is created.
- **Days between dates:** `the days from its from to its to` counts calendar dates in UTC, both ends (4 for Jan 5–8).
- **Working time:** `the work time from its from to its to` is a `Duration`: the hours calendar `work` works between the two times, without its days off. An end at midnight takes that whole day, so `from 2026-03-02 to 2026-03-06` is 5 working days (40 hours at 9:00–17:00), and `from 2026-03-09 9:00 to 2026-03-09 13:00` is a half day (4 hours). Durations add up in totals: `the total length of each Employee's Leaves is at most its allowed`. Without hours, a calendar works whole days (24 hours each).
- **Working time per day:** in a span rule, `the total work time of their Leaves` adds each Leave's working time within that day, not its whole length: `rule apart: for each Employee and day from their from to their to, the total work time of their Leaves is at most 8 hours` lets a morning and an afternoon share a date, and refuses a third half day.
