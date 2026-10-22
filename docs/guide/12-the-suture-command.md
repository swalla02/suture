# 12. The `suture` command

```text
suture check   ⟨dir⟩ [--json]
suture build   ⟨dir⟩
suture run     ⟨dir⟩ --scenario ⟨file⟩ [--models]
suture explain ⟨dir⟩ --scenario ⟨file⟩ ⟨name⟩
suture init    ⟨dir⟩
suture help    [⟨topic⟩ | all]
```

`⟨dir⟩` is the directory that holds the program's `.st` files. The command exits with 0 on success, 1 when a scenario fails (or, for `check`, anything fails), and 2 when the program does not compile or the arguments are wrong.

## `suture check`

`check` is the loop to run after every edit. It compiles the module, runs every scenario of every `.scenario` file in the directory, and keeps the output short: every compile error with its file and line, `✓` and the title for each passing scenario, and the whole trace and the failures only for a failing one. The last line counts them:

```text
✓ rooms.scenario: "a booking that does not fit is refused"
✗ rooms.scenario: "a cancelled booking frees the room"
    …the trace…
  ✗ expected B1 is cancelled
2 scenarios: 1 passed, 1 failed
```

`--json` prints the same as one JSON object: `passed`, `errors` (each with `file`, `line` and `message`), `warnings`, and `scenarios` (each with `file`, `title`, `passed`, `failures`, and the `trace` when it failed).

## Working with a coding agent

Most code is now written by agents, and an agent has never seen Suture before. It doesn't need this guide:

- **`suture help all`** prints the agent book, docs/agents/suture.md: the whole language in about 4k tokens, meant to be read once and kept in context. It has the loop, one module that uses most of the language, every declaration and sentence form, what the runtime does, a table from requirements to forms (`recipes`), and what cannot be written yet with what to write instead (`limits`). `suture help` lists its sections, and `suture help ⟨section⟩` prints one. Every example in it compiles and every scenario passes; the test suite checks it, and fails if the book grows past 20 KB.
- **`suture init ⟨dir⟩`** writes an `AGENTS.md` with the loop (read the book, write, check, build), a `CLAUDE.md` that points to it, and, if the directory has no module yet, a small module and scenario that pass. It never overwrites a file.
- **`suture check ⟨dir⟩`**, above, keeps the agent's loop short.

There is no MCP server: an agent with a shell needs none, and tool descriptions would cost context on every turn.

For Claude Code across many modules, copy `docs/agents/skill/suture` to `~/.claude/skills/suture`. The skill says the same as `AGENTS.md`.

The person reviews what the agent wrote through `suture build`'s read-backs and the scenarios, not through the syntax.

## `suture build`

`build` compiles every `.st` file in the directory, writes the compiled program next to them as `⟨module⟩-v⟨version⟩.sut` (JSON data, not code: the runtime interprets it), and prints what it understood, in this order:

1. **The summary:** the module, the program file, and how many kinds, effects, rules and goals there are.
2. **Warnings,** such as a slot or a `uses` with no `prefer`.
3. **Every rule's read-back:** the sentence, `understood as` (the formula), `in other words` (a paraphrase generated from the formula), and `cost` (the tier; chapter 7).
4. **`choices:`** every binding with its pool key, how many candidates it looks at, and how they are ranked; and every relation across units that adds to a running total (chapter 9).
5. **`roles:`, `admitted:` and `watches:`** the roles, every `admit`, and every watched attribute with who carries it (chapter 10).

Read the build output after every change. The read-backs are the cheapest test there is: if a paraphrase does not say what you meant, nothing else will go right either.

## `suture run`

`run` compiles the program, then runs every scenario in the file and prints its trace. With `--models`, the model backends in the directory's `models.json` answer the models that a scenario does not script (chapter 15). Each line is one thing the runtime did:

| Line | Means |
|---|---|
| `B1  placed: …` | an entity was created, with its values |
| `gate: admitted: fan Ada books a new Booking B1` | an input passed the gate |
| `gate: refused at the gate: …: ⟨reason⟩` | an input was refused, and why |
| `B1  needs Payment` | a part was created for a need |
| `B1  Delivery: trying Shipment` | a slot filled or moved on |
| `B1  Payment → charge via payments (amount 80 EUR)` | a request went out, with exactly the fields sent |
| `choice: Fri for B1/Ticket#1@Session` | a binding picked a candidate |
| `Fri granted …: 2 quantity taken` | the candidate took the amount |
| `Fri refused …: rule seats: …` | the candidate refused, with the rule's paraphrase |
| `B1  Ticket waits: rule early` | a step was held back by a rule |
| `payments: charged (B1/Payment#1)` | an adapter answered |
| `B1  Ticket: no answer within 10 minutes: asking again (send 2 of 3)` | a deadline fired, and the request was re-sent with the same key |
| `B1  Ticket timed out after 3 sends` | the last send got no answer |
| `B1  alert: not met within 1 day: …` then `because …` | a goal missed its deadline, with the reason |
| `B1  goal met: …` | a goal is met |
| `… N pass` | the clock moved |
| `✓ expect …`, `✗ …` | a check passed or failed |

## `suture explain`

`explain` runs the same scenarios and, at the end of each scenario that has an entity called `⟨name⟩`, explains it:

```text
  Order M2 (cancelled no, country IR, customer Ira, total 450 EUR)
    goal not met yet: every Order that is not cancelled is paid and settled (not yet settled)
    Delivery: Refund (2 of 2)
    Line: done — price 30 EUR, product Kettle, quantity 15
    Payment: captured — amount 450 EUR
    Refund: not requested yet — amount 450 EUR, customer Ira
  gate: admitted: customer Ira places a new Order M2
```

It shows the entity's values, each goal and whether it is met, each part with its state, attempt, pending request and deadline, each slot's current alternative, any alert, and the gate's decisions about the entity. It is computed from the runtime's own state, so it always agrees with what the runtime does. The trace just above it says why the Refund is not requested: `Customer Ira refused …: rule monthly`.

## `suture serve`

```sh
suture serve examples/tickets --listen 127.0.0.1:7878
```

runs the program as a service: inputs over HTTP, requests posted to the adapters in `adapters.json`, the real clock, and a log in the state directory (`⟨dir⟩/.suture` by default) that a restart replays. Chapter 17 covers it. With `--pod ⟨name⟩`, several processes serve one state directory together (chapter 20).

## `suture deploy`

```sh
suture deploy examples/tickets
```

checks a new version of the program against the live units in the state directory, and prints what the deploy would do, or the breaks that need a resolution first. It never writes, and it exits with failure if the deploy would be refused. Chapter 18 covers it.

## Errors

Every error has the file and line, and a fixed template. The compiler reports every statement it cannot read, not just the first. Errors that need the whole program (unknown names, types, tiers) are reported once every statement reads.

### A sentence that matches no pattern

When a rule matches no pattern, the error names the **closest pattern**: the one with the fewest differing words. Where it can, it also fills that pattern with your words and the program's declarations, and shows the result as **did you mean**. A suggestion is shown only if the compiler has compiled it in place of your rule, so it never suggests something that does not work. It must also keep every `no` and `not` you wrote and everything after your `unless` or comparison, so it never quietly changes what you meant.

```suture fails
module shop version 1, currency EUR
kind Order  { total: Money, paid: Bool = no }
kind Refund { order: Order, amount: Money }
Order needs Refund
rule cap: refunds of an Order can't be more than its total
```

```errors
main.st:5: rule cap: "can't be more than" is not a Suture phrase
  closest pattern:  the total ⟨attr⟩ of each ⟨Kind⟩'s ⟨Kinds⟩ is at most ⟨num⟩
  did you mean:     the total amount of each Order's Refunds is at most its total
```

Here the compiler saw that `refunds` is the plural of Refund and `Order` is a kind, and chose `amount` because it is Refund's Money attribute.

### Phrases that are not Suture

Every meaning has one phrasing. A fixed table knows common alternatives and points to the canonical phrase:

| You wrote | Suture says |
|---|---|
| can't be more than, may not exceed, must be at most, is no more than, is less than or equal to | is at most |
| can't be less than, must be at least, is no less than, is greater than or equal to | is at least |
| is greater than, is bigger than, exceeds | is more than |
| is smaller than, is fewer than | is less than |
| equals, is equal to, must be, should be | is |
| isn't, must not be, can't be | is not |
| doesn't have, may not have, cannot have | has no |
| may have, must have, can have | has |
| if it isn't | unless it is |
| the sum of | the total |
| the count of, how many | the number of |
| for every | for each |

```suture fails
module shop version 1, currency EUR
kind Order    { total: Money, paid: Bool = no }
kind Shipment { order: Order }
Order needs Shipment
effect send Shipment via carriers within 7 days
  sends    the order's total
  marks    sent
  reports  delivered
rule paid: no Order may have a Shipment that is sent if it isn't paid
```

```errors
main.st:9: rule paid: "may have" and "if it isn't" are not Suture phrases
  closest pattern:  no ⟨Kind⟩ has a ⟨Kind⟩ that is ⟨state⟩ unless ⟨cond⟩
  did you mean:     no Order has a Shipment that is sent unless it is paid
```

The table is used only for messages. A program never contains these phrases, so two sentences that look different always mean different things.

### Misspelt names

An unknown kind, attribute or state that is at most two letters away from exactly one declared name gets a hint:

```suture fails
module shop version 1, currency EUR
kind Order    { total: Money, paid: Bool = no }
kind Shipment { order: Order }
Order needs Shipment
rule big: every Order's totl is at most 1000 EUR
rule ship: no Order has a Shipmnet unless it is payd
```

```errors
main.st:5: an Order has no attribute, part or owner called `totl`; did you mean total?
main.st:6: an Order does not need a Shipmnet; did you mean Shipment?
```

The compiler stops at the first unknown name in a sentence, so `payd` is reported once `Shipmnet` is fixed.

### Everything else

Other errors say what is wrong in the terms of the program, and usually what to write instead:

```suture fails
module shop version 1, currency EUR
kind Order  { total: Money }
kind Refund { order: Order, amount: Money }
Order needs Refund
rule all: the total amount of all Refunds is at most 1000000 EUR
rule mix: no Order has a Refund unless its total is at least 10 EUR and it is done or it is failed
```

```errors
main.st:6: rule mix: one sentence cannot mix `and` with `or`; name a part with `define`
  closest pattern:  no ⟨Kind⟩ has a ⟨Kind⟩ unless ⟨cond⟩
```

Fixing `mix` lets the compiler reach the checks that need the whole program, and the next build reports `all`: ``rule all: `all Refunds` has no group key, so one total would hold up every entity it counts; end the rule with `, checked globally` ``.
