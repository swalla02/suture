# 11. Scenarios

A program is tested with **scenarios**, written in the same controlled English as the program. A scenario builds a small world, sends inputs, plays every outside service, moves the clock, and checks what is true. It is deterministic: the same scenario always gives the same trace, and its journal replays to the same state.

This chapter lists every step form, using a small catering service:

```suture
module catering version 1, currency EUR

role client is a Client
role chef

kind Client  { name: Text }
kind Dish    { name: Text, price: Money }
kind Event   { client: Client, guests: Count, courses: list of Course,
               total: Money = the total of the course's dish's price * guests over its courses,
               cancelled: Bool = no }
kind Course  { event: Event, dish: Dish }
kind Deposit { event: Event, amount: Money = event's total / 2 }
kind Menu    { event: Event }

Event needs Deposit, Menu while it is not cancelled

effect take Deposit via payments within 1 hour
  sends    its amount
  reports  taken with reference: Text | fails with declined
effect print Menu via printer within 1 day
  sends    the event's guests
  marks    printing
  reports  printed

rule paid: no Event has a Menu that is printing unless its Deposit is taken

desired within 2 days: every Event that is not cancelled is ready
define: an Event is ready when its Deposit is taken and its Menu is printed

when a client or chef cancels an Event: the Event is cancelled

admit book a new Event by client where the Event's client is the caller
admit cancel Event by client where the Event's client is the caller
admit cancel Event by chef
admit add a new Dish by chef
admit change Dish's price by chef
```

## The shape

```text
scenario "⟨title⟩":
  given  …
  and    …
  when   …
  and    …
  expect …
```

- A file holds any number of scenarios. Each starts from an empty world.
- Each step is one line, indented under the title. The first word is `given`, `when`, `and` or `expect`; aligning what follows is only for people. A long step continues on a more indented line.
- `and` continues whatever the previous step was: after a `given` it gives, after a `when` it acts.
- `when` and `expect` can alternate as often as needed, so a scenario can check the state halfway.
- Names (`Ada`, `E1`) are the scenario's names for entities. They are used as values (`client Ada`), in conditions (`E1 is ready`), and as callers' names (`client Ada books …`).

## Building the world: `given`

```text
given a ⟨Kind⟩ ⟨Name⟩ [with ⟨attr⟩ ⟨value⟩[, ⟨attr⟩ ⟨value⟩…] [and ⟨attr⟩ ⟨value⟩]]
```

- Values are literals: `price 30 EUR`, `guests 40`, `name "Ada Lovelace"`, `vip yes`, `due 2 days`. A single word may go unquoted: `name Ada`.
- A reference is the other entity's name: `client Ada`. A set is `{North, South}`, or one name.
- `given` is the world, not an input: it never passes the gate. Use it for shared entities and anything that exists before the story starts.

To fill a limit, create many alike in one line. `⟨n⟩ ⟨Kinds⟩ ⟨first⟩ to ⟨last⟩` creates one per name, in order, each with the same values. The kind is written in its plural, and the names share a prefix and count up by one:

```text
given  14 Talks T1 to T14 with program P1 and length 45
when   member Ada books 3 new Bookings B1 to B3 with class Yoga
```

A range cannot be refused as a whole. Create the one that must be refused on a line of its own.

## Inputs: `when`

| Step | Means |
|---|---|
| `when a new ⟨Kind⟩ ⟨Name⟩ with …` | creates a root, with no caller (only in a program with no `admit`) |
| `when ⟨role⟩ ⟨Name⟩ ⟨verb⟩s a new ⟨Kind⟩ ⟨Name⟩ with …` | an admitted creation |
| `when ⟨role⟩ ⟨Name⟩ ⟨verb⟩s ⟨n⟩ new ⟨Kinds⟩ ⟨A1⟩ to ⟨An⟩ with …` | n admitted creations, one per name |
| `  and a ⟨element⟩ with …` | an element of the new entity's list, on the next line |
| `when ⟨role⟩ ⟨Name⟩ ⟨verb⟩s ⟨Name⟩['s ⟨Part⟩] [with …]` | an admitted event, to the entity or its part, with its data |
| `when ⟨role⟩ ⟨Name⟩ changes ⟨Name⟩'s ⟨attr⟩ to ⟨value⟩` | an admitted change |
| `… is refused at the gate` | (on the next line) this input must be refused at the gate |
| `… is refused by rule ⟨name⟩` | this input must be refused by that rule |

## The outside world: `answers` and `pass`

| Step | Means |
|---|---|
| `when ⟨adapter⟩ answers ⟨outcome⟩` | the oldest pending request of that adapter gets that outcome |
| `… answers ⟨outcome⟩ with ⟨attr⟩ ⟨value⟩[, …]` | with the outcome's data |
| `… answers ⟨outcome⟩ for ⟨Name⟩` | the request of that entity, when several are pending |
| `when ⟨n⟩ ⟨unit⟩ pass` | moves the clock: `30 seconds`, `5 minutes`, `2 hours`, `3 days`, `1 week` (singular or plural) |
| `when it is ⟨date⟩` | moves the clock forward to `2026-03-01` or `2026-03-01 14:30`, in UTC; a date already past fails the step |

Moving the clock fires every timer that falls due, in order: deadlines (re-sends and time-outs), slot patience, and goal deadlines. The scenario clock starts at 2026-01-01 00:00 UTC. A date can also be given as a value, `with opens 2026-03-01`, or compared in an `expect`, `expect O1's created is at least 2026-02-01`. A program cannot write one, because a served program's clock starts when its log does (chapter 17).

## Checks: `expect`

| Step | Checks |
|---|---|
| `expect ⟨cond⟩` | any condition, with a name standing for the subject: `expect E1 is ready`, `expect E1's total is 900 EUR`, `expect the number of E1's Courses is 2`, `expect E1 has no Menu` |
| `expect ⟨Name⟩'s ⟨Kind⟩ is [not] ⟨state⟩` | the entity's current part of that kind: `expect E1's Deposit is timed out` |
| `expect an alert for ⟨Name⟩`, `expect no alert for ⟨Name⟩` | whether a goal raised an alert |
| `expect the total ⟨attr⟩ taken from ⟨Name⟩ is ⟨num⟩` | a running total (chapter 9) |

A total across units, such as `the total days of AT's Applications that are approved`, is kept only when a rule, need, admit, definition or formula of the program reads it, or a `keep` names it. An `expect` about one the program does not keep fails the step with `the program keeps no total … of each Allocation`, instead of reading 0.

`keep` keeps a total that no rule bounds, for scenarios and readers:

```suture
keep the total guests of each Client's Events
keep the number of each Client's Events
```

The client keeps the totals as its events change, as it would for a rule over them, and `expect the total guests of C1's Events is 120` reads them. `suture explain` and `GET /units/⟨label⟩` show each one: `its total guests of Events: 120`. A `keep` names a total of each entity across units. Anything else is an error, such as `keep the total guests of all Events`.

A failed `expect` does not stop the scenario: every later step still runs, and the report lists every failure.

## A complete example

```scenario
scenario "a dinner for 30 is booked, paid and printed":
  given  a Client Ada with name Ada
  and    a Dish Soup with name Soup and price 8 EUR
  and    a Dish Fish with name Fish and price 22 EUR
  when   client Ada books a new Event E1 with client Ada and guests 30
         and a course with dish Soup
         and a course with dish Fish
  expect E1's total is 900 EUR
  expect E1's Menu is not printing
  when   payments answers taken with reference "dep-1"
  expect E1's Deposit's reference is "dep-1"
  when   printer answers printed
  expect E1 is ready
  expect no alert for E1

scenario "a declined deposit is never printed, and the goal alerts":
  given  a Client Ada with name Ada
  and    a Dish Soup with name Soup and price 8 EUR
  when   client Ada books a new Event E1 with client Ada and guests 10
         and a course with dish Soup
  and    payments answers declined
  and    payments answers declined
  and    payments answers declined
  and    2 days pass
  expect E1 has no Menu that is printing
  expect an alert for E1

scenario "two events, answered out of order":
  given  a Client Ada with name Ada
  and    a Dish Soup with name Soup and price 8 EUR
  when   client Ada books a new Event E1 with client Ada and guests 10
         and a course with dish Soup
  and    client Ada books a new Event E2 with client Ada and guests 20
         and a course with dish Soup
  and    payments answers taken with reference "b" for E2
  and    payments answers taken with reference "a" for E1
  expect E1's Deposit's reference is "a"
  expect E2's Deposit's reference is "b"

scenario "only the chef sets prices, and only the client or the chef cancels":
  given  a Client Ada with name Ada
  and    a Client Bea with name Bea
  and    a Dish Soup with name Soup and price 8 EUR
  when   client Ada changes Soup's price to 1 EUR
         is refused at the gate
  and    chef Cy changes Soup's price to 9 EUR
  expect Soup's price is 9 EUR
  when   client Ada books a new Event E1 with client Ada and guests 10
         and a course with dish Soup
  and    client Bea cancels E1
         is refused at the gate
  and    client Ada cancels E1
  expect E1 is cancelled
  expect E1 has no Menu
```

## Running scenarios

```sh
suture run ⟨program dir⟩ --scenario ⟨file⟩
```

prints each scenario's trace, a `✓` for each passing check and a `✗` for each failure, and `passed` or `FAILED`. The command exits with status 0 only if every scenario passed. `suture explain` runs the same scenarios and then explains one entity (chapter 12).

## Writing good scenarios

- **One story per scenario,** titled with what it shows: "a declined deposit is never printed".
- **Script the misbehaving world.** Most of the value is in the unhappy paths: declines, silence, duplicates, late answers, cancellations at the worst moment.
- **Check the invariants, not just the outcome.** `expect the total quantity taken from S1 is 0` after a cancellation says more than `expect O1 is cancelled`.
- **Check halfway.** An `expect` between two `when`s pins down the waiting states, which is where bugs hide in conventional code.
