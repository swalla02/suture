# 6. Sentences

Rules, goals, definitions, reactions, `while` conditions, `uses` conditions, `admit` conditions and scenario expectations are all written as sentences in controlled English. Controlled means there is a fixed set of patterns, and each one has exactly one meaning. This chapter is the grammar they share: noun phrases, conditions and numbers. The following chapters say where each kind of sentence goes.

The examples use a support desk:

```suture
module desk version 1, currency EUR

kind Customer { name: Text, region: Text, vip: Bool = no }
kind Case     { customer: Customer, severity: Count, credits: list of Credit,
                escalated: Bool = no,
                value: Money = the total of the credit's amount over its credits }
kind Credit   { case: Case, amount: Money }
kind Reply    { case: Case }
kind Callback { case: Case }

Case needs Reply
Case needs Callback while it is escalated

effect send Reply via mailer within 1 hour
  sends    the case's customer's name
  marks    sent
  reports  delivered | fails with bounced
effect ring Callback via phones within 4 hours
  sends    the case's customer's name
  reports  answered | fails with unanswered

define: a Case is urgent when its severity is at least 3
define: a Case is handled when its Reply is delivered or its Callback is answered
define: a Case is northern when its customer's region is north

rule scale:  every Case's severity is at most 5
rule credit: the total amount of each Case's Credits is at most 200 EUR
rule calls:  the number of each Case's Callbacks is at most 2
rule vip:    no Case has a Callback unless its customer is vip
rule north:  no northern Case has a Callback that is answered unless it is urgent
```

## Subjects

Rules and goals start from a **subject**: `[⟨state⟩] ⟨Kind⟩ [whose ⟨attr⟩ ⟨predicate⟩] [that ⟨cond⟩]`.

- `northern Case`: a state in front.
- `Case whose severity is at least 3`: one of the subject's own attributes, then any predicate. `whose` reads only the subject's own attribute; to filter on something further away (`its customer's region`), `define` a state and put it in front, as `north` does above.
- `Case that is not escalated`: any condition about the subject.

## Noun phrases

A noun phrase names an entity or a value.

| Pattern | Means | Example |
|---|---|---|
| `it` | the sentence's subject | `unless it is urgent` |
| `its ⟨attr⟩`, `their ⟨attr⟩` | an attribute of the subject | `its severity` |
| `⟨np⟩'s ⟨attr⟩` | an attribute of something | `its customer's region` |
| `⟨np⟩'s ⟨Kind⟩` (singular) | a part, or a slot's current alternative: the current attempt | `its Reply` |
| `⟨np⟩'s ⟨Kinds⟩` (plural) | every counting entity of that kind that belongs to it | `each Case's Callbacks` |
| `the ⟨Kind⟩` | the subject if it is that kind; else its owner of that kind; else its unique attribute of that kind | `the Case`, `the Customer` |
| `the ⟨element⟩` | inside `over ⟨list⟩` or `per ⟨list⟩`: the current element | `the credit` |
| `the caller` | inside `admit`: the caller's entity (chapter 10) | |
| literals | | `200 EUR`, `3`, `"north"`, `2 days`, `yes` |

A word after `'s` is looked up in this order: an attribute of the entity (lower-case), one of its parts (a capitalised kind it needs), then its owner, written either as the owner attribute (`case`) or as the owner's kind (`Case`).

### What counts

A relation such as `its Callbacks` includes **every entity that counts**: from its creation until it is withdrawn, or until it fails before any of its effects was requested. Failed attempts that reached the outside world still count. So after two unanswered callbacks, `the number of its Callbacks` is 2. If a bound should leave some out, it says so with a filter: `the number of its Callbacks that are answered`.

## Conditions

| Pattern | Means |
|---|---|
| `⟨np⟩ is ⟨np or literal⟩`, `⟨np⟩ is not …` | equality |
| `⟨np⟩ is ⟨state⟩`, `⟨np⟩ is not ⟨state⟩` | a state: a Bool attribute, a defined state, `done`/`failed`, or an effect's mark or outcome |
| `⟨num⟩ is at most ⟨num⟩`, `is at least`, `is more than`, `is less than` | comparison |
| `⟨np⟩ contains ⟨np⟩`, `⟨np⟩ does not contain ⟨np⟩` | set membership (also `contain`, after a plural) |
| `⟨np⟩ has a ⟨Kind⟩ [that ⟨cond⟩]` | at least one counting entity of that kind |
| `⟨np⟩ has a ⟨Kind⟩ or a ⟨Kind⟩ [that is ⟨state⟩]` | at least one of either; a last `that` is about each, unless an earlier kind has its own |
| `⟨np⟩ has a ⟨Slot⟩ [that is ⟨state⟩]` | at least one of the slot's alternatives, whichever it is |
| `⟨np⟩ has no ⟨Kind⟩ [that ⟨cond⟩]` | none |
| `⟨cond⟩ and ⟨cond⟩`, `⟨cond⟩ or ⟨cond⟩` | both, either |

**Text or state?** In `⟨np⟩ is ⟨word⟩`, a bare word is a text value when `⟨np⟩` is a `Text` attribute (`its customer's region is north`), and a state otherwise (`it is urgent`). Quoting always means text: `is "north"`.

**`and` with `or`.** One sentence cannot mix `and` and `or`, because readers disagree about how they group. Name one part with `define` instead:

```suture fails
module desk version 1, currency EUR
kind Case  { severity: Count, escalated: Bool = no, vip: Bool = no }
kind Reply { case: Case }
Case needs Reply
rule mix: no Case has a Reply unless it is escalated and it is vip or its severity is at least 4
```

```errors
main.st:5: rule mix: one sentence cannot mix `and` with `or`; name a part with `define`
  closest pattern:  no ⟨Kind⟩ has a ⟨Kind⟩ unless ⟨cond⟩
```

```suture
module desk version 1, currency EUR
kind Case  { severity: Count, escalated: Bool = no, vip: Bool = no }
kind Reply { case: Case }
Case needs Reply
define: a Case is important when it is escalated and it is vip
rule mix: no Case has a Reply unless it is important or its severity is at least 4
```

**`that` filters** take one condition. A combined filter is named with `define` as well.

**Unknown values.** A value that is not known yet, such as an answer's data before the answer arrives, never breaks a rule, and makes a condition false. So `its Assessment's offer is at least 100 EUR` is false until the offer exists.

## Numbers

| Pattern | Means |
|---|---|
| `⟨np⟩` | a Count, Money, Duration or Time attribute |
| `the total ⟨attr⟩ of ⟨np⟩'s ⟨Kinds⟩ [that ⟨cond⟩]` | the sum over a relation |
| `the number of ⟨np⟩'s ⟨Kinds⟩ [that ⟨cond⟩]` | the count of a relation |
| `the total ⟨attr⟩ taken from ⟨np⟩` | the amounts taken from a shared entity (chapter 9) |
| `the total of ⟨calculation⟩ over ⟨list⟩` | a sum over a list, in formulas (chapter 3) |
| `+`, `-`, `*`, `/`, parentheses | arithmetic (chapter 3) |

A sum over nothing is 0.

## One meaning, one phrasing

Every meaning has exactly one way to be written. `is at most` is how Suture says "can't be more than", "may not exceed", "must be at most" and "is no more than". The compiler knows those alternatives, but only to tell you the canonical phrase (chapter 12). A program never contains them. This is what makes the read-back trustworthy: two sentences that look different always mean different things.
