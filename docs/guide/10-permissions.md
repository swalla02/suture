# 10. Permissions

Every input from outside the system, whether it creates an entity, sends an event or changes an attribute, passes a **gate**. The gate admits an input only if an `admit` statement allows it for the caller's role, under its condition. Permissions are declared once, next to the rest of the program, instead of being checked in every handler.

The example is a lending library:

```suture
module library version 1, currency EUR

role member is a Member
role librarian

kind Member { name: Text, suspended: Bool = no }
kind Title  { name: Text, stock: Count }
kind Loan   { member: Member, title: Text, copies: Count = 1, returned: Bool = no, renewed: Bool = no }
kind Hold   { loan: Loan }
kind Notice { loan: Loan }

Loan needs Hold while it is not returned
Loan needs Notice while it is renewed
Hold uses  a Title where the Title's name is the Loan's title, taking the Loan's copies

effect lend Hold via desk within 1 day
  sends    the loan's member's name
  marks    lent
  reports  collected
effect mail Notice via mailer within 1 hour
  sends    the loan's member's name
  reports  mailed

rule stock:     the total copies taken from each Title is at most its stock
rule suspended: no Loan whose member is suspended has a Hold that is lent
rule once:     no Loan is renewed unless it is not returned

when a member or librarian returns a Loan: the Loan is returned
when a member renews a Loan:               the Loan is renewed

admit borrow a new Loan by member where the Loan's member is the caller
admit return Loan by member where the Loan's member is the caller
admit return Loan by librarian
admit renew Loan by member where the Loan's member is the caller
admit add a new Member or Title by librarian
admit change Title's stock by librarian
admit change Member's suspended by librarian
```

## Roles

```text
role ⟨name⟩ [is a ⟨Kind⟩]
```

A caller has a role and a name.

- With `is a Member`, the caller's name must be an existing Member, and inside an `admit` condition `the caller` is that Member. An input from a member who does not exist is refused at the gate.
- With no kind, the name is only an identity: `librarian Lin`.

In v0 callers are named by the scenario (and later by the input file). Real authentication is designed but not built (chapter 15).

## Three things to admit

| Form | Lets a caller |
|---|---|
| `admit ⟨verb⟩ a new ⟨Kind⟩[, ⟨Kind⟩… or ⟨Kind⟩] by ⟨role⟩[, ⟨role⟩] [where ⟨cond⟩]` | create an entity from the input's fields |
| `admit ⟨verb⟩ ⟨Kind⟩ by ⟨role⟩… [where ⟨cond⟩]` | send an event to an existing root entity, handled by a `when` reaction |
| `admit change ⟨Kind⟩'s ⟨attr⟩ by ⟨role⟩… [where ⟨cond⟩]` | set one attribute of an existing entity |

The verb is yours: `borrow`, `place`, `request`. It is what a scenario and the gate log say.

### The condition

The condition reads the **target**: the new entity as it would be created (with its formulas and carried facts), or the existing entity. `the caller` is the caller's entity, and `the caller's name` one of its attributes. Using `the caller` in an admit whose role has no kind is a compile error, and so is using it in an admit whose roles are of different kinds.

The check reads at most two units, the target and the caller, so it is bounded.

### The gate

- An input is **admitted** if some `admit` matches its verb, kind (and attribute), role and condition. It then becomes a journaled step, like any other.
- Otherwise it is **refused at the gate** and changes nothing. The host's gate log keeps every decision with its reason: no admit for the role, or the condition that failed. A refused input never enters the journal.
- A program with **no** `admit` at all admits everything. That is how the earlier chapters' scenarios work.
- `given` in a scenario is the world, not an input: it never passes the gate.

## Events

```text
admit return Loan by member where …
when a member or librarian returns a Loan: the Loan is returned
```

`admit return Loan` admits the event `returns` on a Loan: the verb takes `-s` (or `-es` after s, sh, ch, x or z). A reaction `when a ⟨role⟩ [or ⟨role⟩] ⟨verb⟩s a ⟨Kind⟩` handles it, and `the Loan` is the target.

- Only the reactions that name the caller's role run.
- An admitted event with no reaction, a reaction to an event that is not admitted, and a reaction naming a role the admits do not, are compile errors.
- The reaction is a step like any other: if it would break a rule, it is refused, and the caller is told which rule. Renewing a returned loan is refused by rule `once`.

## Changes

```text
admit change Title's stock by librarian
```

A change sets the attribute to a value of its type. It is a step on the entity, checked by its rules, so lowering a title's stock below what is lent is refused by rule `stock`. A change also wakes the bindings waiting on that entity's pool (chapter 9), so adding copies lets a waiting loan go ahead.

Some attributes cannot be changed in v0, and admitting them is a compile error: references, sets, formulas, `created`, the attributes of a part, and any attribute a pool key reads.

### Watches

`suspended` is read by rule `suspended`, which lives in each Loan. The Loan carries the member's `suspended` (chapter 7). Because a librarian can change it, it is a **changeable** carried fact, and the host **watches** it: it remembers which loans carry each member's facts. When a librarian changes it:

1. Every reader is checked with the new value first. If a rule of any reader would newly break, the change is refused by that rule, naming the reader.
2. Otherwise the change applies, every reader is refreshed, and each runs again, so a step that was held back on the old value is tried again.

The cost is the number of readers, and `suture build` lists every watch:

```text
watches:
  Title's stock, changed by librarian: carried by Holds bound to a Title; each change is checked there first, then refreshed
  Member's suspended, changed by librarian: carried by Loans that refer to a Member; each change is checked there first, then refreshed
```

## In scenarios

```scenario
scenario "a member borrows and returns a book":
  given  a Member Mia with name Mia
  and    a Title Dune with name Dune and stock 1
  when   member Mia borrows a new Loan L1 with member Mia and title Dune
  expect the total copies taken from Dune is 1
  when   member Mia returns L1
  expect L1 is returned

scenario "a member cannot borrow in someone else's name":
  given  a Member Mia with name Mia
  and    a Member Ned with name Ned
  and    a Title Dune with name Dune and stock 1
  when   member Ned borrows a new Loan L1 with member Mia and title Dune
         is refused at the gate

scenario "only librarians add titles":
  given  a Member Mia with name Mia
  when   member Mia adds a new Title Dune with name Dune and stock 3
         is refused at the gate
  and    librarian Lin adds a new Title Dune with name Dune and stock 3
  expect Dune's stock is 3

scenario "a returned loan cannot be renewed":
  given  a Member Mia with name Mia
  and    a Title Dune with name Dune and stock 1
  when   member Mia borrows a new Loan L1 with member Mia and title Dune
  and    member Mia returns L1
  and    member Mia renews L1
         is refused by rule once

scenario "stock cannot drop below what is lent, and new stock wakes a waiting loan":
  given  a Member Mia with name Mia
  and    a Member Ned with name Ned
  and    a Title Dune with name Dune and stock 1
  when   member Mia borrows a new Loan L1 with member Mia and title Dune
  and    member Ned borrows a new Loan L2 with member Ned and title Dune
  expect the total copies taken from Dune is 1
  when   librarian Lin changes Dune's stock to 0
         is refused by rule stock
  and    librarian Lin changes Dune's stock to 2
  expect the total copies taken from Dune is 2
```

The forms are `when ⟨role⟩ ⟨Name⟩ ⟨verb⟩s a new ⟨Kind⟩ ⟨Name⟩ with …`, `when ⟨role⟩ ⟨Name⟩ ⟨verb⟩s ⟨Name⟩`, and `when ⟨role⟩ ⟨Name⟩ changes ⟨Name⟩'s ⟨attr⟩ to ⟨value⟩`. Each may be followed by `is refused at the gate` or `is refused by rule ⟨name⟩`, and the scenario fails if it is not refused that way.

`suture explain` lists the gate's decisions about an entity, with the caller of each admitted input and the reason for each refusal.
