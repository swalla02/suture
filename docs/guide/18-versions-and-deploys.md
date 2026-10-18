# 18. Versions and deploys

A served program has live units: bookings waiting for a payment, tickets on their way, sessions with seats taken. Changing the program changes what those units must obey. Suture does not let a change quietly break them. A new version is checked against every live unit first, and the deploy is refused until each break has a resolution written in the program.

## A new version

The module line carries the version:

```text
module tickets version 2, currency EUR
```

A deploy needs a higher version than the one running. The same version with different content is refused, and so is going back to an older one.

## The check

`suture deploy` replays the state directory, carries every live unit over to the new version in memory, and reports what would happen. It never writes, so it can run in a pipeline before the new version is started.

Say version 2 adds a rule that one booking is for one seat:

```text
rule pairs: every Booking's quantity is at most 1
```

Ada's booking B1, made under version 1, is for two seats:

```sh
suture deploy examples/tickets
```

```text
tickets v2: the deploy would be refused; these breaks need a resolution:
  rule pairs would break: each booking on its own: its quantity is at most 1
    live in B1
    resolve: `apply rule pairs to Bookings created from version 2`, or change the rule
```

The command exits with failure, and `suture serve` refuses to start the new version for the same reason.

These are the breaks the check finds:

| Break | Resolution |
|---|---|
| a rule of the new version that a live unit breaks | `apply rule … to … created from version …`, or change the rule |
| a kind removed while live units have it, as a root or a part | keep the kind until no live unit has it |
| an attribute whose type changed while live units hold a value of it | keep the type, and add an attribute of the new type under another name |
| an attribute added without a default, on a kind with live units | give it a default, as in `vip: Bool = no` |

An attribute an effect's answer sets, or a source fetches, needs no default: it is not known until then anyway.

## Applying a rule from a version

```text
apply rule pairs to Bookings created from version 2
```

Rule `pairs` then holds only on the bookings created under version 2 or later. B1 keeps its two seats and goes on as before: its payment, its ticket, its goal. A new booking for two seats is refused. Every unit records the version it was created under, and a deploy does not change it.

Only a rule about one entity at a time (a `local` rule, chapter 7) can apply from a version, and the kind named must be the rule's subject, or the root that holds it. `suture build` shows it under the rule's read-back:

```text
rule pairs: every Booking's quantity is at most 1
  understood as:   for each Booking b: b.quantity ≤ 1
  in other words:  each booking on its own: its quantity is at most 1
  cost:            local
  applies to:      Bookings created from version 2
```

## What a deploy does

Once nothing is left unresolved, `suture deploy` shows what the deploy would do, and starting the new version with `suture serve` does it:

```text
tickets v2 on http://127.0.0.1:7878, state in examples/tickets/.suture
  deployed tickets v2 over v1
  Ada now under version 2
  …
  B1  now under version 2
```

- Attributes the new version removes are dropped from live units, and the trace says which.
- Added attributes take their defaults.
- Goals are matched by their text, so a goal already met or alerted stays so.
- Then every live unit settles once under the new version. A new `needs` creates its parts, and their requests go out. A request already sent stays sent, and its answer is awaited as before.

Nothing that already happened is undone.

## The log

The deploy is a line of the log, `{"deploy": {"version": 2}}`. The state directory keeps each version's program: the first in `program.json`, each later one in `program-v⟨n⟩.json`. A restart replays the log under the version in force at each point and makes each deploy again at its place, so it comes back to the same state.
