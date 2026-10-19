# 9. Shared entities and choices

Up to now each unit has been on its own: an entity and its parts. Real systems share things: stock, drivers, seats, clusters. This chapter is about how units **choose** among shared entities, **take** amounts from them, and are bounded by rules over what they took, with no locks and no scans.

The example is a parcel service. Each parcel is picked from a bin in a warehouse, one bin per item, and carried by a driver who covers the parcel's zone and has room for its weight.

```suture
module parcels version 1, currency EUR

kind Zone    { name: Text }
kind Sender  { name: Text }
kind Product { name: Text }
kind Bin     { product: Product, units: Count }
kind Driver  { name: Text, zones: set of Zone, capacity: Count, rate: Money, electric: Bool = no }
kind Parcel  { sender: Sender, zone: Zone, weight: Count, items: list of Item, cancelled: Bool = no }
kind Item    { parcel: Parcel, product: Product, quantity: Count }
kind Pick    { parcel: Parcel }
kind Leg     { parcel: Parcel }

Parcel needs Pick, Leg while it is not cancelled
Pick   uses  a Bin per item where the Bin's product is the item's product,
             taking the item's quantity
Leg    uses  a Driver where the Driver's zones contain the Parcel's zone,
             taking the Parcel's weight

effect pick Pick via warehouse within 1 hour
  sends    the parcel's weight
  marks    picked
  reports  packed
effect carry Leg via drivers within 1 day
  sends    its Driver's name
  marks    out
  reports  delivered | fails with lost

rule bins:   the total quantity taken from each Bin is at most its units
rule load:   the total weight taken from each Driver is at most its capacity
rule packed: no Parcel has a Leg that is out unless its Pick is packed

prefer Driver:        lowest rate, then those that are electric
prefer Bin for a item: highest units
```

## Shared entities

A kind that no other kind creates through `needs` is a **root**, and a root that other units refer to is **shared**: Zone, Sender, Product, Bin and Driver here. A shared entity is a unit like any other. Its own rules are checked by every step that changes it, and in a scenario it is created with `given`.

## Bindings: `uses`

```text
⟨Part⟩ uses a ⟨Kind⟩ [per ⟨element⟩] where ⟨condition⟩[, taking ⟨amount⟩]
```

- Each Leg gets one **binding** to a Driver. `per item` gives each Pick one binding for each element of its parcel's `items`, and in the condition `the item` is that element.
- In the condition, `the Driver` is the **candidate**. Everything else is read from the part and its owners.
- A part is **ready** only when all its bindings are granted, so a Leg cannot go out before it has a driver.
- After the binding is granted, `its Driver` reads the chosen entity: `sends its Driver's name`.
- `uses` belongs on a **part**, not on a root. Put it on the part whose effect commits the resource (here the Pick and the Leg), so that withdrawing that part gives the resource back (chapter 13).

### Pools

The condition must contain, as one of its `and` parts, an equality `the C's a is ⟨x⟩` or a membership `the C's a contain(s) ⟨x⟩`, where `⟨x⟩` does not read the candidate. That part is the **pool key**. The host indexes the entities of kind C by that attribute, so finding candidates never scans all drivers: it looks up the drivers of one zone. The other parts of the condition filter candidates within the pool.

`suture build` lists every choice with its pool key:

```text
choices:
  Pick uses a Bin for each of its items: pool key Bin's product is the item's product, up to 8 candidates, ranked by `prefer Bin` (1 criterion)
  Leg uses a Driver: pool key Driver's zones contain its parcel's zone, up to 8 candidates, ranked by `prefer Driver` (2 criteria)
```

A `uses` whose condition has no such equality or membership is a compile error: there would be nothing to index, and every choice would read every candidate.

### The choice

A binding is a decision point:

1. The host gathers up to `candidates` (8 by default; module setting) entities of the pool that pass the filter and have not already refused this binding.
2. They are ranked by the `prefer` criteria, most important first, then by creation order.
3. The first is asked to **take** the amount. It adds the amount to its running total and checks its own rules. If a rule would break, it refuses, and the next candidate is asked.
4. The pick is journaled.

### `prefer` for shared entities

```text
prefer ⟨Kind⟩ [for a ⟨element⟩]: ⟨criterion⟩[, then ⟨criterion⟩…]
```

| Criterion | Ranks first |
|---|---|
| `lowest ⟨num⟩` | the smallest value (an unknown value last) |
| `highest ⟨num⟩` | the largest value |
| `those ⟨condition⟩`, `those whose ⟨attr⟩ …` | candidates that meet the condition |

In a `prefer`, `the Driver` and `it` are the candidate. A `uses … per item` is named `prefer Bin for a item`. With no `prefer`, creation order decides, and the build warns.

## Taking

`taking the Parcel's weight` records an amount against the chosen entity. The amount is named after the last attribute: here `weight`, so the driver keeps a running total read as `the total weight taken from …`. A rule about that total is **per-group**: one running total per driver, checked by the driver.

`taking 1` counts instead: each chosen entity takes one, and the total is named after the taking kind's plural. `Repair uses a Garage where …, taking 1` is read as `the total repairs taken from each Garage`, so `rule bays: the total repairs taken from each Garage is at most its bays` gives a garage at most `bays` repairs at a time. Only a count may be taken this way; an amount such as `taking 5 EUR` is an error.

A binding with no `taking` is granted unless a rule of the candidate refuses it.

## Waiting and waking

When no candidate is left, the binding **waits** on its pool key, and its part waits with it. It is woken when:

- an amount is given back to an entity of that pool;
- a new entity joins that pool (`given` in a scenario, or an admitted creation);
- an admitted change sets an attribute of an entity in that pool (chapter 10).

Only the waiters the change can serve are woken. The host goes through them in arrival order, and wakes the first one the changed entity would grant its take. That binding gathers again from scratch, and its take is applied before the host looks for the next one. A waiter the entity cannot serve stays asleep and sends nothing. So when 2 units come back, an order waiting for 2 gets them, and an order behind it waiting for 1 is not woken just to be refused. A smaller waiter can go ahead of a larger one that does not fit.

## Giving back

A part that is withdrawn, or moved on from by its slot, **before any of its effects was requested** gives back every amount it took. After a request, bindings and amounts stay (chapter 2): a lost parcel's leg keeps the driver's capacity taken, and a replacement leg takes its own.

```scenario
scenario "the cheapest driver with room carries the parcel":
  given  a Zone North with name North
  and    a Sender Sam with name Sam
  and    a Product Tea with name Tea
  and    a Bin B1 with product Tea and units 10
  and    a Driver Dee with name Dee, zones North, capacity 20 and rate 9 EUR
  and    a Driver Eli with name Eli, zones North, capacity 5 and rate 7 EUR
  when   a new Parcel P1 with sender Sam, zone North and weight 8
         and an item with product Tea and quantity 2
  and    warehouse answers packed
  and    drivers answers delivered
  expect P1's Leg's Driver's name is "Dee"
  expect the total weight taken from Dee is 8
  expect the total quantity taken from B1 is 2

scenario "a parcel waits for a driver, and a new one wakes it":
  given  a Zone North with name North
  and    a Sender Sam with name Sam
  and    a Product Tea with name Tea
  and    a Bin B1 with product Tea and units 10
  and    a Driver Eli with name Eli, zones North, capacity 5 and rate 7 EUR
  when   a new Parcel P1 with sender Sam, zone North and weight 8
         and an item with product Tea and quantity 2
  expect P1 has no Leg that is out
  expect the total weight taken from Eli is 0
  given  a Driver Dee with name Dee, zones North, capacity 20 and rate 9 EUR
  expect the total weight taken from Dee is 8

scenario "a lost leg keeps the capacity it took, and its replacement takes its own":
  given  a Zone North with name North
  and    a Sender Sam with name Sam
  and    a Product Tea with name Tea
  and    a Bin B1 with product Tea and units 10
  and    a Driver Dee with name Dee, zones North, capacity 20 and rate 9 EUR
  when   a new Parcel P1 with sender Sam, zone North and weight 8
         and an item with product Tea and quantity 2
  and    warehouse answers packed
  and    drivers answers lost
  expect the number of P1's Legs is 2
  expect the total weight taken from Dee is 16
```

In the first scenario Eli is cheaper, but taking 8 would break rule `load` on Eli, so Eli refuses and Dee is asked. In the second, Eli refuses and there is nobody else, so the leg waits until Dee joins the pool. In the third, the lost leg had gone out, so its 8 stay taken, and the replacement leg takes another 8.

## Relations across units

From a shared kind, a plural reads every entity that refers to it through its one attribute of that kind: from a Sender, `their Parcels` is every Parcel whose `sender` is that Sender.

```suture
rule daily: for each Sender and day, the total weight of their Parcels is at most 100
```

Each Parcel contributes to its sender's running total when it is created, exactly as if it had `uses a Sender where the Sender is its sender, taking its weight`. A rule over the relation is per-group, enforced by refusing the contribution: the parcel waits, as above. In v0 such a relation has no `that` filter.

## Cost

Every gather, take, give-back and wake reads or writes a bounded number of units, set by `candidates` and the pool key. Nothing scans all entities of a kind, except a rule `checked globally`, which is one running total (chapter 7).
