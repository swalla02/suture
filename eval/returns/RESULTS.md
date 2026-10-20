# Returns: Suture against Solidus

The second measured comparison, and the first against code its designer did not write. The returns flow of [Solidus](https://github.com/solidusio/solidus), an open-source Rails shop, was ported to Suture (`REQUIREMENTS.md` says what was ported and what was not). Then two real changes from Solidus's history were made to the port, and compared with what they cost Solidus.

`python3 count.py ⟨solidus checkout⟩ ⟨solidus commit⟩ [⟨suture commit⟩]` gives the numbers below.

## Numbers

| | Suture | Solidus |
|---|---|---|
| First version (Solidus `2741034ae^`): lines of logic | 73 | 656, of which 282 are `end`, `module`, `class`, `include` and `private` lines (374 without them) |
| Control-flow statements (`if`, `unless`, `case`, `return`, `.each`, …) | none: the language has none | 47 (not counting `&&`, `\|\|`, `.all?` or `.any?`) |
| Condition and fallback clauses (`unless`, `where`, `while`, `after`, …) | 21 | (inside the 47) |
| Change 1, `2741034ae`, a return item's unit must be on the RMA's order | **+1 −1** (a `where` on the gate) | +7 −0 counted (+9 raw): a validation, its registration, and a guard |
| Change 2, `65c411211`, an `in_transit` reception state | +4 −2 | **+2 −1** |
| After both | 75 | 663 |
| Scenarios | 18, in 201 lines | Solidus's own specs, which were not run against the port |

Solidus's count is over the 23 files that do what the port does. It leaves out, by name and pattern, the lines and methods for exchanges, taxes, simulation, the tolerance check and `errored`, splitting refunds over payments, the override type, display, generated numbers and admin search. The filter is coarse: a few lines either way. Rails gives Solidus a database, transactions, an admin and an API. Suture gives the port:
- a durable log and replay;
- deadlines and retries on refunds, store credits and restocks (Solidus refunds synchronously);
- the read-back of every rule, and explanations.

## What we learned

**Where it wins.**
- The first version is between a ninth and a fifth of the size: 73 lines against 656, or against 374 without Ruby's `end`, `module` and `class` lines. It has no control flow.
- Change 1 is one condition on the gate (`… and the Item's unit's order is the Item's rma's order`), where Solidus wrote a validation method, registered it, and guarded it against missing records. The commit message says the bug let a return item cancel another order's return items. In Suture that path could not exist, because a return item cannot touch another unit (see below).

**Where it loses: a new state.** Change 2 cost Suture twice what it cost Solidus. Solidus has a state machine: a new state is a word in a list and an event. Suture has no states with values: each reception status is a Bool, which needs:
- an attribute;
- a place in the `back` define;
- an `admit`;
- a reaction.

Nothing in the language says that the statuses exclude each other: the port keeps them apart with `where the Item is awaiting` on each `admit`. A state form, `reception is awaiting, received, given, lost, …, in_transit`, would make that a one-word change, and make the exclusion a fact the compiler knows.

**What Suture could not express at all.**
- **Three Solidus behaviours were not ported, so the port's line count looks better than it is.** Each one reads the other return items of the same unit:
  - a new return item cancels the unit's other active return items;
  - a second completed return item for the unit is refused;
  - a unit with an earlier reimbursed return item is ineligible.
  
  In Suture, each return item is its own unit, and v0 has no `that` filter on a relation across units (guide chapter 16). This is the biggest gap. Many real workflows have the shape "an X per thing, and at most one live X per thing".
- **For the same reason:**
  - an RMA cannot check that its order has shipped units, so the order carries `shipped` as a given fact;
  - cancelling an RMA cannot be refused because one of its items is past cancelling.
- **Money × Money.** The promotion share is adjustments × price ÷ item total. Suture refuses to multiply Money by Money, so each unit carries its `discount`.
- **Small gaps in literals.** There is no negative Money literal (a promotion is −3 EUR in Solidus), and no Time literal in scenarios. The port adds an order when it completes, and uses `created`.

**A trap the author fell into.** `rule window: no Item has a Refund that is sent unless its Refund's created - its unit's created is at most 90 days`. This is the natural way to say "refund only within 90 days of shipping", and the facts in it never change. The author expected a refund outside the window to move on to store credit at once. Instead it waited forever, for two reasons:
- the slot moves on at once only when a rule forbids the part *existing* (`has a Refund`), not its being *sent*;
- a path through a part (`its Refund's created`) never counts as fixed.

The same happened to `rule wanted` and `rule kept`, about a preferred payment. The port works around it with patience (`then Credit after 1 minute`), and three scenarios carry a `1 minute passes` for it. Nothing warned the author: the read-back said nothing about it, and the trace said `Refund waits: rule window`.

**Implicit policy, again.** A refund the payments adapter declines is tried 3 times, and then paid in store credit (unless a refund was preferred), by the slot's policy. In Solidus, the reimbursement becomes `errored`, and staff retry it. Neither the requirements nor the author chose Suture's behaviour. The slice-22 read-back now prints it, which is how it was noticed.

## After slice 24

Slice 24 (spec appendix Y) fixes the trap. A rule on facts that never change now moves a slot on when it refuses a part's send, and a refused part's own `created` counts as fixed. So `rule window` moves a late refund on to store credit at once:
- the port drops `after 1 minute` from `prefer Payback`, and its scenarios drop the three `1 minute passes`;
- `suture build` now warns when a fixed refusal leaves a part waiting for good. It warned about `rule located`, whose Restock waited for ever at a location that does not restock. The port now says that on the need (`Item needs Restock while … and its rma's location is restocks`), and the rule is gone (−1 line);
- it also warns about `rule kept`: a preferred refund past 90 days is not paid at all. That is what Solidus does, so the warning stays.

The port is now 74 lines with 20 clauses, after both changes, and its 18 scenarios take 198 lines.

## After slice 25

Slice 25 (spec appendix Z) adds state attributes. The port's six reception Bools and `expired` became one attribute, `reception: awaiting | received | given | lost | wrong | short | expired`. The defines got shorter: `back` is `not awaiting and not expired`, and the five reception admits use a new define, `open` (awaiting, and its RMA not cancelled). The first version is then 72 lines.

Change 2 replayed on it:
- `| transit` in the list (+1 −1);
- `admit forward …` (+1);
- `when a staff forwards an Item: the Item is transit` (+1).

That is **+3 −1**, against +4 −2 before and Solidus's +2 −1. `back` needs no change now, and the compiler knows the states exclude each other: a lost item that is received is no longer lost, which the lost-item scenario now checks. Suture still costs one line more than Solidus. Solidus declares an event and its transition in one place, while Suture needs an `admit` (who may, and from which states) and a reaction (what it does). Merging the two would close the gap.

After both changes, the port is 74 lines, and its 18 scenarios take 200 lines.

## How far to trust this

- **Better than eval/claims:**
  - the baseline was written by Solidus's contributors over ten years, not by Suture's designer;
  - the changes are real commits, chosen from the history of these files before the port was measured, the latest behavioural change and one small 2015 feature.
- **Still weak:**
  - Suture's designer chose the workflow and wrote the port;
  - two changes, both small;
  - Solidus's specs were ported as 18 scenarios, not run, so the port matches Solidus only as far as those reach. A first review against the Solidus code found four places where the port had misread it: the `manual_intervention_required` outcome, a preferred refund past 90 days, the 365-day boundary, and an RMA that could be un-cancelled. All four were fixed in the first version before the changes were replayed;
  - Solidus's lines include Rails declarations that a port elsewhere would also need, and its counted files still have code for the gateway, admin and record shapes;
  - the port leaves out three behaviours it cannot express, which makes its line count look better than it is.
- A third party writing a workflow they run is still the stronger test.

## What it suggests next

1. Move on at once when a rule on fixed facts holds a part's send, not only its creation, and count a part's `created` (and any attribute of a part, which v0 never changes) as fixed. That is the trap above. Done in slice 24.
2. A state form for kinds: `reception is awaiting, received, …`, with transitions on events. That is change 2. Done in slice 25, without transitions.
3. Relations across units with a filter, at least `the number of the Unit's Items that are received`, kept as a running total like `taking`. That is the three unported behaviours.
4. Money ratios (`a * b / c` over Money), negative Money literals, and Time literals in scenarios.
