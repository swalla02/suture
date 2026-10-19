# 7. Rules

A rule is something that must always hold. It is written once, in one place, and the runtime enforces it on every step that could break it. You never write a check.

```text
rule ⟨name⟩: ⟨sentence⟩
```

A rule's name is one lower-case word, unique in the module. The examples use research grants: labs receive grants from funds, each grant is paid out in awards, and a lab may be frozen.

```suture
module grants version 1, currency EUR

kind Fund   { name: Text, budget: Money }
kind Lab    { name: Text, fund: Fund, frozen: Bool = no }
kind Grant  { lab: Lab, amount: Money, approved: Bool = no }
kind Award  { grant: Grant, amount: Money = grant's amount }
kind Report { grant: Grant }

Grant needs Award, Report

effect pay Award via bank within 1 day
  sends    its amount
  marks    sent
  reports  paid | fails with bounced
effect file Report via archive within 30 days
  sends    the grant's lab's name
  reports  filed

rule size:     every Grant's amount is at most 50000 EUR
rule approved: no Grant has an Award that is sent unless it is approved
rule frozen:   no Grant whose lab is frozen has an Award that is sent
rule payout:   the total amount of each Grant's Awards is at most its amount
rule reports:  the number of each Grant's Reports is at most 1
```

## The forms

| Form | Holds when | Example |
|---|---|---|
| `every ⟨subject⟩ is ⟨state⟩` | each matching entity is in that state | `every Workspace is ready` |
| `every ⟨subject⟩ has a ⟨Kind⟩ [that …]` | each has at least one | `every Workspace has a Member that is admin` |
| `every ⟨subject⟩'s ⟨attr⟩ ⟨comparison⟩ ⟨num⟩` | each value is in bounds | `every Grant's amount is at most 50000 EUR` |
| `no ⟨subject⟩ has a ⟨Kind⟩ [that …] [unless ⟨cond⟩]` | none has one, unless the condition holds | `no Grant has an Award that is sent unless it is approved` |
| `no ⟨subject⟩ is ⟨state⟩ [unless …]` | none is in that state | |
| `⟨num with each ⟨Kind⟩⟩ ⟨comparison⟩ ⟨num⟩` | a bound for each entity | `the total amount of each Grant's Awards is at most its amount` |
| `for each ⟨Kind⟩ and ⟨period⟩, ⟨bound⟩` | a bound for each entity and calendar period | below |
| `⟨num with all ⟨Kinds⟩⟩ ⟨comparison⟩ ⟨num⟩, checked globally` | a bound over the whole system | below |

**Rules over a slot.** A rule may name a slot where it would name a kind: `no Claim has an Outcome that is sent unless its score is less than 70` holds back a Repair, a Writeoff and a Cash alike, and an alternative added to `Outcome` later is under it with no change. Every alternative must have the state named. To cover only some, list them: `no Claim has a Writeoff or a Cash that is sent unless …`, where the last `that` is about each. The read-back lists what the slot stands for: `no claim ever has an outcome (a repair, a writeoff or a cash) that is sent unless …`.

`unless` is the usual way to say "only after": `no Grant has an Award that is sent unless it is approved` means an award may go out only once the grant is approved. Since `sent` is the award's mark, the runtime holds the request back until then.

## Read-backs

For every rule, `suture build` prints what it understood:

```text
rule approved: no Grant has an Award that is sent unless it is approved
  understood as:   for each Grant g: exists(a in g.Award where a is sent) implies g is approved
  in other words:  no grant ever has an award that is sent unless it is approved
  cost:            local

rule payout: the total amount of each Grant's Awards is at most its amount
  understood as:   for each Grant g: sum(a in g.awards: a.amount) ≤ g.amount
  in other words:  each grant on its own: the total amount of its awards is at most its amount
  cost:            per-group
```

- **understood as** is the formula in a small logic: `for each`, `exists`, `sum`, `count`, `implies`, `not`.
- **in other words** is generated back from the formula with fixed templates that make each quantifier's scope explicit ("on its own", "ever"). It does not repeat your words: it is built from what the compiler understood. If it reads wrong, your sentence says something other than what you meant.
- **cost** is the rule's tier.

## Cost tiers

Every rule is checked in a bounded amount of work, and the tier says how.

- **local:** the rule reads only one unit: an entity, its parts, and facts carried into it. It is checked inside every step of that unit, at no extra cost.
- **per-group:** the rule bounds a running total kept by one entity: what is taken from a Stock, the refunds of one Customer in a month, the awards of one Grant. Each contribution is checked by the entity that keeps the total.
- **global:** the rule bounds a total over the whole system. It needs one running total for everything, which every contributing step must pass through. That is a bottleneck, so it must be asked for.

```suture fails
rule budget: the total amount of all Awards is at most 1000000 EUR
```

```errors
main.st:24: rule budget: `all Awards` has no group key, so one total would hold up every entity it counts; end the rule with `, checked globally`
```

```suture
rule budget: the total amount of all Awards is at most 1000000 EUR, checked globally
```

```text
rule budget: the total amount of all Awards is at most 1000000 EUR, checked globally
  understood as:   sum(a in all Award: a.amount) ≤ 1000000 EUR
  in other words:  across the whole system: the total amount of all Awards is at most 1000000 EUR
  cost:            global (bottleneck)
```

### Carried facts keep rules local

`rule frozen` reads `its lab`, another unit. It is still local, because the compiler **carries** the facts a local rule reads into the unit when it is created. If the lab's `frozen` can change through an `admit change`, the fact is **changeable**: the host remembers which units carry it, and refreshes and re-checks them when it changes (chapter 10). The rule's tier then becomes per-group.

In v0 only admitted changes are refreshed. If the lab's own reaction or an adapter's answer set `frozen`, the grants would keep their old copy and the rule could break without anyone noticing, so the compiler refuses such a rule: `rule frozen: it reads `frozen` from another unit, which a reaction or an answer changes there; …`. Carry the fact with a formula, or change it with `admit change`.

## Periods

```suture
kind Donor    { name: Text }
kind Donation { donor: Donor, amount: Money }

rule yearly: for each Donor and year, the total amount of their Donations is at most 5000 EUR
```

A period rule keeps one running total per entity and calendar `day`, `week` (from Monday), `month` or `year`, in UTC, of each contributing entity's `created`. `their Donations` from a Donor means every Donation whose one attribute of kind Donor points to it.

## How rules are enforced

Rules are enforced by **refusal**: a step whose result would break a rule is not kept.

| The step | What happens |
|---|---|
| a request the runtime wants to send | it waits, watching the facts the rule read, and is tried again when they change |
| a part or a slot alternative the runtime wants to create | it waits in the same way |
| an amount a binding wants to take | that candidate refuses, and the next is asked; with none left, it waits on its pool (chapter 9) |
| an answer from a service | it is refused; the request stays open with the same key |
| a reaction to a time-out | the time-out is recorded, and the reaction is held back |
| a caller's input | it is refused, and the caller is told the rule |

**Fixed refusals.** If a rule refuses something on facts that can never change (given or formula attributes that nothing sets, and fixed carried facts), waiting would be pointless. A slot whose alternative is refused that way moves on at once. In the shop, an order to a restricted country never waits for a Shipment: rule `embargo` reads only fixed facts, so the delivery goes straight to a Refund.

## Writing good rules

- **One rule, one thing.** Two short rules read back more clearly than one long one, and a refusal names exactly which one failed.
- **Prefer local.** Carry what a rule needs with a formula (`country: Country = customer's country`). Then the rule is checked inside one unit.
- **Rules restrict; they never cause.** A rule can only stop a step. To make something happen, declare a need; to change a fact, declare a reaction.
- **Read the read-back.** It is the compiler's interpretation, in words generated from the formula. It is the cheapest test you have.
