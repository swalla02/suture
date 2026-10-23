# 19. Data marks and the manifest

Some fields must not travel. A card number goes to the payment provider and nowhere else. An address goes to the carrier, not into a trace someone pastes into a ticket. Suture lets the program say so, checks it at compile time, and enforces it at run time.

## Marks

A mark follows the type of an attribute:

```suture
module vault version 1, currency EUR

role customer is a Customer

kind Customer { name: Text, address: Text personal }
kind Order    { customer: Customer, card: Text secret, total: Money }
kind Payment  { order: Order, amount: Money = order's total }
kind Shipment { order: Order }

Order needs Payment, Shipment

effect charge Payment via payments within 30 seconds
  sends    its amount, the order's card
  reports  charged
effect ship Shipment via carriers within 1 day
  sends    the order's customer's address
  reports  delivered

allow payments to receive Order's card
allow carriers to receive Customer's address

admit place a new Order by customer where the Order's customer is the caller
```

- **`personal`**: data about a person. Rules may read it, but it is masked wherever the program shows it, and only adapters granted it receive it.
- **`secret`**: a value the program only passes on. It can be a `Text` only, it has no default, and nothing may compare it or compute with it. It is read in exactly two places: an adapter's `sends`, and the formula of another secret attribute.

References, sets and lists cannot be marked: mark the attributes of the kind they point to.

## Grants

Every marked field an effect or a source sends needs an `allow`, naming the adapter:

```text
allow ⟨adapter⟩ to receive ⟨Kind⟩'s ⟨attr⟩[, ⟨Kind⟩'s ⟨attr⟩…]
```

Without it, the program does not compile:

```suture fails
effect audit Order via auditors within 1 day
  sends    its card
  reports  logged
```

```errors
main.st:23: Order's card is secret, and auditors is not allowed to receive it; write `allow auditors to receive Order's card`
```

A rule cannot look at a secret:

```suture fails
rule visa: every Order's card is not ""
```

```errors
main.st:23: `its card`: Order's card is secret; secrets can be sent to an adapter, never compared or computed with
```

An `allow` for a field no send reads is a warning, and one for a field without a mark is an error. A secret cannot be fetched by a `source`: it comes in from a caller or the world.

## At run time

A secret never enters a unit. When an input carries one, the runtime puts the value in its secret store and keeps a reference in the unit. Only the request to a granted adapter carries the value itself.

- The trace, explanations and `GET /units/⟨label⟩` show a secret as `(secret)` and a personal value as `(personal)`.
- A `no two` group on a personal value is named by a keyed hash, `(personal 3f9a…)`. The key is in `personal.key` in the state directory, readable only by its user, so a reader of the trace cannot check a guess. Keep the file with the state: a new key renames those groups.
- `GET /pending` masks them too, unless it carries the operator token: an adapter polling for its requests receives what it was granted.
- Decision models receive no marked facts. A `Media` attribute can be marked `personal` (not `secret`), and is then not shown to models either.

```text
O1  placed: customer Ada, card (secret), total 80 EUR
      requests: payments: charge Payment for O1   (O1/Payment)
        sends: amount 80 EUR, card (secret)
```

A served program logs the reference, `{"Secret": 1}`, and keeps the values in `secrets.json` in the state directory. The file is readable only by its owner and written, and synced, before the log line that refers to it. A restart reads it back. Without it, a log that refers to a secret is refused: the state directory is incomplete.

Back up `secrets.json` with the log, and protect it like a password file. The log on its own holds no secret.

A new version that marks an attribute secret, or drops its `secret` mark, is refused at the deploy check while live units hold values of it (chapter 18): the values already held would be kept the old way.

## The manifest

`suture build` writes `⟨module⟩-v⟨n⟩.manifest` next to the compiled program, and prints its adapters part:

```text
manifest of vault v1

adapters:
  carriers   receives  Customer.address (personal)
             answers   ship Shipment: delivered
  payments   receives  Payment.amount, Order.card (secret)
             answers   charge Payment: charged
marked:   Customer.address (personal), Order.card (secret)

roles     customer (a Customer)
admitted  place a new Order by customer where the Order's customer is the caller
```

It is what a security review needs: each adapter, every field it receives and its mark, the outcomes it may report, and who may do what. It is derived from the program, so it cannot drift from it. Review its diff with each version.
