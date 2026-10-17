# 13. Modelling patterns

This chapter is about turning requirements into declarations, and about the few places where a reasonable first attempt does something other than what you meant.

## From a requirement to declarations

Take the requirement sentence by sentence and ask what kind of statement it is:

| The requirement says | Write |
|---|---|
| a thing the business talks about (an order, a seat, a driver) | a `kind` |
| "each X has its own Y", "an X gets a Y" | `X needs Y` |
| "… but only when …" | `X needs Y while …` |
| "an X is either a Y or a Z", "try Y, else Z" | a slot `S is Y or Z` with `prefer S: Y, then Z after …` |
| "an X is assigned a Y from the pool" | `X uses a Y where …, taking …` |
| "we ask service S to do something" | an `effect … via S within …` |
| "never", "at most", "only after", "only if" | a `rule` |
| "X counts as done when …", any status | a `define` |
| "within 3 days, every …" | `desired within 3 days: …` |
| "if S says no, then …" | `when a Y is ⟨outcome⟩: …` |
| "a customer can …", "only staff may …" | `role` and `admit` |
| "prefer the cheapest" | `prefer Y: lowest fee` |

What is left over is usually a sequence ("first … then …"). A sequence is almost always a rule in disguise: "ship only after payment" is `no Order has a Shipment that is sent unless it is paid`.

## Statuses are definitions

Do not add a `status: Text` attribute and set it from reactions. Each status is a definition over facts:

```suture
module orders version 1, currency EUR

kind Order    { total: Money, cancelled: Bool = no }
kind Payment  { order: Order }
kind Shipment { order: Order }

Order needs Payment, Shipment while it is not cancelled

effect capture Payment via payments within 30 seconds
  sends    the order's total
  reports  captured | fails with declined
effect send Shipment via carriers within 7 days
  sends    the order's total
  marks    sent
  reports  delivered | fails with lost

define: an Order is paid when its Payment is captured
define: an Order is shipped when its Shipment is sent
define: an Order is complete when it is paid and its Shipment is delivered

rule paid: no Order has a Shipment that is sent unless it is paid

when a Payment is declined: its Order is cancelled
```

A definition cannot be out of date, cannot be set on one path and forgotten on another, and its meaning is in one place.

## Cancellation is a fact and a condition

Cancelling is a Bool attribute, set by a reaction or an admitted event, and a `while` on the needs. Everything that has not reached the outside world is then withdrawn and gives back what it took. Everything that has stays, and counts. You do not write the compensation.

To act on a cancellation (refund a captured payment, say), declare what a cancelled entity needs:

```suture
role shopper

kind Refund { order: Order }

Order needs Refund while it is cancelled and it is paid

effect pay Refund via payments within 30 seconds
  sends    the order's total
  reports  refunded

when a shopper cancels an Order: the Order is cancelled

admit place a new Order by shopper
admit cancel Order by shopper
```

```scenario
scenario "a paid order that is cancelled is refunded":
  when   shopper Sue places a new Order O1 with total 50 EUR
  and    payments answers captured
  expect O1 is paid
  expect O1 has no Refund
  when   shopper Sue cancels O1
  and    payments answers refunded
  expect O1's Refund is refunded
```

The Shipment had already gone out when the order was cancelled, so it stays, and the refund follows from the new facts.

## Put `uses` on the part that commits

A part with no effect is done as soon as its needs are done, and a done part stays done. So if the part that holds a resource has no effect of its own, it is done at once, and it keeps the resource even when its owner is cancelled:

```suture
module seats version 1, currency EUR

kind Show    { name: Text, seats: Count }
kind Booking { show: Show, quantity: Count, cancelled: Bool = no }
kind Place   { booking: Booking }
kind Payment { booking: Booking }

Booking needs Place, Payment while it is not cancelled
Place   uses  a Show where the Show's name is the Booking's show's name,
              taking the Booking's quantity

effect charge Payment via payments within 30 seconds
  sends    the booking's quantity
  reports  charged | fails with declined

rule seats: the total quantity taken from each Show is at most its seats

when a Payment is declined: its Booking is cancelled
```

```scenario
scenario "the trap: a done Place keeps its seats after a cancellation":
  given  a Show Hamlet with name Hamlet and seats 10
  when   a new Booking B1 with show Hamlet and quantity 2
  and    payments answers declined
  expect B1 is cancelled
  expect the total quantity taken from Hamlet is 2
```

Put the `uses` on the part whose effect commits the resource. In chapter 1 that is the Ticket, and in the shop it is the Shipment. That part is unfinished until its request goes out, so a cancellation before then withdraws it and gives the seats back.

## Carry facts with formulas

A formula is computed once, at creation. Use one whenever the business means "as it was at the time":

- `price: Money = product's price` on an order line keeps the price the customer saw.
- `country: Country = customer's country` on an order keeps where it was going when it was placed.

This also keeps rules **local**: a rule that reads the order's own `country` is checked inside the order's unit, and never needs to watch the customer.

## Fallbacks are slots with patience

"Ship it; if we cannot within 2 days, refund it" is a slot:

```text
Delivery is Shipment or Refund
prefer Delivery: Shipment, then Refund after 2 days
```

The slot moves on after a failure on the last attempt, after the patience runs out (only if the shipment never went out), or at once if a rule refuses the shipment on facts that can never change, such as an embargoed country.

## Reactions change facts; needs make work

A reaction can only set attributes. It never creates a part or sends a request. If something must happen after an event, make the event a fact (`its Order is cancelled`) and declare what that fact needs (`Order needs Refund while it is cancelled and it is paid`). This keeps every piece of work visible in the needs, where `explain` can see it.

## Name every combined condition

One sentence cannot mix `and` with `or`, and a `that` filter takes one condition. When you need either, define the part:

```text
define: a Case is important when it is escalated and it is vip
rule callbacks: no Case has a Callback unless it is important or its severity is at least 4
```

The definition gets a name the business can use, and the rule reads back plainly.

## Count what reached the world

Relations and totals include every entity that counts, failed attempts included, because they may have had effects (chapter 6). If a bound is about successes, filter: `the number of its Shipments that are delivered`. If it is about exposure (money sent, parcels in the world), do not.

## Time

There is no `now` in sentences: a rule must not change from true to false while nothing happens. Time enters through:

- deadlines on effects (`within`), which re-send and time out;
- patience on slots (`after`);
- goal deadlines (`desired within`), which raise alerts;
- `created`, and periods in rules (`for each Customer and month`).

## Prefer local rules

A local rule costs nothing extra. A per-group rule costs one running total per group. A global rule is one total for the whole system, and every contributing step queues behind it. When a rule comes out global, look for the group the business really means: per customer, per warehouse, per day.
