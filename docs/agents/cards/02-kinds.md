# kinds: entities, attributes, types, states, defaults and formulas

```suture
module shop version 1, currency EUR

role clerk

kind Customer { name: Text, vip: Bool = no }
kind Product  { name: Text, price: Money }
kind Order    { customer: Customer, product: Product, quantity: Count,
                status: open | paid | shipped = open,
                total: Money = product's price * quantity,
                due: Time = created + 14 days }

admit add a new Customer or Product by clerk
admit place a new Order by clerk
```

- Kind names are capitalised (`Order`); attribute names are lower-case words.
- Types: `Text`, `Bool`, `Count` (whole number, never negative when changed), `Money` (in the module's currency), `Time`, `Duration`, `Media`, a kind (a reference: `customer: Customer`), `list of ⟨Kind⟩` (owned items), `set of ⟨Kind⟩`.
- A state attribute lists its states: `status: open | paid | shipped = open`. Each state is then a word you can use: `the Order is paid`, `every Order that is open …`.
- `= ⟨literal⟩` is a default; `= ⟨formula⟩` is computed and always current: `+ - * /`, a reference's attribute (`product's price`), `created + 14 days`.
- Every entity has `created: Time`.
- Literals: `"text"`, `yes`/`no`, `3`, `9.99 EUR`, `-3 EUR`, `30 seconds`/`minutes`/`hours`/`days`/`weeks`. A program cannot write a date; scenarios can (`2026-03-01`).
- Plural: `kind Person (plural People) { … }` when the plural is not the name plus `s`.

A status that follows from facts is a `define`, not a stored attribute, so it can never be out of date (`suture help rules`):

```suture
define: an Order is big when its total is at least 500 EUR
```
