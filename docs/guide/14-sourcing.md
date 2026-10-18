# 14. Sourcing

Some facts are not given by the caller and not decided by the program: they live somewhere else, and they go stale. A customer's credit at a bureau, a price at a supplier, a stock level in another system. A **source** says where such a fact comes from, how long it can be trusted, and what to do when it cannot be had. You never write when to fetch it: the runtime fetches it when a step needs it, and fetches it again when it is too old for the rule that guards the step.

This chapter uses a credit check: an order's payment goes out only when the customer's credit covers the order's total.

## A source

```suture
module credit version 1, currency EUR

role staff

kind Customer { name: Text }
kind Product  { name: Text }
kind Stock    { product: Product, units: Count }
kind Order    { customer: Customer, product: Product, quantity: Count, total: Money,
                credit: Money, approved: Bool = no, cancelled: Bool = no }
kind Payment  { order: Order, amount: Money = order's total }
kind Shipment { order: Order }

Order    needs Payment, Shipment while it is not cancelled
Shipment uses  a Stock where the Stock's product is the Order's product,
               taking the Order's quantity

effect capture Payment via payments within 30 seconds
  sends    its amount
  marks    sent
  reports  captured | fails with declined
effect send Shipment via carriers within 7 days
  sends    the Order's quantity
  marks    sent
  reports  delivered

source Order's credit via bureau within 5 seconds
  sends      the customer's name
  reports    found | fails with unknown
  good for   1 day
  otherwise  use 0 EUR

rule credit:   no Order has a Payment that is sent unless its total is at most its credit,
               with credit at most 1 hour old
rule approved: no Order has a Payment that is sent unless it is approved
rule stock:    the total quantity taken from each Stock is at most its units
rule paid:     no Order has a Shipment that is sent unless its Payment is captured

prefer Stock: highest units

when a staff cancels an Order: the Order is cancelled

admit place a new Order by staff
admit change Order's approved by staff
admit cancel Order by staff
admit add a new Customer, Product or Stock by staff
```

The general form:

```text
source ⟨Kind⟩'s ⟨attr⟩ via ⟨adapter⟩ within ⟨duration⟩
  sends      ⟨np⟩[, ⟨np⟩…]
  reports    ⟨outcome⟩ [| fails with ⟨outcome⟩[, …]]
  [good for  ⟨duration⟩]
  [otherwise use ⟨literal⟩ | ask again after ⟨duration⟩ | ⟨action⟩[ and ⟨action⟩…]]
```

A source looks like an effect, with three differences:

- **It asks for one value.** The answer to `found` carries the attribute itself: `bureau answers found with credit 500 EUR`. Nothing else may come with it.
- **It has a lifespan.** `good for 1 day` says the value is trusted for a day after it is fetched. Without it, the value is trusted until the unit ends.
- **It has a recovery.** `otherwise` says what happens when the fetch fails: the source answers a `fails with` outcome, or it does not answer before the last of the module's `attempts` sends times out.

The attribute is declared as usual (`credit: Money`), with no default and no formula, and nobody gives it: placing an order with a credit is refused.

## A rule's own bound

`good for` belongs to the fact. A rule can ask for something fresher, for itself:

```text
rule credit: no Order has a Payment that is sent unless its total is at most its credit,
             with credit at most 1 hour old
```

A credit fetched three hours ago is still trusted, so explanations and other rules can read it. But the payment does not go out on it: the `credit` rule wants a credit at most an hour old, so the runtime fetches it again first. The bound can only shorten the lifespan; a value past its `good for` is stale for every rule.

## When a fact is fetched

A rule that reads a sourced fact, directly or through a `define`, is **strict**. When a step is about to act (create a part, send a request, record a part as done) and a strict rule guards it, a sourced value that is missing or too old counts against the step: the step waits, as if the rule were broken. The runtime then fetches what the waiting step needs, and only that:

- a value is fetched when a step waits on a strict rule that reads it, and the value is missing or too old for that rule;
- one fetch at a time per value; a fetch that is open is not repeated;
- when the answer comes, the step is checked again, with the new value.

So the order of a unit's rules matters only for the trace: a step waits on the first rule it would break. Here the payment waits on `credit` first, so the credit is fetched while the order also waits for approval:

```scenario
scenario "the credit is fetched, then covers the payment":
  given  a Customer Ada with name Ada
  and    a Product Kettle with name Kettle
  and    a Stock K1 with product Kettle and units 5
  when   staff Sam places a new Order O1 with customer Ada, product Kettle, quantity 2 and total 60 EUR
  and    staff Sam changes O1's approved to yes
  and    bureau answers found with credit 500 EUR
  expect O1's credit is 500 EUR
  expect O1's Payment is sent
  when   payments answers captured
  expect O1's Shipment is sent

scenario "a low credit holds the payment":
  given  a Customer Ada with name Ada
  and    a Product Kettle with name Kettle
  and    a Stock K1 with product Kettle and units 5
  when   staff Sam places a new Order O1 with customer Ada, product Kettle, quantity 2 and total 600 EUR
  and    staff Sam changes O1's approved to yes
  and    bureau answers found with credit 500 EUR
  expect O1's Payment is not sent
```

The trace of the first scenario shows each fetch and why it happened:

```text
O1  Payment waits: rule credit
O1  credit: asking bureau (name "Ada")
…
      bureau: found, credit 500 EUR   (O1~credit#1)
O1  credit: 500 EUR from bureau, trusted for 1 day
O1  Payment → capture via payments (amount 60 EUR)
```

A fetch is numbered (`O1~credit#1`, `#2`, …); an answer to an older fetch is ignored, and a malformed answer is refused and asked again, so a late or wrong answer never changes the value.

When the approval comes late, the credit is too old for the rule, and it is fetched again:

```scenario
scenario "a credit too old by the time of approval is fetched again":
  given  a Customer Ada with name Ada
  and    a Product Kettle with name Kettle
  and    a Stock K1 with product Kettle and units 5
  when   staff Sam places a new Order O1 with customer Ada, product Kettle, quantity 2 and total 60 EUR
  and    bureau answers found with credit 500 EUR
  and    3 hours pass
  and    staff Sam changes O1's approved to yes
  expect O1's Payment is not sent
  when   bureau answers found with credit 400 EUR
  expect O1's credit is 400 EUR
  expect O1's Payment is sent
```

```text
O1  credit: older than 1 hour for rule credit: asking bureau again (name "Ada")
```

Only guards are strict. Answers, events and changes are checked against the last values, stale or not, so an answer is never refused because a credit got old while it was on its way.

## When the fetch fails

`otherwise` is the recovery. There are three, and a fourth when you write none:

| `otherwise` | After a failure |
|---|---|
| `use 0 EUR` | the value is set as if fetched now, marked as a fallback, and trusted for `good for` |
| `ask again after 1 hour` | the value stays unknown; after an hour the next waiting step fetches again |
| `it is cancelled` (any actions) | the actions apply to the unit, as a reaction's would; needs and `while` do the rest |
| (none) | the value stays unknown and the steps its rules guard wait; a goal that is late says why |

A fallback is a value like any other, so the rules decide what it means. Here no credit holds any payment, and after a day the fallback is stale and the bureau is asked again:

```scenario
scenario "a failing bureau falls back to no credit for a day":
  given  a Customer Ada with name Ada
  and    a Product Kettle with name Kettle
  and    a Stock K1 with product Kettle and units 5
  when   staff Sam places a new Order O1 with customer Ada, product Kettle, quantity 2 and total 60 EUR
  and    staff Sam changes O1's approved to yes
  and    bureau answers unknown
  expect O1's credit is 0 EUR
  expect O1's Payment is not sent
  when   1 day passes
  and    bureau answers found with credit 500 EUR
  expect O1's Payment is sent
```

```text
O1  credit: bureau failed (unknown): using 0 EUR instead
…
O1  credit: older than 1 day: asking bureau again (name "Ada")
```

A bureau that does not answer is asked again with the same fetch number, up to the module's `attempts`, and then the fetch fails:

```text
O1  credit: no answer within 5 seconds: asking again (send 2 of 3)
O1  credit: no answer within 5 seconds: asking again (send 3 of 3)
O1  credit: bureau failed (timed out after 3 sends): using 0 EUR instead
```

### Aborting, and what follows

With actions as the recovery, a failure can end the order. The actions apply as a reaction's would, and the cascade is the usual one: the order is cancelled, so its `while` no longer holds, its parts that were not sent are withdrawn, and the stock they took is given back.

```suture
module credit version 1, currency EUR

role staff

kind Customer { name: Text }
kind Product  { name: Text }
kind Stock    { product: Product, units: Count }
kind Order    { customer: Customer, product: Product, quantity: Count, total: Money,
                credit: Money, cancelled: Bool = no }
kind Payment  { order: Order, amount: Money = order's total }
kind Shipment { order: Order }

Order    needs Payment, Shipment while it is not cancelled
Shipment uses  a Stock where the Stock's product is the Order's product,
               taking the Order's quantity

effect capture Payment via payments within 30 seconds
  sends    its amount
  marks    sent
  reports  captured
effect send Shipment via carriers within 7 days
  sends    the Order's quantity
  marks    sent
  reports  delivered

source Order's credit via bureau within 5 seconds
  sends      the customer's name
  reports    found | fails with unknown
  otherwise  it is cancelled

rule credit: no Order has a Payment that is sent unless its total is at most its credit
rule stock:  the total quantity taken from each Stock is at most its units
rule paid:   no Order has a Shipment that is sent unless its Payment is captured

prefer Stock: highest units

admit place a new Order by staff
admit add a new Customer, Product or Stock by staff
```

```scenario
scenario "an unknown customer cancels the order and frees its stock":
  given  a Customer Ada with name Ada
  and    a Product Kettle with name Kettle
  and    a Stock K1 with product Kettle and units 5
  when   staff Sam places a new Order O1 with customer Ada, product Kettle, quantity 2 and total 60 EUR
  expect the total quantity taken from K1 is 2
  when   bureau answers unknown
  expect O1 is cancelled
  expect O1 has no Payment
  expect O1 has no Shipment
  expect the total quantity taken from K1 is 0
```

```text
O1  credit: bureau failed (unknown)
O1  Order is cancelled
O1  Payment withdrawn
O1  Shipment withdrawn
```

This source has no `good for`, so a credit, once fetched, is trusted for the life of the order.

## What a source cannot be

A sourced attribute belongs to its source alone. These are errors, each with its line:

```suture fails
when a Payment is captured: its Order's credit becomes 5 EUR
```

```errors
main.st:39: Order.credit comes from its source; a reaction cannot set it
```

```suture fails
admit change Order's credit by staff
```

```errors
main.st:39: `admit change Order's credit by staff`: `credit` comes from its source, so it cannot be changed
```

```suture fails
kind Invoice { order: Order }
```

```errors
main.st:26: Order is shared (Invoice refers to it), so its credit cannot be sourced in v0
```

The last one is a v0 limit: a fact that other units carry would need its freshness carried too, so the kind of a sourced attribute must not be referred to by another kind or picked by a `uses`. Its own parts may refer to it, as `Payment` and `Shipment` do here.

The other errors say what they mean: an attribute that does not exist, one with a default or a formula, a second source for the same attribute, an `otherwise use` of the wrong type, an answer that carries something else, and a `with x at most … old` on a rule that reads no sourced `x`.

## What you see

`suture build` lists each source, and each strict rule with the sources it reads and the bound it applies:

```text
rule credit: no Order has a Payment that is sent unless its total is at most its credit
  understood as:   for each Order o: exists(p in o.Payment where p is sent) implies o.total ≤ o.credit
  in other words:  no order ever has a payment that is sent unless its total is at most its credit
  cost:            local
  sources:         credit from bureau, fetched once

sourcing:
  Order's credit from bureau within 5 seconds (sends name); trusted until the unit ends; if it fails, it is cancelled; read by rule credit
```

`suture explain` gives each sourced value with its age and how long it is still trusted, or where its fetch stands:

```text
Order O1 (approved yes, cancelled no, credit 500 EUR, customer Ada, product Kettle, quantity 2, total 60 EUR)
  credit: 500 EUR from bureau, 30 minutes old, trusted 1410 minutes more
```

Other lines read `credit: asking bureau, send 1 of 3`, `credit: 0 EUR, the fallback after bureau failed`, `credit: bureau failed; the steps its rules guard wait`, and `credit: not fetched yet`.

The repository's `examples/credit/` has the whole credit check with its scenarios.
