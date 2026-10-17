# 3. Kinds and values

This chapter covers the facts of a program: the module, its kinds, their attributes, and the values attributes can hold.

## Files and the module

A program is every `.st` file in one directory. The files share one `module` line, which appears once:

```suture
module library version 1, currency EUR

kind Member { name: Text }
```

- `module ⟨name⟩ version ⟨n⟩, currency ⟨code⟩` names the program and its version, and fixes the single currency of every `Money` value.
- Two optional settings follow: `attempts ⟨n⟩` (how many times a part is tried and a request is sent, default 3; chapters 4 and 5) and `candidates ⟨n⟩` (how many candidates a choice looks at, default 8; chapter 9). For example: `module shop version 1, currency EUR, attempts 5`.
- Declarations and sentences may come in any order, and names resolve across all files. Splitting a program into files is only for people: `examples/shop/france.st` holds everything about French customs, and `shop.st` never mentions France.
- A line that starts with `--` is a comment, and so is the rest of a line after `--` (outside a string).
- A statement continues on the following lines as long as they are indented.

## Naming

- **Kinds** are capitalised: `Order`, `CustomsForm`.
- **Attributes, roles, states and adapters** are lower-case: `total`, `support`, `paid`, `payments`.
- **Plurals** are formed by adding `s`, or `es` after s, x, ch and sh, and `y` after a consonant becomes `ies`. An irregular plural is declared: `kind Person (plural People) { … }`. Plurals are used for relations (`its Refunds`, chapter 6).

## Kinds and attributes

```suture
kind Book { title: Text, author: Text, copies: Count, fee: Money = 2 EUR, rare: Bool = no }
```

A `kind` lists attributes as `name: Type`, separated by commas. A long kind can continue on indented lines.

## Types

| Type | Values | Literals |
|---|---|---|
| `Text` | strings | `"Ada"`; in scenarios also a bare word, `Ada` |
| `Bool` | yes or no | `yes`, `no` (also `true`, `false`) |
| `Count` | whole numbers, 0 or more | `3` |
| `Money` | amounts in the module's currency, exact to the cent | `10 EUR`, `9.99 EUR` |
| `Duration` | lengths of time | `30 seconds`, `5 minutes`, `2 hours`, `3 days`, `1 week` |
| `Time` | instants from the runtime's clock | (none: times come from `created` and the clock) |
| a kind name | a reference to one entity | the entity's name, in scenarios |
| `list of K` | entities of kind K given together with this one | `and a line with …`, in scenarios |
| `set of K` | references to several entities of kind K | `{DE, FR}`, in scenarios |

Every entity also has a built-in `created: Time`, the clock when it was created.

## Defaults and formulas

What follows `=` decides whether an attribute is a default or a formula.

- **A literal is a default.** `rare: Bool = no` starts as `no`, and a reaction or an admitted change may set it later.
- **Anything else is a formula**, computed once when the entity is created and never changed afterwards.

```suture
kind Loan { member: Member, book: Book, days: Count,
            fee: Money = book's fee * days,
            returned: Bool = no }
```

A formula can read the entity's own given values, its other formulas, its list elements, its owner's attributes, and attributes of the entities it references. They are computed in whatever order they depend on each other; formulas that read each other in a circle are a compile error, and so is a formula that reads a value that only arrives later (an answer's data, or a part created by a need).

Because a formula is computed once, it is how a fact is **carried**: `fee` keeps the book's fee as it was when the loan was made, even if the book's fee changes later. This is usually what the business means, and it is what keeps rules cheap (chapter 7).

A formula whose value is unknown, or a `Count` formula that would be negative, leaves the attribute unknown. An unknown value never breaks a rule, and it makes a condition false.

## Arithmetic

Calculations use `+`, `-`, `*`, `/` (or `−`, `×`) and parentheses. `*` and `/` bind tighter than `+` and `-`.

| Allowed | Result |
|---|---|
| `Money + Money`, `Money - Money` | Money |
| `Count + Count`, `Count - Count`, `Count * Count`, `Count / Count` | Count |
| `Money * Count`, `Count * Money`, `Money / Count` | Money |
| `Time - Time` | Duration |
| `Time + Duration`, `Time - Duration` | Time |
| `Duration ± Duration`, `Duration * Count`, `Duration / Count` | Duration |

Anything else is a compile error: you cannot add a Count to Money. Division rounds toward zero (to the cent for Money). An overflow or a division by zero makes the result unknown.

## References

An attribute whose type is a kind holds a reference to one entity of that kind:

```suture
kind Branch { city: Text }
kind Shelf  { branch: Branch, label: Text }
```

Through a reference you can read the other entity's attributes, and their references in turn: `its shelf's branch's city`. A reference must name an existing entity when the entity is created, or the creation is refused.

There is one special case. When a kind is a **part** of another (chapter 4), its attribute of the owner's kind is not an ordinary reference: it is the owner, and the runtime sets it.

## Sets

`set of K` holds references to several entities:

```suture
kind Region  { name: Text }
kind Courier { name: Text, regions: set of Region }
```

Conditions test membership with `contains` and `does not contain`: `the Courier's regions contain the Branch's region`. In a scenario a set is written `{North, South}`, or as one name.

## Lists

`list of K` gives an entity elements that are created together with it, such as an order's lines:

```suture
kind Basket { member: Member, items: list of Item,
              total: Money = the total of the item's price * the item's quantity over its items }
kind Item   { basket: Basket, book: Book, quantity: Count, price: Money = book's fee }
```

- The element kind names its owner, like a part does (`basket: Basket`).
- One element is named by the list's name in the singular: `items` → `the item`, `lines` → `the line`, `entries` → `the entry`.
- `the total of ⟨calculation⟩ over ⟨list⟩` sums a calculation in which `the item` is each element in turn. It is used in formulas.
- In a scenario, elements are given with their owner: `and an item with book Dune and quantity 2`.
- An element is `done` from the start and has no needs or effects of its own.

## Shared entities

A kind that no other kind creates through `needs` can be **shared**: many units may refer to it. In this chapter, `Member`, `Book`, `Branch`, `Region` and `Courier` are shared; a scenario creates them with `given`. Chapter 9 explains how units choose among shared entities and take amounts from them.
