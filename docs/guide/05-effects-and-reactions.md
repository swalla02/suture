# 5. Effects and reactions

Everything a program does to the outside world goes through an **effect**: a request to a service, which answers with one of a fixed set of outcomes. Everything a program does in response to an outcome is a **reaction**. This chapter uses insurance claims: each claim is assessed, and an accepted claim is paid out.

## Effects

```suture
module claims version 1, currency EUR

kind Holder     { name: Text }
kind Claim      { holder: Holder, asked: Money, approved: Money = 0 EUR, closed: Bool = no }
kind Assessment { claim: Claim }
kind Payout     { claim: Claim, amount: Money = claim's approved }

Claim needs Assessment while it is not closed
Claim needs Payout while its Assessment is accepted

effect assess Assessment via assessors within 2 days
  sends    the claim's asked
  reports  accepted with offer: Money | fails with rejected
effect pay Payout via bank within 1 hour
  sends    its amount, the claim's holder's name
  marks    sent
  reports  paid

when an Assessment is accepted: its Claim's approved becomes its offer
when an Assessment is rejected: its Claim is closed

rule offer: every Claim's approved is at most its asked
```

The general form:

```text
effect ⟨verb⟩ ⟨Kind⟩ via ⟨adapter⟩ within ⟨duration⟩
  sends    ⟨field⟩[, ⟨field⟩…]
  [marks   ⟨state⟩]
  reports  ⟨outcome⟩ [with ⟨attr⟩: ⟨type⟩, …] [| fails with ⟨outcome⟩[, ⟨outcome⟩…]]
```

- **`via ⟨adapter⟩`** names the outside service. An adapter is the only code outside the program, and it sees only what its effects send.
- **`within ⟨duration⟩`** is required: every request has a deadline.
- **`sends`** is required: the exact fields the adapter receives, as noun phrases read from the entity (`its amount`, `the claim's holder's name`). Nothing else leaves the system.
- **`marks ⟨state⟩`** names the state the entity is in from the moment it is requested (`sent`). Rules use it to say what may not go out yet.
- **`reports`** lists the success outcome first, then `| fails with` and the failure outcomes. The success outcome can carry data: `accepted with offer: Money` stores `offer` as an attribute of the Assessment. You do not declare it in the kind.

Each outcome name, the mark, and `timed out` become states of the kind, so a sentence can say `its Assessment is accepted` or `a Payout that is sent`.

## When a request goes out

A request goes out once its entity is ready (chapter 4) and the request step passes every rule. In this program a Payout only exists once the assessment is accepted, so it cannot be paid before.

## Outcomes

- **Success** makes the entity `done` and stores the outcome's data.
- **A failure outcome** makes the entity `failed`. Reactions to the outcome run first; then the part is replaced if its need is still active (chapter 4).
- **An answer that would break a rule** is refused: the entity keeps its previous state, the trace shows the rule's read-back, and the same request stays open with the same idempotency key, so a corrected answer can still arrive.

```scenario
scenario "an accepted claim is paid what was offered":
  given  a Holder Ada with name Ada
  when   a new Claim C1 with holder Ada and asked 1000 EUR
  and    assessors answers accepted with offer 800 EUR
  and    bank answers paid
  expect C1's approved is 800 EUR
  expect C1's Payout's amount is 800 EUR
  expect C1's Payout is paid

scenario "an offer above what was asked is refused":
  given  a Holder Ada with name Ada
  when   a new Claim C1 with holder Ada and asked 1000 EUR
  and    assessors answers accepted with offer 1500 EUR
  expect C1's approved is 0 EUR
  expect C1 has no Payout
  when   assessors answers accepted with offer 900 EUR
  expect C1's approved is 900 EUR

scenario "a rejected claim is closed, so it is not assessed again":
  given  a Holder Ada with name Ada
  when   a new Claim C1 with holder Ada and asked 1000 EUR
  and    assessors answers rejected
  expect C1 is closed
  expect the number of C1's Assessments is 1
```

In the second scenario the first answer would set `approved` above `asked`, which rule `offer` forbids, so the whole answer is refused. The request is still open, and the second answer is accepted.

In the third, the reaction closes the claim before the runtime looks at replacing the rejected assessment. A closed claim no longer needs one, so none is created.

## Deadlines and time-outs

When the host hands a request to its adapter, it starts a timer for the effect's `within`. If the timer fires with no answer:

1. The request is sent again, with the **same idempotency key**, so a service that did receive it can recognise the duplicate. The trace notes `send 2 of 3`.
2. After `attempts` sends (3 by default) with no answer, the entity fails with outcome **`timed out`**.
3. A timed-out entity is **never replaced automatically**. The request may still have happened, and replacing it could charge someone twice. The need stays unmet, and the owner's goal explains it (chapter 8).

An answer to a request that was sent again is an answer to that request, whichever send it answers. An answer that arrives after the time-out has no open request, and is ignored with a note.

```scenario
scenario "a silent assessor times out after three sends":
  given  a Holder Ada with name Ada
  when   a new Claim C1 with holder Ada and asked 1000 EUR
  and    6 days pass
  expect C1's Assessment is timed out
  expect the number of C1's Assessments is 1
```

## Reactions

```text
when ⟨event⟩: ⟨action⟩[ and ⟨action⟩…]
```

There are three kinds of event:

- **an outcome:** `when a Payout is paid: …`, `when an Assessment is timed out: …`;
- **an admitted event from a caller:** `when a holder cancels a Claim: …` (chapter 10);
- **a time passing:** `when a Loan's due passes: …`, below.

In the reaction, the subject is the entity the event happened to. `its Claim` (or `the Claim`) is its owner, and `its offer` is its own attribute.

There are three actions:

| Action | Meaning |
|---|---|
| `⟨np⟩ is ⟨Bool attribute⟩` | sets it to yes: `its Claim is closed` |
| `⟨np⟩ is not ⟨Bool attribute⟩` | sets it to no |
| `⟨np⟩ is ⟨state⟩` | puts its state attribute in that state: `the Copy is lent` (chapter 3) |
| `⟨np⟩'s ⟨attr⟩ becomes ⟨num or np⟩` | sets a value: `its Claim's approved becomes its offer` |

The types must match. A value that is unknown is not set, and the trace says so. A `Count` never becomes negative.

A reaction is a step like any other. If its actions would break a rule, the step is refused: for an answer, the answer is refused as above; for a caller's event, the caller is told which rule refused it. A reaction to a `timed out` is different, because a time-out is a fact: the time-out is always recorded, and only the reaction is held back (`reaction held: rule …`).

### When a time passes

```suture
kind Member { name: Text }
kind Loan   { member: Member, due: Time = created + 14 days, returned: Bool = no, overdue: Bool = no }

define: a Loan is late when it is overdue and it is not returned

when a Loan's due passes: it is overdue
```

`when a ⟨Kind⟩'s ⟨time⟩ passes` reacts when the clock reaches one of the entity's `Time` attributes, given at creation or computed by a formula. The trace shows `L1  its due passes (2026-01-15 00:00)`. A time already past when the entity is created passes as it is created.

The time may be moved by an admitted change (`admit change Hold's until by librarian`), for a renewal. A change to a later time moves the passing, a change to a time already past passes at once, and the old time then does nothing. No reaction, answer or source may set it: the compiler refuses that. The reaction has no condition, so guard what it sets with a definition, as `late` does here. Only an entity that is no part has times that pass, for now.

Reactions only ever change facts. They never start work directly: work follows from needs. To "send a letter when a claim is rejected", declare that a rejected claim needs a Letter, and give Letter an effect.
