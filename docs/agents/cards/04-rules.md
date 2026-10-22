# rules: invariants, totals, definitions and goals

A rule is checked before every change. A step that would break it is refused (or waits, if it is the system's own work), and the trace names the rule. Rules never fire actions; they only allow or refuse.

```suture
module gym version 1, currency EUR

role staff

kind Member  { name: Text, banned: Bool = no }
kind Class   { name: Text, seats: Count }
kind Booking { member: Member, class: Class, cancelled: Bool = no }

define: a Booking is live when it is not cancelled

rule seats:  the number of each Class's Bookings that are live is at most its seats
rule three:  the number of each Member's Bookings that are live is at most 3
rule banned: no Booking whose member is banned is live

when a staff cancels a Booking: the Booking is cancelled

admit add a new Member or Class by staff
admit book a new Booking by staff
admit cancel Booking by staff
admit change Member's banned by staff
```

```scenario
scenario "a fourth live booking is refused, and a cancel makes room":
  given  a Member M with name "Ann"
  and    a Class C with name "Yoga" and seats 10
  when   staff S books a new Booking B1 with member M and class C
  and    staff S books a new Booking B2 with member M and class C
  and    staff S books a new Booking B3 with member M and class C
  and    staff S books a new Booking B4 with member M and class C
         is refused by rule three
  and    staff S cancels B1
  and    staff S books a new Booking B5 with member M and class C
  expect the number of M's Bookings that are live is 3
```

Rule forms (`⟨cmp⟩` is `is at most`, `is at least`, `is more than`, `is less than`):

```text
rule ⟨name⟩: every ⟨subject⟩ is ⟨state⟩
rule ⟨name⟩: every ⟨subject⟩'s ⟨attr⟩ ⟨cmp⟩ ⟨num⟩
rule ⟨name⟩: no ⟨subject⟩ is ⟨state⟩ [unless ⟨cond⟩]
rule ⟨name⟩: no ⟨subject⟩ has a ⟨Kind⟩ [that ⟨cond⟩] [unless ⟨cond⟩]
rule ⟨name⟩: the number of each ⟨Kind⟩'s ⟨Kinds⟩ [that ⟨cond⟩] ⟨cmp⟩ ⟨num⟩
rule ⟨name⟩: the total ⟨attr⟩ of each ⟨Kind⟩'s ⟨Kinds⟩ [that ⟨cond⟩] ⟨cmp⟩ ⟨num⟩
rule ⟨name⟩: for each ⟨Kind⟩ and day|week|month|year, the total ⟨attr⟩ of their ⟨Kinds⟩ ⟨cmp⟩ ⟨num⟩
define: a ⟨Kind⟩ is ⟨state⟩ when ⟨cond⟩
desired within ⟨duration⟩: every ⟨subject⟩ is ⟨state⟩      (raises an alert if not met in time)
keep the number of each ⟨Kind⟩'s ⟨Kinds⟩ [that ⟨cond⟩]     (a total nothing else reads, for scenarios and reads)
```

- A subject is `[⟨state⟩] ⟨Kind⟩ [whose ⟨attr⟩ ⟨predicate⟩] [that ⟨cond⟩]`. Rule names are one lower-case word.
- `each Class's Bookings` counts Bookings whose reference to a Class is that Class: references are how one entity sees many others.
- Conditions join with `and` or `or`, never both in one sentence: name the part with a `define` (`define: a Loan is late when it is overdue and it is not returned`).
- A rule holds at every step, so it also refuses the steps that repair it. If an old wrong state must stay fixable, put the condition in an `admit … where` instead of a rule.
- `suture build` prints each rule's read-back and its cost: local, per group, or global. Prefer local and per-group rules.
