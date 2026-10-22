# Suture, for agents: the whole language in one read

Suture is a declarative backend language in controlled English: you state facts, rules, who may do what, and what work the system owes the world; the runtime does the rest (ordering, retries, compensation, limits, audit). Every sentence pattern has one meaning. You have not seen it in training, so do not guess forms from other languages: everything that exists is below.

## loop: how to work

1. A module is the `.st` files of one directory; scenarios (tests) are `.scenario` files beside them.
2. After every edit run `suture check <dir>`: it compiles, runs every scenario, prints each error as `file:line: message`, `✓` per passing scenario, and the trace of each failing one. Exit 0 = all good. `--json` for tools.
3. Fix the first error first; errors name the form they expected.
4. Cover each requirement with a scenario, and each limit or permission with a refused input.
5. When it passes, `suture build <dir>` prints every rule in plain English with its cost. Give that to the person: they review the read-back and the scenarios, not the syntax.

## example: one module that uses most of the language

```suture
module studio version 1, currency EUR, attempts 3

role member is a Member
role staff

kind Member  { name: Text, banned: Bool = no }
kind Class   { name: Text, seats: Count, price: Money }
kind Booking { member: Member, class: Class, cancelled: Bool = no,
               price: Money = class's price,
               until: Time = created + 7 days }
kind Payment { booking: Booking }

Booking needs Payment while it is not cancelled

effect charge Payment via payments within 1 hour
  sends    the booking's price
  reports  paid with reference: Text | fails with declined

define: a Booking is live when it is not cancelled
define: a Booking is confirmed when its Payment is paid

rule seats:  the number of each Class's Bookings that are live is at most its seats
rule three:  the number of each Member's Bookings that are live is at most 3
rule banned: no Booking whose member is banned is live

desired within 1 day: every Booking that is live is confirmed

when a member or staff cancels a Booking: the Booking is cancelled
when a Payment is declined: its Booking is cancelled

admit book a new Booking by member where the Booking's member is the caller
admit cancel Booking by member where the Booking's member is the caller
admit cancel Booking by staff
admit add a new Member or Class by staff
admit change Member's banned by staff
```

```scenario
scenario "a member books, pays, and a full class refuses":
  given  a Member Ann with name "Ann"
  and    a Member Bo with name "Bo"
  and    a Class Yoga with name "Yoga", seats 1 and price 12 EUR
  when   member Ann books a new Booking B1 with member Ann and class Yoga
  and    payments answers paid with reference "p-1" for B1
  expect B1 is confirmed
  when   member Bo books a new Booking B2 with member Bo and class Yoga
         is refused by rule seats
  when   member Bo cancels B1
         is refused at the gate
  when   member Ann cancels B1
  and    member Bo books a new Booking B3 with member Bo and class Yoga
  expect the number of Yoga's Bookings that are live is 1

scenario "an unpaid booking raises an alert":
  given  a Member Ann with name "Ann"
  and    a Class Yoga with name "Yoga", seats 5 and price 12 EUR
  when   member Ann books a new Booking B1 with member Ann and class Yoga
  and    2 days pass
  expect an alert for B1
```

## declarations: kinds, types, needs, effects, permissions

```text
module ⟨name⟩ version ⟨n⟩, currency ⟨EUR|USD|…⟩[, attempts ⟨n⟩]       first line, once
role ⟨role⟩ [is a ⟨Kind⟩]                                                callers; `is a` ties them to entities
kind ⟨Kind⟩ [(plural ⟨Plural⟩)] { ⟨attr⟩: ⟨Type⟩ [= ⟨default or formula⟩], … }
⟨Kind⟩ needs ⟨Kind or Slot⟩[, …] [while ⟨cond⟩]                         each one gets its own part
⟨Slot⟩ is ⟨Kind⟩ or ⟨Kind⟩                                               one part, several kinds
prefer ⟨Slot⟩: ⟨Kind⟩, then ⟨Kind⟩ [after ⟨duration⟩]
⟨Kind⟩ uses a ⟨Kind⟩ where ⟨cond⟩[, taking ⟨num⟩]                       take from a shared pool
prefer ⟨Kind⟩: lowest ⟨num⟩ | highest ⟨num⟩ | those ⟨cond⟩[, then …]     how a pool is chosen
effect ⟨verb⟩ ⟨Kind⟩ via ⟨adapter⟩ within ⟨duration⟩                      outside work, with a deadline
  sends    ⟨np⟩[, ⟨np⟩]
  [marks   ⟨state⟩]                                                      true once sent
  reports  ⟨outcome⟩ [with ⟨attr⟩: ⟨Type⟩] [| fails with ⟨outcome⟩[, …]]
admit ⟨verb⟩ a new ⟨Kind⟩[, ⟨Kind⟩ or ⟨Kind⟩] by ⟨role⟩[, ⟨role⟩] [where ⟨cond⟩]
admit ⟨verb⟩ ⟨Kind⟩ by ⟨role⟩ [where ⟨cond⟩]                             an event on an existing one
admit change ⟨Kind⟩'s ⟨attr⟩ by ⟨role⟩ [where ⟨cond⟩]
keep the total ⟨attr⟩ | the number of each ⟨Kind⟩'s ⟨Kinds⟩ [that ⟨cond⟩]  a total for reads and scenarios
```

- **Types:** `Text`, `Bool`, `Count` (whole, ≥ 0), `Money`, `Time`, `Duration`, `Media`, a kind (a reference), `list of ⟨Kind⟩`, `set of ⟨Kind⟩`; a state attribute `status: open | paid | shipped = open`. Every entity has `created: Time`.
- **Literals:** `"text"`, `yes`/`no`, `3`, `9.99 EUR`, `-3 EUR`, `30 seconds|minutes|hours|days|weeks`. A program cannot write a date; scenarios can (`2026-03-01`).
- **Formulas** are computed when the entity is created: `class's price`, `price * quantity`, `created + 14 days`, `the days from its from to its to` (calendar dates, both ends: 4 for Jan 5–8), `the total of ⟨calc⟩ over its ⟨list⟩`.
- **Names:** kinds are Capitalised, attributes, states, verbs and rule names one lower-case word.

## sentences: rules, definitions, goals, reactions, and their parts

```text
rule ⟨name⟩: every ⟨subject⟩ is ⟨state⟩
rule ⟨name⟩: every ⟨subject⟩'s ⟨attr⟩ ⟨cmp⟩ ⟨num⟩
rule ⟨name⟩: no ⟨subject⟩ is ⟨state⟩ [unless ⟨cond⟩]
rule ⟨name⟩: no ⟨subject⟩ has a ⟨Kind⟩ [that ⟨cond⟩] [unless ⟨cond⟩]
rule ⟨name⟩: the number of each ⟨Kind⟩'s ⟨Kinds⟩ [that ⟨cond⟩] ⟨cmp⟩ ⟨num⟩
rule ⟨name⟩: the total ⟨attr⟩ of each ⟨Kind⟩'s ⟨Kinds⟩ [that ⟨cond⟩] ⟨cmp⟩ ⟨num⟩
rule ⟨name⟩: the total ⟨attr⟩ taken from each ⟨Kind⟩ ⟨cmp⟩ ⟨num⟩
rule ⟨name⟩: for each ⟨Kind⟩ and day|week|month|year [from their ⟨attr⟩ to their ⟨attr⟩], ⟨number rule about their ⟨Kinds⟩⟩
define: a ⟨Kind⟩ is ⟨state⟩ when ⟨cond⟩
desired [within ⟨duration⟩]: every ⟨subject⟩ is ⟨state⟩[ and ⟨state⟩]   alert if not met in time
when a ⟨role⟩ [or ⟨role⟩] ⟨verb⟩s a ⟨Kind⟩: ⟨action⟩[ and ⟨action⟩]
when a ⟨Kind⟩ is ⟨outcome | timed out⟩: ⟨action⟩[ and ⟨action⟩]           an effect's outcome
```

- **cmp:** `is at most`, `is at least`, `is more than`, `is less than`.
- **subject:** `[⟨state⟩] ⟨Kind⟩ [whose ⟨attr⟩ ⟨predicate⟩] [that ⟨cond⟩]`.
- **np:** `it`, `its ⟨attr⟩`, `their ⟨attr⟩`, `the ⟨Kind⟩`, `⟨np⟩'s ⟨attr⟩`, `⟨np⟩'s ⟨Kind⟩` (its part), `⟨np⟩'s ⟨Kinds⟩` (those referencing it), `the caller`.
- **cond:** `⟨np⟩ is [not] ⟨state, np or literal⟩`, `⟨num⟩ ⟨cmp⟩ ⟨num⟩`, `⟨np⟩ has [a|no] ⟨Kind⟩ [that ⟨cond⟩]`, `⟨np⟩ contains ⟨np⟩`, joined by `and` or by `or`, never both: name one side with a `define`. After `and`, a bare state shares the subject: `it is cancelled and paid`.
- **num:** an np, a literal, `the number of ⟨np⟩'s ⟨Kinds⟩ [that ⟨cond⟩]`, `the total ⟨attr⟩ of ⟨np⟩'s ⟨Kinds⟩ […]`, `+ - * /`, parentheses.
- **actions:** `⟨np⟩ is ⟨state or Bool⟩`, `⟨np⟩ is not ⟨Bool⟩`, `⟨np⟩'s ⟨attr⟩ becomes ⟨num or np⟩`. Only on the entity the reaction is about, its parts and its owner.

## semantics: what the runtime does

- **Gate.** Every input (create, event, change) is refused unless an `admit` allows the caller's role, with its `where` true of the target and `the caller`.
- **Rules** are checked before every change. An input that would break one is refused, naming the rule. The system's own work (a part, a take from a pool) waits until the rule allows it. A rule also refuses the steps that would repair an old wrong state, so put conditions that must stay fixable in an `admit … where`.
- **Definitions** are always current; a stored status you would keep in sync is a `define` instead.
- **Needs** create parts; an effect sends the part's request, re-sends after `within` up to `attempts`, then the part is `timed out`. When a need's `while` turns false, an unfinished part is withdrawn and gives back what it took; one whose request went out stays and counts.
- **Totals** across references (`each Class's Bookings`) are kept incrementally; `suture build` shows each rule's cost: local, per group, or global. Prefer local and per-group.
- **No `now`.** A fact never changes while nothing happens; time acts only through deadlines, `after`, `desired within` and periods.
- **Everything is journaled** and replays to the same state.

## recipes: requirement → form

| Requirement | Write |
|---|---|
| status that follows from facts | `define: a Booking is confirmed when its Payment is paid` |
| status someone sets | state attribute + `when a staff ships an Order: the Order is shipped` + `admit ship Order by staff` |
| only their own | `admit … by member where the Booking's member is the caller` |
| not their own | `admit approve Expense by manager where the Expense's employee is not the caller` |
| at most N per owner / at most one active | `rule three: the number of each Member's Bookings that are live is at most 3` |
| sum within a budget | `rule budget: the total amount of each Project's Expenses that are approved is at most its budget` |
| blocked while something holds | `admit borrow a new Loan by member where the number of the Loan's member's Loans that are late is at most 0` |
| one of a set of numbers | `define: a Talk is standard when its length is 30 or its length is 45` + `admit … where the Talk is standard` |
| an average at least N | compare totals: `… unless the total score of its Reviews is at least the number of its Reviews * 3` |
| derived from what references it | `define: a Book is lent when the number of its Loans that are open is at least 1` |
| call an outside service | a part with an `effect`; scenarios answer for it |
| undo work on cancel | `needs … while it is not cancelled` |
| try A, else B after a wait | `Return is Courier or Collection` + `prefer Return: Courier, then Collection after 2 days` |
| shared stock | `Hold uses a Title where the Title's name is the Loan's title, taking the Loan's copies` + `rule stock: the total copies taken from each Title is at most its stock` |
| deadline that alerts | `desired within 30 days: every Talk is decided` |
| becomes overdue by itself | a part with `effect remind … within 14 days`, `attempts 1`, and `when a Reminder is timed out: its Loan is overdue`; guard with `define: … late when it is overdue and it is not returned` |
| limit per day or month | `rule daily: for each Member and day, the number of their Bookings is at most 2` |
| number of days between dates | `days: Count = the days from its from to its to` |

```suture
module recipes version 1, currency EUR

role employee is a Employee
role manager is a Employee

kind Employee { name: Text }
kind Project  { name: Text, budget: Money }
kind Expense  { employee: Employee, project: Project, amount: Money,
                status: draft | approved = draft }
kind Book     { title: Text }
kind Loan     { book: Book, employee: Employee, returned: Bool = no }
kind Talk     { title: Text, length: Count, accepted: Bool = no }
kind Review   { talk: Talk, score: Count }

define: a Loan is open when it is not returned
define: a Book is lent when the number of its Loans that are open is at least 1
define: a Talk is standard when its length is 30 or its length is 45

rule budget: the total amount of each Project's Expenses that are approved is at most its budget
rule single: the number of each Book's Loans that are open is at most 1
rule good:   no Talk is accepted unless the total score of its Reviews is at least the number of its Reviews * 3

when a manager approves an Expense: the Expense is approved
when a employee returns a Loan: the Loan is returned
when a manager accepts a Talk: the Talk is accepted

admit submit a new Expense by employee where the Expense's employee is the caller
admit approve Expense by manager where the Expense's employee is not the caller
admit borrow a new Loan by employee where the Loan's employee is the caller
admit return Loan by employee where the Loan's employee is the caller
admit propose a new Talk by employee where the Talk is standard
admit score a new Review by manager
admit accept Talk by manager
admit add a new Employee, Project or Book by manager
```

```scenario
scenario "the recipes hold":
  given  a Employee Ann with name "Ann"
  and    a Employee Bo with name "Bo"
  and    a Project P with name "Web" and budget 500 EUR
  and    a Book K with title "Dune"
  when   employee Ann submits a new Expense E1 with employee Ann, project P and amount 300 EUR
  and    manager Ann approves E1
         is refused at the gate
  and    manager Bo approves E1
  and    employee Ann submits a new Expense E2 with employee Ann, project P and amount 300 EUR
  and    manager Bo approves E2
         is refused by rule budget
  and    employee Bo borrows a new Loan L1 with book K and employee Bo
  expect K is lent
  when   employee Bo returns L1
  expect K is not lent
  when   employee Ann proposes a new Talk T1 with title "Rust" and length 40
         is refused at the gate
  when   employee Ann proposes a new Talk T2 with title "Rust" and length 45
  and    manager Bo scores a new Review R1 with talk T2 and score 2
  and    manager Bo accepts T2
         is refused by rule good
  when   manager Bo scores a new Review R2 with talk T2 and score 5
  and    manager Bo accepts T2
  expect T2 is accepted
```

## limits: not writable yet, and what to do

| Cannot | Instead |
|---|---|
| `now`, "past its due date" in a sentence | the reminder part above, or a `desired within` alert |
| uniqueness of a pair ("reviews a talk at most once") | model the pair as an entity; say duplicates are not prevented |
| a reaction setting another entity it only references | a `define` on that entity, from a total of what references it |
| a negative `Count` (`score - 3` is not −1) | compare totals, or use `Money` |
| holidays and half days in a day count | take working days as an input, and say so |
| `and` with `or` in one sentence | name one side with a `define` |
| taking back a request already sent | guard the state it sets with the fact that ends it |
| loops, functions, generated text or code | none, by design |

Say in your report when a requirement needs one of these.

## scenarios: tests in the same language

```text
scenario "⟨title⟩":
  given  a ⟨Kind⟩ ⟨Name⟩ [with ⟨attr⟩ ⟨value⟩, … and ⟨attr⟩ ⟨value⟩]     no gate
  when   ⟨role⟩ ⟨Caller⟩ ⟨verb⟩s a new ⟨Kind⟩ ⟨Name⟩ with …              through the gate
  when   ⟨role⟩ ⟨Caller⟩ ⟨verb⟩s ⟨Name⟩
  when   ⟨role⟩ ⟨Caller⟩ changes ⟨Name⟩'s ⟨attr⟩ to ⟨value⟩
         is refused at the gate | is refused by rule ⟨name⟩            required for a refusal
  when   ⟨adapter⟩ answers ⟨outcome⟩ [with ⟨attr⟩ ⟨value⟩] for ⟨Name⟩
  when   ⟨n⟩ days pass | when it is 2026-03-01 [14:30]
  when   model "⟨m⟩@1" picks ⟨Option⟩ [at 0.9] | reads ⟨value⟩ [at 0.9] | declines | is late
  and    …
  expect ⟨cond, with names as subjects⟩ | ⟨Name⟩'s ⟨Kind⟩ is [not] ⟨state⟩ | [an|no] alert for ⟨Name⟩
```

The caller of a role with `is a Member` is named by a Member's name. Values: `"text"`, `3`, `9.99 EUR`, `yes`, `2026-03-01`, a name for a reference. The clock starts at 2026-01-01 00:00 UTC; time passing re-sends and times out requests. A failed `expect` does not stop the scenario. `suture explain <dir> --scenario <file> <Name>` shows one entity's states, parts, totals and why a step waits.

## models: optional, and never generative

A decision model picks among options the rules allow; an extraction model reads one typed value. Scenarios script them; `models.json` backs them only with `--models`.

```suture
module claims version 1, currency EUR

role adjuster

kind Claim    { note: Text }
kind Repair   { claim: Claim }
kind Writeoff { claim: Claim }

Claim needs Outcome
Outcome is Repair or Writeoff
prefer Outcome: Repair, then Writeoff
decide Outcome with model "assessor@1" within 2 seconds at least 0.8 sure otherwise Writeoff

admit file a new Claim by adjuster
```

```scenario
scenario "an unsure model leaves the claim to the fallback":
  when   model "assessor@1" picks Repair at 0.6
  and    adjuster Ann files a new Claim C1 with note "roof"
  expect C1 has a Writeoff
```

Also: `decide … with model "a@1" or model "b@1", picked by model "router@1"`, and `source ⟨Kind⟩'s ⟨attr⟩ via model "invoices@1" within 10 seconds` with `sends its invoice`, `asking "…"`, `at least 0.8 sure`, `otherwise use ⟨literal⟩`.

## serve: HTTP and JSON

`suture serve <dir>` serves the module with a durable log; the caller is the `Suture-Caller` header (`member Ann`). `POST /inputs` takes `{"create": {"kind", "label", "verb", "data"}}`, `{"event": {"target", "verb"}}` or `{"change": {"target", "attr", "value"}}` and answers 200, 409 `refused`, or 400. `GET /units/⟨label⟩` explains one entity; `GET /pending` and `POST /answers` serve adapters with no URL in `adapters.json`. JSON values: `Count` an integer, `Money` `"12.50 EUR"`, `Time` RFC 3339 (`"2026-03-01T14:30:00Z"`), `Duration` ISO 8601 (`"PT2H"`, `"P3D"`), a reference its label, a set a list of labels.
