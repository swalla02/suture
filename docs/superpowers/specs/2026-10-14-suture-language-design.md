# Suture language v0: design

- **Date:** 2026-10-14
- **Status:** each section was approved in conversation on 2026-10-13 and 2026-10-14. This document needs the user's review before an implementation plan is written.
- **Builds on:** the kernel (`docs/ir-spec.md`), Experiment DNC-1 (`docs/dnc.md`, `docs/dnc-experiment-1.md`), and a throwaway cost probe (Appendix A).
- **Supersedes:** the readme's v1 position "IR first, Rust-based definitions" (§2.9 and the *Syntax* row of the decision register). The IR now exists, and this is the language §2.9 deferred. It keeps §2.9's condition that a language must compile to the same IR.

## 1. Intent

In the user's words:

> software today is creating systems to enforce rules via paradigms. the goal here is to create a new paradigm with its own syntax so that the way software is built different fundamentally. instead of writing the if for whiles you define desired estates and behaviors and rules directly and system emerges. goal is repeatable, observable, consistent. AI currently is writing code so it did not fundamentally change the way we cbuild software. it just made it ervolve. My take is with these new decision models like jev we can do better and simpler

> suture needs to satisfy alot of other less funny things. such deployability, secruibility, extensibility, versionning , scalability, maintanability etc etc

> we are using only jev like models and those do not reason

Suture v0 is a language and a runtime. A developer writes:
- what exists (`kind`);
- what each thing needs (`needs`, `is … or …`, `uses`);
- what must always hold (`rule`);
- what must eventually be true, and by when (`desired`);
- what the outside world does (`effect`), and how the system reacts to it (`when`);
- which options are preferred (`prefer`);
- who may do what (`admit`).

The developer never writes the order of steps, status machines, retries, locks, compensation flows or crash recovery. The runtime does that work on the existing kernel.

Three properties are required:
- **Repeatable:** a journal replays to the same state.
- **Observable:** any entity can say why it is not done yet.
- **Consistent:** no rule is ever broken.

Suture works with no AI. Jev-like decision models (choose, score, yes/no; no reasoning, no text generation) are an optional extra at runtime choice points, and v0 has no model backend.

### 1.1 Success criteria

The first build succeeds when all five hold:
1. **No control flow.** The reference shop (§3.6) is written without any: orders, stock, payment, shipping, refunds, cancellation and lost parcels. Every line states a fact, a need, a rule, a goal, a reaction to an outside event, a preference or a permission.
2. **Consistent.** Across 10,000 simulated orders with misbehaving outside services, no rule is ever broken.
3. **Observable.** Every order that is not done has an explanation.
4. **Repeatable.** Every journal replays to the same final state.
5. **Simpler.** Compared with the same features built the conventional way (§8):
   - Suture needs about ten times fewer lines;
   - each rule lives in exactly one place;
   - three scripted changes take a line or two each.

   If Suture misses this, the write-up says so plainly.

The work-count bounds of §7.4 are acceptance tests as well.

## 2. Decisions

- **D1. Declare, never direct.** A program states facts, needs, rules, goals, reactions, preferences and permissions. Nothing in the language says what to do next.
- **D2. Two surfaces, one meaning each.**
  - Declarations (kinds, needs, effects, preferences, permissions) use a compact structured form.
  - Rules, goals, definitions, reactions, conditions and scenarios use controlled English: a fixed set of sentence patterns, each with exactly one meaning.
  - Calculations use plain math (`price × quantity`).
  - Every meaning has one canonical phrasing.
- **D3. Read-back.** For every rule, goal and definition, the compiler prints what it understood: the formula, a canonical paraphrase, and the cost tier (§3.4).
- **D4. A deterministic toolchain.**
  - Parsing, checks, read-backs, error messages and suggestions all follow fixed rules.
  - No model decides what a program means or whether it is valid.
  - The same input always gives the same output.
- **D5. No AI by default.**
  - Every open choice has a deterministic resolution (`prefer`).
  - A decision model is opt-in per choice. It is pinned by name and version, sees only that choice's inputs, and can only pick an allowed option. Every pick is journaled.
  - v0 builds the choice interface but no model backend (§5.7).
- **D6. Rules have cost tiers:** local, per-group or global (§4.3). A global rule is a compile error unless it is marked `checked globally`.
- **D7. Every mechanism has a work bound that does not depend on system size.** Tests check the bounds by counting work, not by timing (§7.4). The limits found by the probe in Appendix A are fixed by design.
- **D8. Rules are enforced by refusal.**
  - A step that would break a rule never happens.
  - Whatever was waiting on that step watches the facts the rule read and is retried when they change.
  - Nothing polls.
- **D9. The outside world is never assumed undone.**
  - Once an effect has been requested, its entity keeps counting toward rules and totals for good, whatever the outcome.
  - A timed-out request is re-sent with the same idempotency key.
  - A request that finally times out is never replaced by a new one automatically.
- **D10. Repeatable through the journal.**
  - Every input, answer, timer and choice is journaled, and the clock is an input like any other. Replaying a journal gives the same state.
  - Two live runs that receive inputs in a different order may make different, equally valid choices.
- **D11. The compiler emits data.**
  - The runtime interprets a program file with a fixed set of mechanisms built on the existing kernel. No code is generated.
  - `INTERACT` stays the only internal computation.
- **D12. Versioning default:** a deploy is refused until every rule that live entities would break has an explicit resolution (§6). v0 builds the hooks.
- **D13. Least authority.**
  - An adapter sees only the fields its effect declares, and can only answer with the declared outcomes.
  - `secret` values never enter the journal.

## 3. The language

### 3.1 Program files

- **Files.** A program is one or more `.st` files that share a `module` line. Lines that start with `--` are comments.
- **Order.** Declarations and sentences may appear in any order, and names resolve across all files.
- **Naming.** Kinds are capitalized (`Order`); attributes, roles and adapters are lower-case (`total`, `support`, `payments`).
- **Plurals.** A kind's plural adds `s`, or `es` after s, x, ch and sh. Irregular plurals are declared: `kind Person (plural People) { … }`.

### 3.2 Declarations

| Form | Meaning |
|---|---|
| `module shop version 1, currency EUR` | The program's name and version, and the single currency of `Money`. Optional settings are `attempts` (default 3) and `candidates` (default 8). |
| `role customer is a Customer`, `role support` | A role a caller can have. `is a Customer` ties the caller's identity to a Customer entity. |
| `kind Order { customer: Customer, cancelled: Bool = false, … }` | A kind of entity and its attributes. `= value` is a default; `= calculation` is computed once at creation. |
| `Order needs Payment, Delivery while it is not cancelled` | Each Order gets its own Payment and Delivery (parts) while the condition holds. |
| `Delivery is Shipment or Refund` | A slot that an owner fills with exactly one alternative, chosen by `prefer`. |
| `Shipment uses a Stock per line where … taking …` | Binds to existing, shared entities that meet the condition. |
| `effect capture Payment via payments within 30 seconds …` | A request to an outside service. |
| `prefer Carrier: lowest fee` | How an open choice is resolved, most important criterion first. |
| `admit cancel Order by customer where …` | Who may do an action, and under which condition. |

**Types**

| Type | Values |
|---|---|
| `Text` | strings |
| `Bool` | yes or no |
| `Count` | whole numbers, 0 or more |
| `Money` | amounts in the module's currency, exact to the cent |
| `Duration` | `30 seconds`, `2 days` |
| `Time` | instants from the runtime clock |
| a kind name | a reference to one entity |
| `list of K` | entities of kind `K` created together with this one (an Order's lines) |
| `set of K` | references to entities of kind `K` |

An attribute can be marked `personal` or `secret` (§6).

**Built-ins**
- Every entity has `created: Time`.
- Parts and slots have the states `done` and `failed`.
- An effect adds the state named by `marks`, its outcomes, and `timed out`.

**Defaults and formula attributes.** `= ⟨literal⟩` is a default, which the program may change later (`cancelled: Bool = false`). Anything else after `=` is a formula, computed once when the entity is created and never changed. That is how a fact is carried: an Order keeps the customer's country as it was at purchase. The compiler also carries any fixed fact that a local rule reads from another entity (§4.3).

**Parts.** `A needs B, C [while ⟨condition⟩]` gives each A its own B and C.
- **Ownership.** The part kind must have exactly one attribute of the owner's kind, and the runtime sets it.
- **Activity.** A need is active while its condition holds (always, if there is none).
- **Withdrawal.** When the condition turns false, an unfinished part is withdrawn and gives back what it reserved. A part that is done stays done.
- **Failure.** A part that fails with an explicit failure outcome (such as `lost`) is replaced, up to the `attempts` setting. After that, the need fails and the owner is told. A part that ends `timed out` is not replaced (D9): the need fails, and the goal's alert explains why.

**Slots.** `D is S or R` declares a slot that an owner fills with exactly one alternative, tried in `prefer` order. The slot moves to the next alternative when:
- the current one has failed on every attempt;
- its patience has run out (`then Refund after 2 days`);
- or a rule refuses it on facts that can never change.

A slot is `done` when its alternative is done.

**Shared entities.** `S uses a C [per ⟨list attribute⟩] where ⟨condition⟩ [taking ⟨amount⟩]` binds S to an existing C that meets the condition.
- `per line` makes one binding for each element of `lines`.
- `taking` records an amount against the chosen entity, and a per-group rule such as `stock` limits the total. The amount is named after the attribute it was taken as: `taking the line's quantity` is read back as `the total quantity taken from …`.
- Bindings count as needs: S is ready once all of them are bound.

**Effects**

```
effect ⟨verb⟩ ⟨Kind⟩ via ⟨adapter⟩ within ⟨duration⟩
  sends    ⟨field⟩[, ⟨field⟩…]
  [marks   ⟨state⟩]
  reports  ⟨outcome⟩ [with ⟨attr⟩: ⟨type⟩…] [| fails with ⟨outcome⟩[, ⟨outcome⟩…]]
```

- **Request.** The effect is requested once its entity is ready: it exists, its needs and bindings are done, and the request step passes every rule. From then on, the entity is in the `marks` state, if one is declared.
- **Success.** The success outcome makes the entity `done` and stores the outcome's data as attributes.
- **Failure.** A failure outcome makes the entity `failed`. Reactions to the outcome run before any replacement.
- **Deadline.** If the deadline passes without an answer, the request is re-sent with the same idempotency key, up to `attempts` sends in total. After that, the entity is `failed` with outcome `timed out`.
- **Required parts.** A deadline is required, and so is the list of fields the adapter may receive.

**Preferences.** `prefer ⟨choice⟩: ⟨criterion⟩[, then ⟨criterion⟩…]`.
- **Choice names:** a slot's name (`Delivery`), or the kind a `uses` binds to (`Carrier`). Add `for a ⟨element⟩` when that `uses` has `per` (`Stock for a line`).
- **Criteria:** an alternative's name (for slots), `lowest ⟨number⟩`, `highest ⟨number⟩`, or `those ⟨condition⟩` (candidates that meet the condition come first).
- **Patience:** `then ⟨alternative⟩ after ⟨duration⟩` sets how long the slot waits before moving on.
- **Ties** are broken by entity identity, which is deterministic.
- **Without `prefer`,** a choice uses declaration order and the compiler warns.

**Permissions**
- `admit ⟨verb⟩ a new ⟨Kind⟩[, ⟨Kind⟩… or ⟨Kind⟩] by ⟨role⟩[, ⟨role⟩] [where ⟨condition⟩]` lets callers create entities from an input's fields.
- `admit ⟨verb⟩ ⟨Kind⟩ by ⟨role⟩ … [where ⟨condition⟩]` admits an event on an existing entity, which `when` sentences handle.
- `admit change ⟨Kind⟩'s ⟨attr⟩ by ⟨role⟩ [where ⟨condition⟩]` lets callers change an attribute.
- Inside a condition, `the caller` is the caller's entity.

### 3.3 Sentences

This is the complete v0 grammar for sentences.
- **Placeholders:** `⟨np⟩` a noun phrase, `⟨cond⟩` a condition, `⟨num⟩` a number, `⟨state⟩` a state.
- **Subject:** a subject is `[⟨state⟩] ⟨Kind⟩ [whose ⟨attr⟩ is ⟨value or state⟩] [that ⟨cond⟩]`.
- **Formal meanings** use the core of §3.4.

**Counting.** An entity counts from its creation until it is withdrawn, or until it fails before any of its effects was requested. Relations and totals include every entity that counts (D9).

**Noun phrases**

| Pattern | Meaning |
|---|---|
| `the ⟨Kind⟩` | the sentence's subject, if it has that kind (in a `uses` condition, the candidate of the used kind); otherwise the subject's unique attribute of that kind |
| `it`, `its ⟨attr⟩`, `their ⟨attr⟩` | the subject, and its attributes |
| `⟨np⟩'s ⟨attr⟩` | an attribute |
| `⟨np⟩'s ⟨Kind⟩` (singular part or slot) | the current attempt |
| `⟨np⟩'s ⟨Kinds⟩` (plural) | every counting entity of that kind whose unique attribute of `⟨np⟩`'s kind points to `⟨np⟩` (`each Order's Refunds`) |
| `the line` | inside `per line`, the current element |
| `the caller` | inside `admit`, the caller's entity |
| literals | `10 EUR`, `3`, `"text"`, `2 days`, and instance names in scenarios |

**Conditions**

| Pattern | Meaning |
|---|---|
| `⟨np⟩ is ⟨np or literal⟩`, `⟨np⟩ is not …` | equality |
| `⟨np⟩ is ⟨state⟩`, `⟨np⟩ is not ⟨state⟩` | a Bool attribute, a defined state, a built-in state, or an effect state |
| `⟨num⟩ is at most`, `at least`, `more than`, `less than ⟨num⟩` | comparison |
| `⟨np⟩ contains ⟨np⟩`, `⟨np⟩ does not contain ⟨np⟩` | set membership |
| `⟨np⟩ has a ⟨Kind⟩ [that ⟨cond⟩]`, `has a ⟨Kind⟩ or a ⟨Kind⟩`, `has no ⟨Kind⟩` | at least one counting entity, or none |
| `⟨cond⟩ and ⟨cond⟩`, `⟨cond⟩ or ⟨cond⟩` | conjunction, disjunction. One sentence cannot mix `and` with `or`; name a part with `define` instead. |

**Numbers**

| Pattern | Meaning |
|---|---|
| `⟨np⟩` | a numeric attribute |
| `the total ⟨attr⟩ of ⟨np⟩'s ⟨Kinds⟩ [that ⟨cond⟩]` | sum over a relation |
| `the number of ⟨np⟩'s ⟨Kinds⟩ [that ⟨cond⟩]` | count over a relation |
| `the total ⟨attr⟩ taken from ⟨np⟩` | sum of the `taking` amounts recorded against an entity |
| `the total of ⟨calculation⟩ over ⟨list attribute⟩` | sum of a calculation over a list (in formula attributes) |
| `+`, `−`, `×`, `/`, parentheses | plain math |

**Rules**: `rule ⟨name⟩: ⟨sentence⟩`

| Pattern | Meaning |
|---|---|
| `every ⟨subject⟩ is ⟨state⟩` (also `has …`, `'s ⟨attr⟩ is at most …`) | for each matching entity, the statement holds |
| `no ⟨subject⟩ ⟨is …, has …⟩ [unless ⟨cond⟩]` | for each matching entity, the statement is false, unless the condition holds |
| `⟨num of each ⟨Kind⟩⟩ is at most ⟨num⟩` (and the other comparisons) | a bound for each entity: `the total amount of each Order's Refunds is at most its total` |
| `for each ⟨Kind⟩ and ⟨period⟩, ⟨num⟩ is at most ⟨num⟩` | a bound for each entity and calendar `day`, `week`, `month` or `year` (of `created`) |

**Goals**: `desired [within ⟨duration⟩]: every ⟨subject⟩ is ⟨state⟩[ and ⟨state⟩…]`. For each entity that matches the subject, the states must eventually all hold. The deadline runs from the moment the entity first matched.

**Definitions**: `define: a ⟨Kind⟩ is ⟨state⟩ when ⟨cond⟩`.

**Reactions**: `when ⟨event⟩: ⟨action⟩[ and ⟨action⟩]`.
- **Events:**
  - `a ⟨Kind⟩ is ⟨outcome⟩` (an effect outcome, including `timed out`);
  - `a ⟨role⟩ [or ⟨role⟩] ⟨verb⟩s a ⟨Kind⟩` (an admitted event).
- **Actions:** `⟨np⟩ is ⟨Bool attribute⟩`, `⟨np⟩ is not ⟨Bool attribute⟩`, `⟨np⟩'s ⟨attr⟩ becomes ⟨num or np⟩`.
- **Refusal.** An action is a step like any other. If it would break a rule, it is refused, and the event's sender is told which rule refused it.

**Scenarios**

```
scenario "⟨title⟩":
  given  a ⟨Kind⟩ ⟨Name⟩ with ⟨attr⟩ ⟨value⟩[, ⟨attr⟩ ⟨value⟩…] [and ⟨attr⟩ ⟨value⟩]
  when   ⟨role⟩ ⟨Name⟩ ⟨verb⟩s [a new] ⟨Kind⟩ ⟨Name⟩ [with …] [and a ⟨part⟩ with …]
  and    ⟨adapter⟩ answers ⟨outcome⟩ [with ⟨attr⟩ ⟨value⟩] [for ⟨Name⟩]
  and    ⟨duration⟩ pass
  expect ⟨cond⟩
```

`answers` applies to the oldest pending request of that adapter, unless `for ⟨Name⟩` picks one.

### 3.4 The formal core and read-back

Every sentence means a formula in a small typed logic:
- **Terms:** variables bound by `for each`, attribute paths (`o.country.restricted`), literals, `+ − × /`, `sum(x in R where φ: t)`, `count(x in R where φ)`, `taken(e).amount`, and the periods `day(t)`, `week(t)`, `month(t)`, `year(t)`.
- **Formulas:** comparisons, `in`, `and`, `or`, `not`, `implies`, `exists(x in R where φ)`.
- **Rules:** `for each x: K [, period p]: φ`, a state invariant.
- **Goals:** `for each x: K: φ implies eventually ψ [within d]`.
- **Definitions:** `s(x) iff φ`.

The read-back shows the formula, a paraphrase, and the cost tier:

```
rule cap: the total amount of each Order's Refunds is at most its total

  understood as:   for each Order o:  sum(r in o.refunds: r.amount) ≤ o.total
  in other words:  all refunds of one order, added up, are at most its total
  cost:            per-group (one running total per Order)
```

The paraphrase is generated from the formula with fixed templates that make each quantifier's scope explicit ("on its own", "added up"). `suture explain rule ⟨name⟩` prints a read-back, and every build prints the read-backs of new or changed rules.

### 3.5 Errors and suggestions

Every error uses a fixed template.
- **Closest pattern.** When a sentence matches no pattern, the compiler picks the pattern with the fewest differing words.
- **Suggestion.** It fills that pattern's slots with the user's words and the program's declarations, and shows the result.
- **Phrasing table.** A fixed table of common alternative phrasings ("can't be more than", "may not exceed") points to the canonical phrase. The table is used only for suggestions; programs always use the canonical phrase.

```
rule cap: refunds of an Order can't be more than its total

error in rule cap: "can't be more than" is not a Suture phrase
  closest pattern:  the total ⟨attr⟩ of each ⟨Kind⟩'s ⟨Kinds⟩ is at most ⟨num⟩
  did you mean:     the total amount of each Order's Refunds is at most its total
```

### 3.6 Reference program: the shop

```
module shop version 1, currency EUR

-- Who calls in
role customer is a Customer
role support
role staff

-- What exists
kind Country   { code: Text, restricted: Bool }
kind Customer  { name: Text, country: Country, address: Text personal }
kind Product   { name: Text, price: Money }
kind Warehouse { code: Text, country: Country }
kind Carrier   { name: Text, countries: set of Country, fee: Money }
kind Stock     { warehouse: Warehouse, product: Product, units: Count }
kind Line      { product: Product, quantity: Count, price: Money = product's price }
kind Order     { customer: Customer, lines: list of Line, card: Text secret,
                 country: Country = customer's country,
                 address: Text personal = customer's address,
                 total: Money = the total of price × quantity over lines,
                 cancelled: Bool = false }
kind Payment   { order: Order, amount: Money = order's total }
kind Shipment  { order: Order }
kind Refund    { order: Order, customer: Customer = order's customer,
                 amount: Money = order's total }

-- How an order gets done
Order     needs  Payment, Delivery  while it is not cancelled
Order     needs  Refund             while it is cancelled and paid and not refunded
Delivery  is     Shipment or Refund
Shipment  uses   a Carrier where the Carrier's countries contain the Order's country
Shipment  uses   a Stock per line where the Stock's product is the line's product,
                 taking the line's quantity

-- The outside world
effect capture Payment via payments within 30 seconds
  sends    amount, the Order's card
  reports  captured with reference: Text | fails with declined
effect send Shipment via carriers within 7 days
  sends    the Order's address, the Order's lines
  marks    sent
  reports  delivered | fails with lost
effect pay Refund via payments within 30 seconds
  sends    amount, the Order's Payment's reference
  reports  refunded

-- What "done" means
define: an Order is paid when its Payment is captured
define: an Order is settled when its Delivery is done
define: an Order is refunded when the total amount of its Refunds that are refunded is at least its total

-- Rules that always hold
rule stock:    the total quantity taken from each Stock is at most its units
rule embargo:  no Order whose country is restricted has a Shipment
rule paid:     no Order has a Shipment or a Refund unless it is paid
rule cap:      the total amount of each Order's Refunds is at most its total
rule late:     no cancelled Order has a Shipment that is sent
rule monthly:  for each Customer and month, the total amount of their Refunds is at most 1000 EUR

-- Goals
desired within 3 days: every Order that is not cancelled is paid and settled
desired within 1 day:  every Order that is cancelled and paid is refunded

-- Reactions to the outside world
when a Payment is declined:                   its Order is cancelled
when a customer or support cancels an Order:  the Order is cancelled

-- Choices
prefer Delivery:          Shipment, then Refund after 2 days
prefer Carrier:           lowest fee
prefer Stock for a line:  those whose warehouse's country is the Order's country

-- Who may do what
admit place a new Order by customer where the Order's customer is the caller
admit cancel Order by customer where the Order's customer is the caller
admit cancel Order by support
admit add a new Country, Product, Warehouse, Carrier or Stock by staff
admit change Stock's units by staff
```

How it runs, with no line above saying so:
1. **Payment.** A placed Order needs a Payment, so `capture` runs.
2. **Delivery waits.** Delivery tries a Shipment, but rule `paid` refuses it until the Order is paid. The Shipment watches the Payment instead of polling (D8).
3. **Shipment.** Once the Order is paid, the Shipment takes a Carrier and a Stock for each line, and `send` runs.
4. **Lost parcel.** A `lost` answer fails that Shipment, and Delivery tries again with a new one. The lost units stay taken (D9).
5. **No stock.** If no stock frees up within 2 days, Delivery switches to a Refund.
6. **Restricted country.** An Order to a restricted country refunds at once, because rule `embargo` refuses on facts that never change.
7. **Cancellation.** Cancelling releases whatever the Delivery reserved and starts a refund. Rule `late` refuses the cancellation once a Shipment has been sent.

`Country`'s `restricted` cannot be changed by any caller, so it is a fixed fact. Changing an embargo list means deploying a new version, and D12's check then lists the open orders it would affect.

What the compiler reports:

```
$ suture build shop.st
shop v1 → shop-v1.sut
  kinds 11 · roles 3 · effects 3 (adapters: payments, carriers)
  rules:
    stock     per-group   one running total per Stock
    embargo   local       an Order's country and its restricted flag are fixed, carried into each Order
    paid      local
    cap       per-group   one running total per Order
    late      local
    monthly   per-group   one running total per Customer and month
  open choices: 3
    Delivery of an Order: Shipment or Refund  → prefer: Shipment, then Refund after 2 days
    Carrier for a Shipment, by country         → prefer: lowest fee
    Stock for each line of a Shipment, by product → prefer: those in the Order's country
```

## 4. Compiler

### 4.1 Stages

1. **Parse** declarations and sentences with the fixed grammar of §3.
2. **Resolve** names and check types.
3. **Translate** sentences into formulas (§3.4).
4. **Analyze:**
   - who can change each attribute (admitted changes, reactions, effect outcomes, the runtime);
   - which facts each rule reads;
   - the cost tiers;
   - the open choices and their pool keys;
   - which facts are carried;
   - whether every goal is reachable.
5. **Emit** the program file (§4.5).

### 4.2 Checks

Every check is fixed. None uses a model.

| Mistake | Example |
|---|---|
| a sentence matches no pattern | "can't be more than" (§3.5) |
| an unknown kind or attribute, or a wrong type | `an Order's refunds` when Refund has no attribute of kind Order |
| a part kind without exactly one attribute of its owner's kind | Payment with no `order` attribute |
| needs that loop | Database needs Storage, and Storage needs Database |
| an open choice with no `prefer` | a warning; declaration order is used |
| an effect without a deadline or a `sends` list | `effect capture Payment via payments` |
| a global rule without `checked globally` | a daily cap on all refunds together |
| a `uses` condition with no equality or membership to key a pool | `where the Carrier's fee is at most 10 EUR` alone |
| a goal whose states nothing can produce | `every Order is shipped` with no `shipped` |
| a rule, condition or preference that reads a `secret` value | secrets can be sent to an adapter, never compared |

Two kinds of mistake are not compile errors:
- **A rule that means something other than intended** is caught by the developer reading its read-back.
- **Rules that together block every option, and goals that can never be met,** are caught by the seeded simulation (§7.2).

### 4.3 Rule cost tiers

**Cheap** means two things: the work of a check does not depend on the size of the system, and a check never makes unrelated work wait.

| Tier | When | How it runs |
|---|---|---|
| **local** | The formula reads only the subject's own attributes, its parts' states, and fixed facts of other entities. The compiler carries those facts into the subject when it is created. | An engine policy checks every step that creates or changes the subject. |
| **per-group** | The formula sums or counts over a relation whose group key is known at the step (the owner, or the owner plus a period). The same tier applies when it reads a changeable fact of another entity. | Each group key has a running total, and every change reserves against it. A changeable fact is watched: when it changes, every entity that read it is checked again. That costs the size of the group. |
| **global** | The formula needs every entity of a kind (there is no group key), or a single total that every change in the system must update. | Allowed only with `checked globally`. It then runs as one running total and is reported as a bottleneck. |

A uniqueness rule, such as "no two Orders share a reference", is per-group: the reference value is the key.

### 4.4 Open choices

Every slot is an open choice, and so is every `uses` that has more than one possible candidate. The compiler lists them all, each with its resolution (`prefer`, or declaration order) and the pool key it draws from.

A `uses` condition must contain at least one of these, to give the pool key:
- an equality, such as `the Stock's product is the line's product`;
- a membership, such as `the Carrier's countries contain the Order's country`.

### 4.5 The program file

`shop-v1.sut` contains:
- the name, version and content hash;
- as data: the kinds, needs, slots, pools, effects, rules (with formulas and tiers), goals, reactions, preferences and permissions;
- the manifest (§6).

A signature field is reserved and is filled in by sub-project 3.

## 5. Runtime

### 5.1 The kernel and its v0 changes

The runtime runs on the existing kernel. That means typed agents, `INTERACT` as the only internal computation, validated patches, policies, lineage identities and the journal. v0 changes the kernel in four ways:
1. **Effect queue.**
   - The machine appends each effect it creates to a queue, and the host takes only new effects from it.
   - `outbox()` iterates an index of pending effects instead of sorting every hole.
   - This removes the scan measured in Appendix A.
2. **Payload limit.** `Limits.max_payload_bytes` (default 4096) is checked for every agent a step or an inject creates. Collections live in the graph as agents, not inside payloads.
3. **Version record.** `Record::Version { program, version, hash }` is written when a program starts on a journal, and recovery checks it.
4. **Work counters.** `Stats` gains the counters the cost tests read: steps per operation, and effects examined per host call.

The mechanisms of §5.2 are generic rules that load at start and are parameterized by the program file. The depnet program of Experiment DNC-1 stays as a record of that experiment and is not reused. Its list walks are what Appendix A measured.

### 5.2 Mechanisms

"Bounded" means independent of system size. Each bound is set by the program (its declarations, `candidates` and `attempts`) and by the payload limit.

| Mechanism | What it does | Work per operation |
|---|---|---|
| **Entity** | One agent per entity. It holds the entity's attributes and the current state of its parts, and has its own mailbox label. | Delivery is one label lookup; a step reads and writes one entity. |
| **Part** | Creates the parts that active needs require and withdraws unfinished ones when a need's condition turns false. Failed parts are replaced up to `attempts`; after that, the owner is told. | Bounded per creation, withdrawal or failure. |
| **Slot** | Fills a slot with one alternative, in preference order, and moves on as described in §3.2. | Bounded by the declared alternatives. |
| **Pool** | Indexes shareable entities by the values of the `uses` condition's equalities and memberships. Each key has its own pool with a free list, and keeps the running totals for `taking` and for per-group rules. | Gathering costs the `candidates` setting plus the entries skipped within the same key. It never touches other keys. |
| **Choice** | Ranks the gathered candidates with `prefer` (or with a model later). The pick is a journaled token. Enacting it checks the pick again, and a stale pick is retried a bounded number of times. | Bounded by `candidates`. |
| **Effect** | Requests the outside action with the declared fields once its entity is ready, starts the deadline, records the answer as a fact, and re-sends timeouts with the same idempotency key. | Bounded per request and per answer. |
| **Watch** | A step refused by a rule, or a need with no candidate, waits on the facts or pool it read and is retried when they change. A pool wakes waiters in arrival order, only as many as the freed amount can serve. | Per change, bounded by the waiters it wakes. Nothing polls. |
| **Guard** | Local rules are engine policies on every step that creates or changes the subject. Per-group rules are running totals, reserved against by each change. Global rules are a single running total. | Local: bounded. Per-group: bounded per change, with contention only within a group. |
| **Reaction** | Applies a reaction's actions to its target when the event arrives. | Bounded. |
| **Goal** | Tracks the entities that match a goal: deadline timer, alert and explanation. | Bounded per entity, per change. |
| **Gate** | Checks the caller's identity and the `admit` condition before an input enters, and journals the decision. | Bounded. Conditions get cost tiers like rules. |

**Withdrawals and failures:**
- **Before any request.** When a part is withdrawn, or fails before any of its effects was requested, everything it reserved is released.
- **After a request.** Once an effect has been requested, the entity's bindings and totals stay (D9). A lost parcel's units stay taken, and a refund that finally timed out still counts against `cap`.

### 5.3 From declarations to mechanisms

| Declaration | Mechanisms |
|---|---|
| `kind` | Entity |
| `needs … [while …]` | Part, with a Watch on the condition |
| `is … or …` with `prefer` | Slot and Choice |
| `uses … where … [taking …]` | Pool (keyed by the condition) and Choice |
| `rule` | Guard, by tier. Local rules also use carried facts; changeable facts use Watch. |
| `define` | a derived state, recomputed by the owning Entity when its inputs change |
| `desired` | Goal |
| `effect` | Effect |
| `when` | Reaction |
| `admit` | Gate |

### 5.4 Host loop, adapters and clock

```
repeat:
  run the machine until it is quiescent or a step budget is spent
  take the new effects:
    an internal message → deliver to the addressed mailbox (an inject at its label)
    a decision site     → resolve it with prefer, inject the decision token
    an outside request  → hand it to its adapter with only the declared fields, start its deadline
  take adapter answers, gated inputs and due timers → inject each
  commit the journal
```

- **Clock.** The clock is an input. Timers sit in a queue ordered by due time, and a timer that fires is injected and journaled, so a replay is identical.
- **Adapters.** v0 has simulated `payments` and `carriers` adapters. They can decline, lose, delay, duplicate and reorder answers on a seed. A scenario adapter answers exactly as a scenario says.
- **Inputs.** `suture run` reads admitted inputs as JSON lines from a file or from stdin. Each input names its caller's role and identity. A network API comes with sub-project 3.

### 5.5 Failures

| What goes wrong | What happens | Where it shows |
|---|---|---|
| An outside service answers with a failure (`declined`, `lost`) | The answer is recorded as a fact. Matching reactions run, then the part is replaced if its need is still active. | the entity's history |
| An outside service never answers | The request is re-sent with the same idempotency key, up to `attempts` sends. Then the entity is `timed out`, and it is never replaced automatically (D9). | the history, and the goal's alert |
| A step would break a rule | The step is refused, and whatever needed it watches the facts the rule read. If those facts can never change, a slot moves on at once. | `suture explain` |
| No allowed candidate exists | The need waits on its pool. A slot moves on when its patience runs out. | `suture explain` |
| A goal stays unmet past its deadline | The runtime raises an alert that carries the explanation. | alerts, `suture explain` |
| An adapter sends a malformed answer | It is rejected at the door and logged, and the request keeps waiting for a valid answer or its deadline. | logs |

### 5.6 Explanations

`suture explain ⟨entity⟩` replays a journal file (or reads a running test) and prints the entity's goals, unmet needs, waiting reasons (the rule and the facts it read), pending requests with their deadlines, and its history. No model is involved.

```
$ suture explain O42
Order O42 · customer C7 · goal "paid and settled" due in 2 days 21 hours
  paid:     yes, Payment P1 captured (reference ch_9)
  settled:  no, Delivery is trying a Shipment (attempt 2 of 3; attempt 1 was lost)
    Carrier:          Parcelo, lowest fee
    Stock for line 1: waiting for a Stock of Kettle with 1 free unit
                      S1 refused by rule stock (5 of 5 units taken)
    next:             a Refund, if there is no Shipment by 16 Oct 14:02
```

### 5.7 Decision models (interface only)

Choice resolves every decision through one interface: candidates come in, each with the attributes its preference reads, and an index comes out, journaled as a token. v0 implements that interface with `prefer`.

A model backend can plug in later behind `decide ⟨choice⟩ with model "⟨name@version⟩"`, reusing the resolver in `src/dnc.rs`, with `prefer` as the fallback. Correctness never depends on it, because enactment re-checks every pick and the rules hold whatever is picked.

## 6. Versioning, security and deployment in v0

**Versioning**
- **v0:**
  - The program file carries the name, version and content hash.
  - The journal records the version (§5.1).
  - Recovery refuses a journal written by another version.
- **Later (sub-project 5):**
  - the deploy check against live state;
  - resolutions such as `apply to Shipments created from v2`;
  - replay across versions.

**Security**
- **Gate.** Every input passes the `admit` gate. In v0, callers are named by the input file or the test harness; real authentication comes in sub-project 4.
- **Secrets.** `secret` values live in a separate secret store. In v0 that is a local file readable only by the runtime's user. The journal records an opaque reference instead.
- **Personal data.** `personal` values are marked in the manifest and masked in explanations and logs. Per-customer encryption keys, so that erasure works, come in sub-project 4.
- **Manifest.** The manifest lists, for each adapter, the fields it receives and the outcomes it may report:

```
adapters:
  payments  receives  Payment.amount, Order.card (secret), Refund.amount, Payment.reference
            answers   captured (reference) | declined, refunded
  carriers  receives  Order.address (personal), Order.lines
            answers   delivered | lost
roles     customer (a Customer), support, staff
admitted  place Order (customer), cancel Order (customer, support),
          add Country, Product, Warehouse, Carrier, Stock (staff), change Stock.units (staff)
```

**Deployment**
- **v0:** `suture build`, `suture run` (one process, a local journal file, simulated adapters), `suture test` and `suture explain`.
- **Later (sub-project 3):** Postgres, journal snapshots, idempotency-key expiry, signing and `suture deploy`.

## 7. Testing

Everything runs through `suture test` and gives the same result on every run. Each test also fits on a small machine.

### 7.1 Scenarios

Scenarios are written in the controlled English of §3.3. For example:

```
scenario "a lost parcel is re-shipped from another warehouse":
  given  a Country DE with code "DE" and restricted no
  and    a Customer C with name "Ada", country DE and address "1 Main St"
  and    a Product P with name "Kettle" and price 30 EUR
  and    a Warehouse A with code "A" and country DE
  and    a Warehouse B with code "B" and country DE
  and    a Carrier K with name "Parcelo", countries DE and fee 5 EUR
  and    a Stock S1 with warehouse A, product P and units 1
  and    a Stock S2 with warehouse B, product P and units 1
  when   customer C places a new Order O with card "tok_1" and a line with product P and quantity 1
  and    payments answers captured with reference "ch_1"
  and    carriers answers lost
  and    carriers answers delivered
  expect O is settled
  expect the total quantity taken from S1 is 1
  expect the total quantity taken from S2 is 1
```

The shop's scenarios:
1. **Happy path.** Placed, captured, shipped and delivered, so it settles within the deadline.
2. **Declined payment.** The order is cancelled, nothing stays reserved, and nothing is refunded.
3. **Payment timeout, then capture.** One request is re-sent with the same key, and the customer is charged once.
4. **Lost parcel.** It is re-shipped from another warehouse (above).
5. **No stock anywhere.** A refund after 2 days.
6. **Restricted country.** An immediate refund.
7. **Cancel before shipping.** Reserved stock is released, and the order is refunded in full.
8. **Cancel after sending.** Refused by rule `late`.
9. **Cancel after a delivery refund.** No second refund.
10. **Monthly refund cap reached.** The refund waits, and an alert fires after 1 day.
11. **Staff lowers units below what is taken.** Refused by rule `stock`.
12. **A customer cancels someone else's order.** Refused at the gate.
13. **Duplicate and reordered adapter answers.** Each answer is applied once.

### 7.2 Simulation

The simulation runs 10,000 orders on a seed. The setup:
- random products and quantities;
- 3 to 5 warehouses and 2 to 3 carriers;
- `payments` declines 5% of requests, times out 5%, and duplicates 5% of answers;
- `carriers` loses 3% of parcels, delays answers up to their deadline, and reorders answers;
- customers cancel 5% of orders at random times;
- staff restock on a schedule.

Two checks run on it:
- **Rules.** An independent evaluator, separate from the mechanisms, recomputes every rule's formula over the whole state at checkpoints.
- **Goals.** After the run, every goal is either met or has an alert that carries an explanation.

Any failure replays from its seed.

### 7.3 Replay

Every test's journal is replayed, and the result must equal the live run's final state, digest for digest.

### 7.4 Cost tests

Each test counts work and does not time it.

| Test | Pass condition |
|---|---|
| **C1. Choice cost** | Steps per choice are the same whether the other keys hold 10, 1,000 or 100,000 entities. Within one key, steps stay within `candidates` plus the entries skipped plus a constant. |
| **C2. Host calls** | Taking new effects examines only new effects, whether 0, 10,000 or 100,000 are pending. |
| **C3. Steps** | No step reads more than its two meeting agents, and no payload exceeds the limit. |
| **C4. Groups** | N changes to N different groups have a critical path of 1 round. N changes to one group have a critical path of N (this documents the bottleneck). |
| **C5. Orders** | Steps per order, over its whole life, stay constant as total orders grow from 100 to 10,000. |

### 7.5 Compiler tests

- golden files for read-backs, error messages and suggestions, cost tiers, and open-choice lists;
- every example in this document as a test.

## 8. Evaluation

**The baseline.** We build the same features, permissions and deadlines conventionally: TypeScript with Postgres, done properly. That means:
- status columns and a state machine per order;
- a job queue with retries;
- an outbox for side effects;
- transactions and row locks for stock.

It is built after the Suture version, and the shop's scenarios are ported to it as tests.

**Measures**
1. **Lines of application code:** non-blank, non-comment, excluding tests and generated code.
2. **Enforcement sites:** for each rule, the number of places that check or maintain it.
3. **Three scripted changes** (lines added or changed):
   - add gift wrapping that carriers receive;
   - add a rule that fragile products ship only with carriers that insure them;
   - change the shipping preference.
4. **Scenario pass rate**, for both versions.

The results go to `docs/language-v0-results.md`, including any criterion Suture misses.

## 9. Scope and layout

### 9.1 In v0

- **Language:** everything in §3.
- **Compiler:** §4, with read-backs and suggestions.
- **Kernel changes:** §5.1.
- **Mechanisms:** §5.2.
- **Runtime:** host loop, clock, simulated adapters, `suture run` and `suture explain`.
- **Testing:** scenarios, simulation, replay and cost tests through `suture test`.
- **The shop,** passing all of the above.
- **The baseline and the evaluation** (§8).

### 9.2 Later

| Sub-project | What |
|---|---|
| 3. Durability and deployment | Postgres journal, snapshots, idempotency-key expiry, signing, `suture deploy`, a network input API |
| 4. Security | real authentication, per-customer keys for personal data, a production secret store |
| 5. Versioning | the live deploy check, `apply to … created from …`, replay across versions |
| 6. Tooling | editor completion, an inspector UI |
| 7. Scaling | parallel reduction, partitions across machines |
| Decision models | a Jev-like backend behind `decide`, compared with `prefer` |
| Second application | workspace provisioning (readme §4) |

### 9.3 Code layout

| Path | Contents |
|---|---|
| `/` | the kernel crate `suture`, with the changes of §5.1. The root `Cargo.toml` also becomes a workspace. |
| `lang/` | `suture-lang`: grammar, parser, checks, formulas, read-back, tiers, program file |
| `runtime/` | `suture-runtime`: mechanisms, host loop, clock, simulated adapters, explanations, scenario runner, simulation |
| `cli/` | `suture-cli`, which builds the `suture` binary |
| `examples/shop/` | `shop.st` and `scenarios.st` |
| `baseline/shop-ts/` | the conventional version |

### 9.4 Milestones

1. **M1. Kernel changes** (§5.1), with C2 and C3 on the existing nets.
2. **M2. Front end:** parser, checks, formulas, read-back, suggestions, tiers, open choices and the program file, with golden tests.
3. **M3. Mechanisms,** each with unit tests and its work-count test.
4. **M4. Runtime:** host loop, clock, simulated adapters, `suture run` and `suture explain`.
5. **M5. Testing tools:** scenario runner, simulation, replay, and `suture test`.
6. **M6. The shop:** program, scenarios, simulation and cost tests all pass.
7. **M7. Baseline and evaluation.** This milestone can get its own plan.

## 10. Risks and open questions

1. **Writing controlled English** may get harder as programs grow. The scripted changes measure this, and so does how often suggestions fix our own errors.
2. **Preferences rank only the gathered candidates** (at most `candidates`).
   - `prefer` is exact for criteria a pool can sort by, which are fixed attributes.
   - Criteria that change at runtime give the best of the candidates gathered.
   - Joint choices, such as the fewest warehouses per shipment, cannot be written in v0.
3. **Refusal can surprise.** Staff cannot lower units below what is taken, and a customer cannot cancel after a send. Explanations must make the reason obvious.
4. **A changeable fact read by many entities** makes each change of it cost the size of that group.
5. **The kernel is single-threaded in v0.** Unrelated work is independent in structure (measured as critical-path depth), not yet in parallel speed.
6. **The journal and idempotency keys grow without bound** until sub-project 3. v0 workloads stay small.
7. **The tenfold target may not hold.** The baseline decides.
8. **Whether decision models improve choices is untested here.** In DNC-1, a learned rule only matched a hand-written one.

## Appendix A. Cost probe, 2026-10-13

This probe was throwaway code (not kept) against the kernel and DNC-1's depnet. It ran on a machine with 8 cores and 3.8 GB of RAM, of which about 730 MB was free, alongside other load, so timings vary by about ±30%.

**What was cheap**
- A policy sees only the meeting pair and the agents a step creates (`src/policy.rs`).
- One operation (admitting a request, plus one step checked by a policy) cost 9.8–11.9 µs with no other orders, and 14.4–20.9 µs with 300,000 unrelated orders waiting. The drift came from memory effects, not from scans.
- A per-group running total behaved as expected. 1,000 updates spread over 1,000, 100, 10 and 1 totals gave a critical path of 1, 10, 100 and 1,000 rounds.

**What was not cheap**

| Where | Measured | Cause |
|---|---|---|
| Finding pending work | `outbox()` took 7.7–10.6 ms with 10,000 pending, 106–148 ms with 100,000, and 847–937 ms with 300,000 | It sorts every hole (`src/machine.rs`), and the depnet driver called it every round. |
| A choice in a depnet market | with 20 needs and 10, 100, 1,000 or 10,000 providers: 51, 142, 1,042 and 10,043 steps per need (0.96 ms to 292 ms) | The gathering walk visits the whole provider list, even after it has its candidates. |
| Many waiting needs | with 10 providers and 10, 50, 100, 200 or 400 needs: 40, 88, 150, 277 and 529 steps per need | Each decision walks the parked needs, and starting a root walks the root list. |
| Growing state | step time rose from 11.9 µs to 27.6 µs as one provider gained consumers | Every step copies both agents' payloads, and a provider's payload held its consumer list. |
| History | read from the code, not measured | The in-memory journal and the idempotency keys are never trimmed, and recovery replays from the first record. |

These findings shape D7, the kernel changes of §5.1, the keyed pools and free lists of §5.2, and the cost tests of §7.4.

## Appendix B. Prototype slice 1: conditional depth (2026-10-14)

The user asked for a working prototype as early as possible, and for "deeper" structure: structure that appears only in some contexts and carries its own rules. The example is a customs form that only orders to France get, with an excise section that only alcohol gets. Slice 1 builds that case end to end, before the choice machinery. It replaces plan 2's original scope (the whole front end), and later plans widen it.

### B.1 What runs

- `suture build <dir>` compiles every `.st` file in a directory, writes the program file, and prints each rule's read-back.
- `suture run <dir> --scenario <file>` runs scenarios against simulated services and prints a trace of every step.
- `suture explain <dir> --scenario <file> <name>` runs the scenarios, then explains one unit.
- `examples/shop/` holds `shop.st`, `france.st` and `orders.scenario`.

### B.2 Language in slice 1

| In slice 1 | Waits for slice 2 or later |
|---|---|
| `module`; `kind` with `Text`, `Bool`, `Money`, `Count`, owner references and literal defaults | formula attributes |
| `needs … while …` | `is … or …`, `uses`, `prefer`, pools |
| `effect` with `sends`, `marks`, `reports`, `fails with`; `within` is parsed | deadlines that fire, `timed out` |
| `rule`, local tier only | per-group and global rules |
| `define`, `desired` (its `within` is parsed) | goal alerts |
| `when ⟨Kind⟩ is ⟨outcome⟩` reactions | admitted events, `admit`, roles |
| scenarios with `when a new …`, `answers`, `expect` | `given` |
| errors with file and line | closest-pattern suggestions (§3.5) |

### B.3 Semantics that slice 1 settles

1. **Units.** A root entity, together with everything it owns through `needs`, runs as one engine agent (a unit) with one mailbox.
   - Every event for a unit is one checked, journaled step: its creation, or an answer.
   - All rules inside a unit are local.
   - This refines §5.2's "one agent per entity". Shared entities become their own units in slice 2.
   - A unit must fit the payload limit (4096 bytes).
2. **A part without an effect is done** when all its active needs are done.
3. **Unknown values.** An attribute that has no value yet (an answer not given) never breaks a rule. It makes a `while`, `define` or `desired` condition false.
4. **Refused answers.** If recording an answer would break a rule, the answer is refused.
   - The unit keeps its previous state, and the trace shows the rule's read-back.
   - The same request goes out again with the same idempotency key, so the person can correct their answer.
5. **Waiting.** A part's request that a rule refuses waits. Every later step of its unit checks it again; inside a unit, this re-check is §5.2's Watch.
6. **Host loop order.** run → take new effects → route internal messages → commit → dispatch outside requests. This corrects §5.4, which committed after dispatching, to match `ir-spec.md` §8.2.
7. **Text comparisons.** In `⟨np⟩ is ⟨word⟩`, a bare word is a text literal when ⟨np⟩ is a `Text` attribute (`its country is France`). Otherwise the word names a state.
8. **Names.** A step in a noun phrase is resolved in this order:
   - an attribute of the node (lower-case);
   - one of its parts (a capitalized kind it needs);
   - its owner, written either as the owner attribute (`form`) or as the owner's kind (`Order`).

### B.4 Acceptance

- `shop.st` and `france.st` compile, and every rule prints a read-back.
- **French order with alcohol:**
  - the customs form appears;
  - an over-declared value is refused, showing the read-back;
  - the excise section appears only for alcohol;
  - shipping waits for the payment and for the form;
  - the order settles.
- **Japanese order:** no customs form ever appears.
- Every scenario's journal replays to the same state.
- The `.st` files contain no control flow.

## Appendix C. Prototype slice 2: deadlines (2026-10-14)

Slice 1 parses `within` but never acts on it. Slice 2 makes time real: a request that gets no answer is sent again and then times out, and a goal that is not met in time raises an alert that explains why. The user chose this over choices and over clearing slice 1's minor findings first, and chose host-side timers (option A below).

### C.1 What runs

- Scenarios gain a step that moves the clock: `when 3 days pass` (also `seconds`, `minutes`, `hours`, `weeks`, singular or plural).
- Scenarios gain an expectation about alerts, `expect an alert for O1` and `expect no alert for O1`, and one about a unit's current part: `expect O1's Payment is timed out`.
- `suture run` traces every re-send, every time-out and every alert. `suture explain` also shows each pending request's deadline and the unit's alerts.
- `examples/shop/orders.scenario` gains three orders: a payment service that never answers, a payment service that answers only after one re-send, and an answer that arrives after its request timed out.

### C.2 Language in slice 2

| Added in slice 2 | Still waits |
|---|---|
| `within` on an effect fires | the `attempts` setting (fixed at 3, as in slice 1) |
| `timed out` is an outcome of every effect: usable in conditions, `define`, `desired` and `when` reactions | `prefer … then … after ⟨duration⟩` (with choices) |
| `desired within …` raises alerts | alerts sent anywhere but the trace and `explain` |
| scenario steps `when ⟨n⟩ ⟨unit⟩ pass`, `expect [no] alert for ⟨name⟩`, `expect ⟨name⟩'s ⟨Kind⟩ is …` | choices, `uses`, pools, per-group and global rules |

`timed out` is a state name like any other outcome. Slice 1's checker gives each state name one meaning per kind, so no attribute, mark or other outcome may also be called `timed out`.

### C.3 Semantics that slice 2 settles

1. **The clock is an input.** The host owns the clock. It starts at 0 and moves only when a scenario says time passes (later: when a real clock ticks). The units never read it.
2. **Timers live in the host** (option A). The host keeps one queue of timers ordered by due time, then by the order they were started.
   - When the clock moves forward by d, the host fires, one at a time and in queue order, every timer due no later than the new time. While a timer fires, the clock reads its due time, so a deadline started by that step is measured from it.
   - Firing a timer injects one message into its unit. That injection is a journaled step, like an answer, so a replay is identical.
   - A timer whose reason is gone (its request was answered, its goal met) fires as nothing: the host drops it without a step.
   - **Cost:** moving the clock does work for the timers that are due, plus a constant. It does not grow with the number of units that have nothing due. This is checked by counting kernel steps (`Stats`), not by timing.
3. **Deadlines.** When the host hands a request to its adapter, it starts a timer at now + the effect's `within`.
   - If the timer fires and the part is still requested, the request is sent again with the same idempotency key, and the trace notes the send number ("send 2 of 3").
   - After 3 sends with no answer, the part fails with outcome `timed out`. It is never replaced automatically (D9), so its need stays unmet, and the goal alert explains it.
   - A time-out is a fact, so it is always recorded. A `when … is timed out` reaction that would break a rule is held back, and the trace says so (`reaction held: rule …`). Nothing is ever asked again for a time-out.
   - An answer that arrives after its part timed out has no open request: it is ignored with a note, as in slice 1.
   - An answer to a request that was sent again is an answer to that request: same key, any send.
4. **Goal deadlines.** A goal's deadline runs from the step in which its entity first matched the goal's subject (§3.3).
   - The unit records which goals it has matched. When a step first matches one that has a `within`, the unit tells the host, which starts that goal's timer from the step's clock time.
   - If the timer fires and the goal is not met, the unit records an alert. The alert carries the explanation: the goal's read-back, and for each unmet state, the part that is missing, waiting (with the rule), failed or timed out.
   - A goal alerts at most once per unit. If it is met later, the trace notes it as met late.
5. **Replay.** Timer injections are journaled, so every scenario's journal still replays to the same state. The host's timer queue is not part of the kernel's state: in slice 2 nothing resumes a host from a journal.

### C.4 Acceptance

- **Payment service never answers:** the request is sent 3 times, 30 seconds apart. Then the Payment is `timed out`, the Shipment never goes out, and after 3 days the order raises an alert whose explanation names the timed-out Payment.
- **Slow payment service:** it answers after the first re-send. The order settles within its goal, with no alert. (A slow carrier cannot show this: its 7-day deadline is longer than the 3-day goal.)
- **Late answer:** an answer after the time-out is ignored with a note, and the unit is unchanged.
- **Slice 1 still holds:** the three existing shop scenarios pass unchanged.
- **Cost:** with one order that has a due timer, moving the clock costs the same number of kernel steps whether there are 1 or 1,000 other orders with nothing due.
- Every scenario's journal replays to the same state.

## Appendix D. Prototype slice 3: slots (2026-10-15)

Slice 3 adds the first kind of choice: a slot that an owner fills with one of several alternatives, tried in `prefer` order (§3.2). Choices among shared entities (`uses`, pools, `prefer … lowest …`) come in slice 4. The user agreed this split.

### D.1 What runs

- `Delivery is Shipment or Refund` declares a slot; `Order needs Payment, Delivery` needs it like a part.
- `prefer Delivery: Shipment, then Refund after 2 days` sets the order and the patience.
- `suture build` prints a warning for a slot with no `prefer`; it then uses declaration order (§3.2).
- `suture explain` shows each slot's current alternative.
- The shop grows toward §3.6: Delivery is a Shipment or a Refund. New scenarios: three lost parcels end in a refund, and a French order whose customs form is never filled is refunded after 2 days.

### D.2 Language in slice 3

| Added in slice 3 | Still waits |
|---|---|
| `⟨Slot⟩ is ⟨Kind⟩ or ⟨Kind⟩[ or …]` | `uses`, pools, `taking` (slice 4) |
| `prefer ⟨Slot⟩: ⟨Kind⟩[, then ⟨Kind⟩ [after ⟨duration⟩]]…` | `prefer` criteria `lowest`, `highest`, `those` (slice 4) |
| a slot in a `needs` list and in noun phrases (`its Delivery is done`) | moving on when a rule refuses on facts that never change |
| warnings from `suture build` | `decide … with model` backends |

Checks: a slot's name is capitalized and is not a kind; its alternatives are kinds that name the slot's owner, and none of them is also needed directly by that owner; a slot is needed by exactly one owner; a `prefer` names a slot and lists each of its alternatives once; `after` may follow every alternative but the first. A slot has the states `done` and `failed`, and a noun phrase cannot go past it (`its Delivery's …` is an error; name the alternative).

### D.3 Semantics that slice 3 settles

1. **Representation.** A slot is not a node. Its alternatives are ordinary parts of the owner, and the owner records the slot's current alternative. `its Delivery` reads the current part of that alternative; a slot is `done` or `failed` when that part is.
2. **Filling.** When the slot's need is active and the slot is empty, the choice point picks the first alternative in `prefer` order, and the part is created as for a need (held by rules as in slice 1).
3. **Moving on.** The slot moves to the next alternative when its current one:
   - has failed on its last attempt, or timed out (slice 2 never replaces a timed-out part);
   - or has waited past its patience without going out: a patience timer starts when the slot moves to the alternative (so a part that a rule holds back still has one), and when it fires, the slot moves on only if that alternative's part is missing or still untouched (never requested). A part that reached the outside world is not abandoned (D9).
   
   An untouched part is withdrawn when the slot moves on; a touched one stays (it counts, D9). With no alternative left, the slot stays on its last one, so it is `failed`, and the goal alert explains it.
   
   If moving on would break a rule, the slot stays where it is, and the trace says so (`patience held: rule …`). When a slot's need stops being active, its untouched part is withdrawn and the slot is emptied; if the need comes back, the slot is filled afresh and its patience starts again. Each fill has its own patience timer, and a timer from an older fill does nothing.
4. **The choice point.** Every pick goes through one function, which receives the slot and the alternatives still open and returns one of them. Slice 3 implements it with `prefer`; a `decide … with model` backend replaces it later (§5.7), and the rules still hold whatever it picks.
5. **Timers.** A patience timer is a host timer like a deadline (C.3.2): the unit asks for it, the host starts it, and its firing is a journaled step.

### D.4 Acceptance

- The shop compiles with `Delivery is Shipment or Refund` and `prefer Delivery: Shipment, then Refund after 2 days`, with no warnings; a slot without `prefer` compiles with a warning.
- **Three lost parcels:** Delivery moves to a Refund, which is paid, and the order settles.
- **A customs form that never comes:** the Shipment waits on rule `hold`; after 2 days it is withdrawn, the Refund is paid, and the order settles.
- **Patience never abandons a part that went out:** a Shipment already sent stays, whatever its patience.
- The slice 1 and slice 2 scenarios still pass. One expectation changed on purpose: an order whose payment never comes no longer keeps an unsent Shipment after 2 days (its Delivery moves to a Refund, which waits on rule `refund`).
- Every scenario's journal replays to the same state.

## Appendix E. Prototype slice 4: values and relations (2026-10-17)

Slices 1–3 were chosen by what the shop needed next. The user asked why the work followed the example rather than the whole language, so from slice 4 on, each slice is chosen from a construct-by-construct audit of §3, and is tested on small programs of its own and on two apps: the shop and workspace provisioning. The audit found that the largest gap is values and relations (numbers, math, totals, plurals), which shared entities depend on. Slice 4 builds the part of that which lives inside one unit; what needs other units moves to slice 5.

### E.1 What slice 4 adds

| Added in slice 4 | Moves to slice 5 (needs shared entities or a clock in sentences) |
|---|---|
| math: `+`, `−`, `×`, `/` (also `-`, `*`) and parentheses | `set of K`, `contains`, `does not contain` |
| `the total ⟨attr⟩ of ⟨np⟩'s ⟨Kinds⟩ [that ⟨cond⟩]`, `the number of ⟨np⟩'s ⟨Kinds⟩ [that ⟨cond⟩]` | `the total ⟨attr⟩ taken from ⟨np⟩` |
| plurals, with irregular ones declared: `kind Person (plural People) { … }` | `Time`, `Duration`, `created`, per-period rules |
| bounds for each entity: `the total amount of each Order's Refunds is at most its total` | |
| `whose ⟨attr⟩` with any predicate (`whose total is at least 100 EUR`), `has a ⟨Kind⟩ or a ⟨Kind⟩` | |
| formula attributes: `amount: Money = order's total`, computed once at creation | |
| `list of K`: entities given together with their owner, and `the total of ⟨calculation⟩ over ⟨list⟩` with `the ⟨element⟩` | |
| `⟨np⟩'s ⟨attr⟩ becomes ⟨num or np⟩` in reactions | |
| the module setting `attempts` | `candidates` (with `uses`) |
| scenarios: `and a ⟨element⟩ with …` in creation, `expect ⟨cond⟩` about any unit | |

### E.2 Semantics that slice 4 settles

1. **Relations.** `⟨np⟩'s ⟨Kinds⟩` is every part of that kind under the node, all attempts included, in creation order: a part counts until it is withdrawn (§3.3, D9), so three lost Shipments are three Shipments. `⟨np⟩'s ⟨Kind⟩` (singular) is still the current attempt. Failed attempts count too, because they reached the outside world (D9); a bound that should leave them out says so with a filter: `the total amount of each Order's Refunds that are not bounced`. A `that` filter is one condition (it never takes `and` or `or`, which belong to the sentence around it; a combined condition is named with `define`).
2. **Plurals.** A kind's plural is its name plus `s`, or `es` after s, x, ch and sh, unless declared. A plural is only valid after `'s` (`its Refunds`), and a list attribute names the same relation (`its lines`).
3. **Numbers.** Arithmetic works on `Money` and `Count`:
   - `Money ± Money`, `Count ± Count`, `Money × Count`, `Count × Money`, `Count × Count`, `Money / Count` and `Count / Count`; division rounds toward zero (to the cent for Money). Anything else is a compile error.
   - `×` and `/` bind tighter than `+` and `−`, left to right; parentheses group.
   - A value that is not known yet (B.3.3) makes the whole calculation unknown, and so does an overflow or a division by zero. An unknown calculation never breaks a rule and makes a condition false, as in slice 1.
   - A sum over no entities is 0.
4. **Bounds for each entity.** `⟨num with each K⟩ ⟨comparison⟩ ⟨num⟩` is a rule about every K; `its` in it means that K. It reads back with its scope made explicit, and its cost is per-group: one running total per K. In slice 4 the group always lies inside one unit, so it is checked like a local rule.
5. **Formula attributes.** After `=`, a literal is a default; anything else is a formula, computed once when the entity is created and never changed. It is computed from the entity's own given values, its other formulas, its list elements and its owner's attributes, after defaults, in whatever order they depend on each other. Formulas that read each other in a circle, or that read a value an answer sets later, or parts that a need creates after the entity exists, are compile errors. A formula whose value is unknown, or a negative count, leaves the attribute unknown.
6. **Lists.** `lines: list of Line` gives an entity a list of elements given with it at creation (a scenario writes `and a line with product tea and quantity 2`). The element kind names its owner like a part, is the element of one list only, has no needs or effects of its own in slice 4, and is `done` from the start. One element is named by the list's name without `s` or `es`, with `ies` made `y`, or by its kind's name (`entries` → `an entry`). `the total of ⟨calculation⟩ over ⟨list⟩` sums a calculation in which `the ⟨element⟩` (the list's name in the singular, `the line`) is each element in turn.
7. **`becomes`.** `⟨np⟩'s ⟨attr⟩ becomes ⟨num or np⟩` sets a value attribute when the reaction's event happens; the types must match. Like every action, it is refused if it would break a rule (B.3.4, C.3.3 for time-outs). A value that is unknown is not set, and the trace says so. A `Count` never becomes negative: that is refused too.
8. **`attempts`.** `module shop version 1, currency EUR, attempts 5` sets the number of attempts of a part and of sends of a request (default 3).
9. **Scenario conditions.** `expect ⟨cond⟩` takes any condition, in which a unit's name stands where a subject would (`expect P1's total is at most 90 EUR`, `expect the number of P1's Shipments is 3`). The names `expect … alert …` and the slice 1–3 forms still work.

### E.3 Acceptance

- Each row of E.1 has small programs that use it, and programs that misuse it get errors with file and line.
- **The shop:** a `cap` rule bounds refunds by the order's total; the number of an order's Shipments after three lost parcels is 3; formula attributes carry the order's total into a Refund.
- **Workspace provisioning** (`examples/workspaces/`): a workspace is requested with a list of members; it needs an account, a repository and a charge whose amount is a formula; a bound limits members to the plan's seats; a slot chooses storage; scenarios cover a workspace that is ready, one with too many members, and one whose account service never answers.
- Every scenario of both apps replays to the same state.

## Appendix F. Prototype slice 5: shared entities (2026-10-17)

Slice 5 makes units meet. Until now every unit was a root and its parts, and nothing read across units. Slice 5 adds entities that many units share (a Stock, a Carrier, a Customer), references to them, choices among them (`uses`, pools, `prefer lowest …`), amounts taken from them, rules about totals that span units (per-group and global tiers), sets, and time in sentences.

### F.1 What slice 5 adds

| Added in slice 5 | Still waits |
|---|---|
| references: an attribute whose type is a root kind (`customer: Customer`), and `set of K` | changing a shared entity's attributes (`admit change …`, slice 6) and the watches that changeable facts need |
| reading through references: `its customer's country`, `the Customer` for the subject's unique attribute of that kind | parts of a shared entity read from another unit |
| `⟨np⟩ contains ⟨np⟩`, `does not contain`, and equality of references | sets of values (`set of Text`) |
| `S uses a C [per ⟨element⟩] where ⟨cond⟩[, taking ⟨num⟩]`, and `its C` for the bound entity | `uses` with no equality or membership (a compile error, §4.4) |
| `the total ⟨attr⟩ taken from ⟨np⟩` | the free list and waking only as many waiters as the freed amount serves (§5.2) |
| `prefer C [for a ⟨element⟩]: lowest ⟨num⟩, highest ⟨num⟩, those ⟨cond⟩` (also `those whose …`), and the module setting `candidates` | `decide … with model` |
| relations across units: `their Refunds` from a Customer, when each Refund has one attribute of kind Customer | a filter on a relation across units (`their Refunds that are refunded`) |
| `for each ⟨Kind⟩ and ⟨period⟩, ⟨bound⟩`, and `all ⟨Kinds⟩` with `, checked globally` | |
| `Time`, `Duration`, the built-in `created`, durations in calculations | `now` in sentences (a rule must not change truth while nothing happens) |
| a slot moves on at once when a rule refuses its alternative on facts that never change | |
| scenarios: `given a ⟨Kind⟩ ⟨Name⟩ with …`, a set written `{DE, FR}` (or one name), references by name | callers and roles in `when` (slice 6) |

### F.2 Semantics that slice 5 settles

1. **Shared entities.** A root kind that no unit creates through `needs` can be shared: a scenario writes `given a Stock S1 with …`. A shared entity is a unit like any other (B.3.1), so its own rules are checked by every step that changes it.
2. **References.** An attribute whose type is a root kind holds a reference to that unit; `set of K` holds references to several. A reference names an existing unit of that kind, or the creation is refused. An attribute whose type is the owner's kind is still the owner (§3.2), and a reference to a part kind is a compile error.
3. **Carried facts.** A unit reads another unit only through facts carried into it (§4.3). When a unit is created, the host copies into it every unit it references, with that unit's own carried facts, so `its customer's country's restricted` is read inside the unit. A binding's chosen entity is carried in the same way when it is granted. In slice 5 no caller can change a shared entity's attributes, so every carried fact is fixed. Through a reference, only attributes can be read; a Bool attribute is the only state.
4. **`the K`.** In a sentence, `the K` is the subject if it has kind K, else its owner of that kind, else the subject's (or an owner's) unique attribute of kind K. In a `uses` condition and its `prefer`, `the C` is the candidate.
5. **Bindings.** `Shipment uses a Carrier where …` gives each Shipment one binding; `per line` gives it one for each element of the list called `lines` of the Shipment or of an owner of it (`the line` is that element). A binding is asked for when its entity is created, and the entity is ready (§3.2) only when all its bindings are granted.
6. **Pools.** The `uses` condition must contain, as one of its `and` parts, an equality `the C's a is ⟨x⟩` or a membership `the C's a contain(s) ⟨x⟩`, where `⟨x⟩` does not read the candidate: that part is the pool key. The host indexes the units of kind C by the key attribute, in creation order. The other parts of the condition filter the candidates of one key.
7. **The choice.** A binding is a decision site (§5.4). The host gathers up to `candidates` (default 8) entities of the key that pass the filter and have not refused this binding, and the choice point ranks them by the `prefer` criteria in order: `lowest x` and `highest x` by value (an unknown value last), `those …` meeting the condition first, then creation order. The pick is journaled. Without `prefer`, creation order is used and the compiler warns.
8. **Taking.** The pick is asked to take the binding's amount (`taking the line's quantity`, named after its last attribute: `quantity`). The picked unit adds the amount to its running total of that name and checks its own rules; if a rule would break, it refuses and the next candidate is asked. `the total quantity taken from each Stock` reads that running total, and a rule about it is per-group: one running total per Stock. A binding with no `taking` is granted by the pick unless a rule of the pick refuses it.
9. **Waiting.** When no candidate is left, the binding waits on its pool key, and its entity waits with it (an alert names it). A release to a unit of that key, or a new unit with that key, wakes the waiting bindings in arrival order; each one gathers again from scratch.
10. **Releases.** A part that is withdrawn, or moved on from by its slot, before any of its effects was requested gives back every binding it holds: the amount is subtracted from the pick's running total. After a request, bindings and amounts stay (D9), so a lost parcel's units stay taken and a replacement takes its own.
11. **Relations across units.** From a kind K, `their Refunds` (or `its Refunds`) means every Refund whose unique attribute of kind K points to the subject. Each such entity contributes to the subject's running total when it is created, exactly as an implicit `uses a K where the K is its k, taking ⟨its amount, or 1 for a count⟩`, so a rule over the relation is per-group and is enforced by refusing the contribution: the contributing entity waits, as in item 9. In slice 5 such a relation has no filter.
12. **Periods.** `for each Customer and month, ⟨bound⟩` keeps one running total per Customer and calendar month of each contributing entity's `created`; the periods are `day`, `week` (from Monday), `month` and `year`, in UTC. A scenario's clock starts at 2026-01-01 00:00 UTC.
13. **Global rules.** `all Refunds` is every Refund in the system. A rule over it has no group key, so it is a compile error unless it ends with `, checked globally`; it then runs as one running total in a single unit, and `suture build` reports it as a bottleneck.
14. **Time.** Every entity has `created: Time`, the clock when it was created. `Time − Time` is a Duration, `Time ± Duration` is a Time, durations add, subtract, multiply and divide by counts, and all of them compare. Duration literals are written as in effects: `2 days`.
15. **Fixed refusals.** A rule whose filter and `unless` read only fixed facts (given or formula attributes that no reaction changes, and carried facts) refuses on facts that never change. When such a rule holds back a slot's alternative, the slot moves on at once (§3.2).
16. **Cost.** Every gather, take, release and wake reads or writes a bounded number of units, set by `candidates` and the pool key. Nothing scans all units of a kind, except a rule `checked globally`, which is one running total.

### F.3 Acceptance

- Each row of F.1 has small programs that use it, and programs that misuse it get errors with file and line.
- **The shop** moves to §3.6's kinds: countries, customers, products, warehouses, carriers, stock and lines. Scenarios: an order is shipped from stock in its country with the cheapest carrier; a lost parcel is re-shipped from another warehouse with the lost units still taken; no stock anywhere refunds after 2 days; a restricted country refunds at once; a cancelled order releases its stock; a monthly refund cap makes a refund wait.
- **Workspace provisioning:** workspaces are placed on a cluster in their region with enough free seats, and a full region makes a workspace wait.
- `suture build` lists the open choices with their pool keys, and every rule's tier.
- Every scenario of both apps replays to the same state.

## Appendix G. Prototype slice 6: permissions (2026-10-17)

Slice 6 lets callers in. Until now a scenario created units and answered for adapters with no one asking. Slice 6 adds roles, the `admit` gate, admitted events with their reactions, admitted changes to attributes, and the watches that changeable carried facts need.

### G.1 What slice 6 adds

| Added in slice 6 | Still waits |
|---|---|
| `role r`, `role r is a K` | real authentication (sub-project 4) |
| `admit ⟨verb⟩ a new K[, K… or K] by r[, r] [where ⟨cond⟩]` | `suture run` reading JSON inputs (§5.4) |
| `admit ⟨verb⟩ K by r[, r] [where ⟨cond⟩]`, and reactions `when a r [or r] ⟨verb⟩s a K: …` | events with data (`cancels an Order with reason …`) |
| `admit change K's a by r[, r] [where ⟨cond⟩]` | changing a reference, a set, a calculated attribute or a pool key |
| `the caller` and `the caller's a` in an `admit` condition | `the caller` in rules and reactions |
| changeable carried facts: readers are refreshed and checked | refreshing carried facts that a shared entity's own reactions or answers change |
| scenarios: `when r N ⟨verb⟩s [a new] K M …`, `when r N changes M's a to ⟨value⟩`, `is refused at the gate` | |
| `suture build` lists roles, what is admitted, and the watches | |

### G.2 Semantics that slice 6 settles

1. **Roles.** A caller has a role and a name. For `role customer is a Customer`, the name is a Customer unit, and the input is refused at the gate if there is none. For a role with no kind, the name is only an identity (`support Sam`).
2. **The gate.** Every input passes the gate: a creation, an event or a change. It is admitted if some `admit` has its verb, its kind (and attribute), the caller's role, and a condition that holds. Otherwise it is refused at the gate and changes no unit. The host's gate log keeps every decision; an admitted input enters the journal with its step, and a refused one injects nothing, so replay never sees it. A program with no `admit` keeps the slice 1–5 behaviour: every input is admitted. `given` is the scenario's world, not an input, and never passes the gate.
3. **The condition.** It reads the target: the new unit as it would be created (with its calculated attributes and carried facts), or the existing unit. `the caller` is the caller's unit, and `the caller's a` reads one of its attributes; both are compile errors if one of the admit's roles has no kind. The check reads at most two units, so it is bounded.
4. **Admitted events.** `admit cancel Order by customer …` admits the event `cancels` on an Order; the verb takes `-s` (`-es` after s, sh, ch, x or z). A reaction `when a customer or support cancels an Order: …` handles it, and `the Order` is its target. An admitted event with no reaction, a reaction to an event that is not admitted, or a reaction naming a role that the admit does not, is a compile error. The reaction is a step like any other (§3.3): if it would break a rule, it is refused and the caller is told the rule. Events target root kinds.
5. **Admitted changes.** `admit change Stock's units by staff` lets a caller set that attribute to a value of its type. The change is a step on the unit, checked by its rules, so lowering units below what is taken is refused by rule `stock`. A change on a unit wakes the bindings waiting on its pool keys, as a release does (F.2.9). A reference, a set, a calculated attribute, a part's attribute, or an attribute that keys a pool (F.2.6) cannot be changed in slice 6 (a compile error).
6. **Changeable facts.** An attribute is changeable if an admitted change, a reaction or an effect outcome can set it; every other attribute is fixed (F.2.15). A rule that reads a changeable fact of another unit is per-group (§4.3), and never fixed. Only admitted changes refresh readers in this slice, so a rule that reads, from another unit, a fact that a reaction or an effect outcome sets is a compile error: its readers would keep a stale copy and the rule could break unseen.
7. **Watches.** The host remembers, for each unit, the units that carry its facts: those that reference it, and those whose bindings it granted. When an admitted change sets a carried attribute, every such reader is checked with the new value first. If a rule of any reader would newly break, the change is refused by that rule, naming the reader. Otherwise the change applies, every reader is refreshed, and each reader runs its pass again, so a step that was held back on the old value is tried again. The cost is the number of readers, which `suture build` reports for each watched attribute.
8. **Explanations** show the caller of every admitted input, and the gate's reason for every refusal: no admit for the role, or the condition that failed.

### G.3 Acceptance

- Each row of G.1 has small programs that use it, and programs that misuse it get errors with file and line.
- **The shop** gets §3.6's roles and admits. Scenarios: a customer places an order; a customer cancels someone else's order and is refused at the gate; support cancels an order before shipping, which releases its stock; a cancellation after sending is refused by rule `late`; staff lowering units below what is taken is refused by rule `stock`; staff raising units wakes an order that waited for stock.
- **Workspace provisioning:** admins add clusters and tenants place workspaces; resizing a cluster below its taken seats is refused, and adding a cluster wakes a waiting workspace.
- Every scenario of both apps replays to the same state.

## Appendix H. Prototype slice 7: errors and suggestions (2026-10-17)

Slice 7 makes mistakes cheap to fix. Until now an error said where parsing stopped and what it expected. Slice 7 adds §3.5: the phrasing table, the closest pattern, and a suggestion that is shown only when it compiles.

### H.1 What slice 7 adds

| Added in slice 7 | Still waits |
|---|---|
| contractions lex as one word (`can't`, `doesn't`), so they reach the phrasing table instead of failing the file | |
| the phrasing table: fixed alternative phrasings and their canonical phrase | phrasings learned from use (never: D4) |
| the closest pattern for a `rule` that matches no pattern, and `did you mean` filled with the user's words and the program's declarations | closest patterns for `define`, `desired` and `when` (they get the phrasing table only) |
| every statement of a file is parsed, so one bad sentence no longer hides the errors after it | |
| `did you mean` for a misspelt kind, attribute or state (at most two letters off) | |

### H.2 Semantics that slice 7 settles

1. **Fixed and deterministic.** The table, the patterns, the scoring and the tie-breaks are fixed code. No model is used (D4). The same program always gets the same errors.
2. **The phrasing table** maps a phrase to its canonical form: `can't be more than`, `may not exceed`, `must be at most` and `is no more than` to `is at most`; `must be at least` to `is at least`; `is greater than` to `is more than`; `is smaller than` to `is less than`; `equals` and `is equal to` to `is`; `isn't` and `must not be` to `is not`; `doesn't have`, `may not have` and `cannot have` to `has no`; `may have` and `must have` to `has`; `the sum of` to `the total`; `how many` to `the number of`; and a few more. Words compare ignoring case, and the longest phrase wins. A program that uses a table phrase is an error, `"⟨phrase⟩" is not a Suture phrase; write "⟨canonical⟩"`; programs only use the canonical phrase.
3. **Patterns.** The rule patterns are the forms of §3.3: `every ⟨Kind⟩ is ⟨state⟩`, `every ⟨Kind⟩'s ⟨attr⟩ is at most ⟨num⟩`, `no ⟨Kind⟩ has a ⟨Kind⟩ unless ⟨cond⟩`, `no ⟨Kind⟩ has a ⟨Kind⟩ that is ⟨state⟩`, `no ⟨Kind⟩ has a ⟨Kind⟩ that is ⟨state⟩ unless ⟨cond⟩`, `no ⟨Kind⟩ whose ⟨attr⟩ is ⟨value⟩ has a ⟨Kind⟩`, `the total ⟨attr⟩ of each ⟨Kind⟩'s ⟨Kinds⟩ is at most ⟨num⟩`, `the number of each ⟨Kind⟩'s ⟨Kinds⟩ is at most ⟨num⟩`, `the total ⟨attr⟩ taken from each ⟨Kind⟩ is at most ⟨num⟩`, and `for each ⟨Kind⟩ and ⟨period⟩, the total ⟨attr⟩ of their ⟨Kinds⟩ is at most ⟨num⟩`. The comparison in a pattern takes whichever of the four the user wrote.
4. **The closest pattern** is the one with the fewest differing words, after the phrasing table. Word order is ignored. A user word fills a slot if it fits: a declared kind for `⟨Kind⟩`, a declared plural (in any case) for `⟨Kinds⟩`, a declared attribute for `⟨attr⟩`, a declared state for `⟨state⟩`, a period for `⟨period⟩`; `⟨num⟩` and `⟨cond⟩` take what follows the comparison or `unless`. Every other word counts as differing, on either side, except `a`, `an`, `the`, `each`, `every` and `'s`. Ties go to the pattern listed first.
5. **Did you mean.** A slot no user word fills is filled from the declarations, where one choice is clear: the `⟨attr⟩` of a total is the first Money attribute of that kind (else the first Count), and of `taken from` the binding's amount. The suggestion is shown only if the program compiles with it in place of the rule, and only if it keeps every `no` and `not` the user wrote and everything after their `unless` or comparison, since dropping either says something else. Otherwise only the closest pattern is shown, and a table phrase names its canonical phrase (`; write "⟨canonical⟩"`).
6. **Names.** An unknown kind, attribute or state at most two letters away from exactly one declared name gets `did you mean ⟨name⟩?`.

### H.3 Acceptance

- The §3.5 example gives exactly the error shown there.
- Golden tests (§7.5): a file of wrong sentences, each with its expected message, checked word for word.
- No suggestion that is shown fails to compile, and a file with several bad statements reports all of them.

## Appendix I. Prototype slice 8: sourcing (2026-10-18)

Slice 8 lets a decision depend on facts that live outside the system. Until now every fact was given at creation, carried from a shared entity, or reported by an effect's answer. Slice 8 adds **sourced** attributes. Each one says where its value comes from, how long a value is trusted, and what to do when a fresh value cannot be had. The compiler knows exactly which rules read which sourced facts. The runtime fetches a fact only when a step held back by such a rule needs it.

```text
kind Order { customer: Customer, total: Money, credit: Money, cancelled: Bool = no }

source Order's credit via bureau within 2 seconds
  sends      the customer's name
  reports    found | fails with unknown
  good for   1 day
  otherwise  use 0 EUR

rule credit: no Order has a Payment that is sent unless its total is at most its credit,
             with credit at most 1 hour old
```

### I.1 What slice 8 adds

| Added in slice 8 | Still waits |
|---|---|
| `source K's a via ⟨adapter⟩ within ⟨duration⟩` with `sends`, `reports ⟨found⟩ [| fails with …]`, optional `good for ⟨duration⟩` and `otherwise …` | sourced attributes of shared entities (referenced, or a `uses` target) |
| per rule: `…, with a at most ⟨duration⟩ old[ and b at most …]` | one source for several attributes; several sources for one attribute |
| recovery: `otherwise use ⟨literal⟩`, `otherwise ask again after ⟨duration⟩`, `otherwise ⟨action⟩[ and ⟨action⟩]` | `decide sources … with model` (I.3) |
| strict checking of the rules that read sourced facts | fetching ahead of need |
| scenarios: `when ⟨adapter⟩ answers found with a ⟨value⟩` | |
| `suture build` lists every source and the rules that read it; `explain` shows each sourced value's age | |

### I.2 Semantics that slice 8 settles

1. **A sourced attribute** is declared in its kind like any other (`credit: Money`) and named by one `source` statement. It has no default and no formula. It is never given at creation, changed by an `admit change` or set by a reaction: only its source sets it. Its kind must not be shared (referenced by another kind, or the target of a `uses`) in this slice. All of these are compile errors.
2. **The source** is a request like an effect's: `sends` says what the adapter receives, `within` is the deadline, and requests are re-sent with the same key up to the module's `attempts` (C.3.3). The success answer carries the value as data named after the attribute, of its type (`found with credit 500 EUR`). Failure outcomes and time-outs are failures. The key is the node's key, the attribute and the fetch's number (`O1~credit#2`), so each fetch is answered once and a late answer is ignored.
3. **Fresh.** A value is fresh while its age is within its `good for` (no `good for`: for ever) and, when a rule reads it, within that rule's `with a at most ⟨duration⟩ old`. A rule's bound only narrows; it never stretches `good for`. Freshness is read only when a step checks a rule before acting; observers (`while`, goals, `explain`, scenario checks) read the last value.
4. **Strict rules.** A rule that reads a sourced attribute, directly or through a definition, is strict. When it guards an action (creating, finishing or requesting a part), a value that is unknown, or not fresh for the rule, counts against the action: a filter that might hold applies, an `every` that might fail is broken, and a `no … has` that might be present is present. So no step ever acts on a fact it cannot confirm. Answers, deadlines, events and changes are facts and are checked as before (B.3.4), with the last values.
5. **When to fetch.** In each step, every part that waits on a strict rule fetches each sourced value the rule reads that is not fresh for it and has no open fetch. Nothing is fetched for a rule that holds nothing back. When a value's `good for` runs out, the unit gets a timer step, so a part that still waits fetches again. The trace says why: `credit: asking bureau` or `credit: older than 1 hour for rule credit: asking bureau again`.
6. **Recovery.** A fetch fails after a failure outcome or after its last send times out. Then:
   - `otherwise use ⟨literal⟩` sets the value as if fetched now, marked as a fallback in `explain`;
   - `otherwise ask again after ⟨duration⟩` waits that long, then fetches again if a part still waits;
   - `otherwise ⟨action⟩[ and ⟨action⟩]` applies reaction actions at the sourced node (`it is cancelled`, `its Order is held`). The cascade follows from the needs: a `while` that no longer holds withdraws every unfinished part and gives back what it took, and new needs may start;
   - with no `otherwise`, the value stays unknown and the fetch stays failed: every step its rules guard waits, and a goal's alert says `because credit could not be sourced from bureau`.
7. **Deterministic.** Fetches, answers, time-outs and recovery are journaled steps, so a scenario's journal replays to the same state. A sourced value never changes while nothing happens; only its freshness does, and freshness only holds steps back.
8. **Read-backs.** A strict rule's read-back adds `sources: credit from bureau, at most 1 hour old`. `suture build` adds a `sourcing:` section listing each source with its adapter, what it sends, how long values are trusted, the recovery, and the rules that read it.

### I.3 Models for sourcing and choices (slice 9, appendix J)

The compiler lists every open choice; a model may only pick among those options (D4). The sourcing choices are which source to ask when several are declared, whether to fetch ahead of need, and whether a still-fresh value is reused. A model never adds a source, skips a fact a rule reads, or stretches a lifespan; enactment re-checks every pick.

Models are named per choice point: `decide ⟨choice point⟩ [for a K | whose …] with model "name@version"`. A choice point is a slot, a `uses` binding, or a source. A scope narrows it by a condition on the entity, or by a module file. The most specific scope wins, an overlap where neither is more specific is a compile error, `prefer` is the fallback when no `decide` applies or the model does not answer in time, and `suture build` lists the handler of every choice point. The model is pinned in the manifest, and every pick is journaled with the options it had.

### I.4 Acceptance

- Each row of I.1 has small programs that use it, and programs that misuse it get errors with file and line.
- **A credit check:** an order fetches its customer's credit before its payment goes out; a low credit holds the payment; a stale credit is fetched again when an approval comes late; a bureau that fails falls back to 0 EUR, which holds a large order; `otherwise it is cancelled` withdraws the shipment and gives back its stock; `otherwise ask again after 1 hour` recovers.
- No step acts on a sourced fact that is not fresh for its rule, and every scenario replays to the same state.

## Appendix J. Prototype slice 9: models at choice points (2026-10-18)

Slice 9 lets a program say which decision model handles which choices, at any level: the whole app, one workflow, one step of a workflow, one decision, or any of these under a condition. A model only picks among the options the compiler and the runtime list; `prefer` ranks those options, and remains the answer whenever no model applies, the model declines, or it picks something it was not offered (D4). Models are decision models only: they choose, never generate (spec §1).

```text
decide with model "planner@3"                                         -- app: every choice in the module
decide for Orders with model "orders@2"                               -- workflow: choices in units whose root is an Order
decide for Shipments with model "shipping@1"                          -- step: choices a Shipment, or a part of it, makes
decide Carrier with model "carriers@1"                                -- decision: one choice point, wherever it is made
decide Stock for Shipments with model "stock@4"                       -- a decision within a step
decide Carrier for Orders whose total is at least 1000 EUR with model "careful@1"
decide Carrier for Orders whose country is local with prefer          -- back to `prefer` for this scope
```

### J.1 What slice 9 adds

| Added in slice 9 | Still waits |
|---|---|
| `decide [⟨choice point⟩] [for ⟨Kinds⟩ [whose ⟨cond⟩]] with model "name@version"` | models choosing a slot's alternative (slots keep `prefer` and patience) |
| `… with prefer`, to take a scope back from a model | models for sourcing (which source, fetching ahead); slice 8 has one source per attribute |
| the most specific `decide` wins; an overlap neither side wins is a compile error | a deadline for a model's answer (the v0 stand-in answers at once) |
| a host registry of models by name and version; a scripted stand-in for scenarios | real model backends |
| scenarios: `when model "name@version" picks K2[, then K3…]` | |
| `suture build` lists the handler of every choice point; the trace names the model of every pick | |

### J.2 Semantics that slice 9 settles

1. **A choice point** is a `uses` binding, named by its target kind (`Carrier`, `Stock`). A `decide` that names none applies to every choice point in its scope. Naming a kind that no `uses` picks, or a slot, is a compile error.
2. **Scopes.** With no `for`, a `decide` covers the module: the app level. `for Ks` covers the choices made inside every K: by a K itself or by any part under it. When K is a root kind, this is a whole workflow; when K is a part kind, one step of it. `whose ⟨cond⟩` narrows the scope to the Ks for which the condition holds when the choice is made. The condition reads the K like any `define` (its own facts, its parts, its owners); a condition that is unknown does not hold.
3. **Specificity.** For one choice, every `decide` that applies is ranked, in this order: naming the choice point beats not naming it; then a scope on a kind nearer the choosing part beats one further up (the part itself, then its owner, …, then the root, then the module); then a `whose` beats no condition. The highest-ranked one handles the choice. The compiler checks every choice point against every chain of owners that can make it: two `decide`s of the same rank that can both apply are an error (two with `whose` at the same rank always count as overlapping in v0). Two `decide`s that are the same apart from their model are an error too.
4. **What a model sees and does.** The runtime gathers the options exactly as before (F.2.7): candidates that meet the binding's condition, up to `candidates`, in `prefer` order. The model is given the choice point, the unit, the choosing part's key, and the options with their attributes and their rank under `prefer`. It answers with one option's name or declines. A pick that is not among the options, an unknown model, and a decline all fall back to `prefer`'s first, and the trace says why.
5. **Enactment re-checks every pick.** A picked candidate is still asked to grant the take (F.2.8); if it refuses, the option is dropped and the same handler is asked again among the rest, as `prefer` would be.
6. **Pinned and journaled.** Every `decide`, with its model's name and version, is part of the compiled program, so a journal does not replay under a program with another model (§5.4). The pick is injected into the journal as before, so replay never calls the model again.
7. **The stand-in.** In v0 the host holds a registry of models by `name@version`. Scenarios play them as they play adapters: `when model "carriers@1" picks Hermes[, then DHL]` queues picks, and each time that model is asked (a choice, a re-ask after a refused take, a woken waiter) it takes the next one; with none queued, it declines. A binding with no pool key (F.2.11) chooses nothing and is not a choice point. Scripting a model that no `decide` (or, from slice 30, no source) names fails the scenario. Rust callers may register any function from the options to a pick.
8. **Read-backs.** `suture build` lists each choice point with its handlers, most specific first, ending in `prefer`. A pick in the trace reads `choice: K2 for O1/Shipment#1@Stock by model "stock@4" (of K1, K2)`; a fallback reads `choice: K1 for … by prefer (model "stock@4" declined)`.

### J.3 Acceptance

- Each level (app, workflow, step, decision, condition) has a program in which it handles a choice, and a scenario in which a nearer level overrides a further one.
- Overlaps, unknown choice points, slots named in a `decide` and duplicate `decide`s are compile errors with file and line.
- A pick outside the options, a decline, an unknown model and a refused take each fall back as J.2.4–J.2.5 say, and every scenario replays to the same state.

## Appendix K. Prototype slice 10: every choice, deadlines and real backends (2026-10-18)

Slice 10 finishes the model story of appendix J. Models may also pick a slot's alternative and, when an attribute has several sources, which source to ask. Every model answer has a deadline. A real backend speaks the Jev decision API by default, and other APIs through a custom format. Correctness still never depends on a model: `prefer` stands behind every pick, and every pick is checked and journaled.

```text
Delivery is Shipment or Refund
prefer Delivery: Shipment, then Refund after 2 days
decide Delivery with model "routing@1" within 300 milliseconds

source Order's credit via bureau within 5 seconds ...
source Order's credit via backup within 2 seconds ...
decide Order's credit with model "sources@1"
```

### K.1 What slice 10 adds

| Added in slice 10 | Still waits |
|---|---|
| `decide ⟨Slot⟩ …`: a model picks a slot's alternative | fetching ahead of need |
| several `source`s for one attribute, tried in turn; `decide K's a …` picks which | a model choosing to reuse a value or to fetch early |
| `decide … within ⟨n⟩ milliseconds` or `seconds` (default 1 second, at most 10 minutes) | asynchronous model calls (a model is called in the step that needs it) |
| scenarios: `when model "m@1" declines`, `when model "m@1" is late` | |
| backends: Jev (default), a custom JSON format, or any Rust backend; `models.json` beside the module, read by `suture run --models` | secrets beyond an environment variable |

### K.2 Semantics that slice 10 settles

1. **Choice points** are now: a `uses` binding with a pool key (named by its target), a slot (named by its name), and a sourced attribute with more than one source (named `K's a`). A `decide` that names none covers them all in its scope. The chooser is the part that needs the binding, the owner of the slot, or the entity that holds the sourced attribute; scopes and ranking are those of J.2.2–J.2.3, along the chooser's chain of owners.
2. **A model is asked only when there is a choice:** with one option left, that option is taken and no model is asked.
3. **A slot.** Its options are its alternatives not yet tried since the slot was last filled afresh, in `prefer` order. A slot is filled when its need becomes active, and moves on when its part fails for good, is refused on fixed facts, or runs out of patience (D.3). Each fill and move asks the slot's handler. While a model's pick is awaited, the slot is empty and nothing is created for it; the pick arrives as a journaled message to the unit, which checks it against the options (or takes `prefer`'s first) and fills the slot. Patience stays per alternative.
4. **Several sources.** All sources of one attribute give its type, and their order of declaration is the `prefer` order. A fetch asks one source. When it fails (a failure outcome, or its last time-out), the next is chosen by the handler among those not yet tried in this fetch; when every source has failed, the `otherwise` applies. Only one source of an attribute may have an `otherwise`, and the value's `good for` is that of the source that answered.
5. **Deadlines.** `within ⟨duration⟩` on a `decide` bounds each answer of its model; without it, 1 second. A model that does not answer in time, like one that declines, fails or is unsure (its confidence is below the backend's `min_confidence`), leaves the pick to `prefer`, and the trace says why. The answer, or its absence, is what the journal holds, so replay never waits or calls again.
6. **Backends.** The host maps each `name@version` to a backend. The default backend speaks Jev's decision API (`POST https://api.typesafe.ai/v1/systemone`): it sends `{"model", "state", "questions": {"pick": {"type": "choice", "instructions", "criteria"}}}`, where the state is the chooser with its facts, the criteria name each option with its facts, and the instructions give `prefer`'s order (a JSON object has none); it reads `answers.pick.choice` and `answers.pick.confidence`. A custom format sends the same state with the options as a list, and reads the pick (and, optionally, the confidence) at JSON pointers it names. Rust callers may register any backend. A backend's transport is replaceable, so tests never reach the network.
7. **Configuration.** `models.json` in the module's directory maps model names to backends, for example `{"routing@1": {"backend": "jev", "model": "jev-latest", "key_env": "JEV_API_KEY", "min_confidence": 0.6}, "sources@1": {"backend": "custom", "url": "…", "key_env": "…", "pick": "/result/pick", "confidence": "/result/p"}}`. It is read only with `suture run --models`; without it, a model that the scenario does not script is not available. Endpoints and keys are host configuration, not program text: changing them does not change the program, while changing a model's name or version does.

### K.3 Acceptance

- A model picks a slot's first alternative, and the next after a failure and after patience; a decline, a late answer and an unknown model each fall back to `prefer`.
- An attribute with two sources asks the one a model picks, moves to the other when it fails, and applies its `otherwise` when both fail.
- The Jev backend builds the documented request and reads the documented response through a fake transport; low confidence and transport errors fall back. A custom format reads the pick at its pointer.
- Every scenario replays to the same state.

## Appendix L. Prototype slice 11: serving a program (2026-10-18)

Until slice 10, a program ran only inside a scenario: the inputs, the outside answers and the clock all came from the scenario, and nothing outlived the run. Slice 11 makes a program a service. `suture serve` runs it as a long-lived process, as one pod among the others (see `docs/architecture.svg`). It takes admitted inputs over HTTP, calls adapters over HTTP, follows the real clock, and keeps a log on disk, from which a restart rebuilds the same state.

### L.1 What slice 11 adds

| Added in slice 11 | Still waits |
|---|---|
| `suture serve <dir> [--state <dir>] [--listen <addr>] [--models]` | several runtime pods sharing the work |
| the input log: every input, clock move and model answer, one JSON line each, written before the input is answered | versioning: a deploy with live units under a changed program (refused for now) |
| restart: the log replays into the same state; picks are not asked again | compaction or snapshots of the log |
| an HTTP API: inputs from callers, the world and adapter answers from the operator, and reads | authentication beyond a gateway's caller header and an operator token |
| `adapters.json`: each request posted to its adapter, answered at once or later | TLS in the host itself (the gateway terminates it) |
| the real clock, which fires timers when they fall due | sub-second timers |

### L.2 Semantics that slice 11 settles

1. **The state directory** (default `<dir>/.suture/`) holds `program.json`, the compiled program the log was written under, and `log.jsonl`. Its first line is a header, `{"suture": 1, "module", "version", "started"}`, where `started` is the Unix time of the clock's zero. Each later line is one record: `{"pass": secs}`, `{"world": …}`, `{"new": …}`, `{"event": …}`, `{"change": …}`, `{"answer": …}`, or `{"model": {"key", "answer"}}`.
2. **One input at a time.** Inputs are applied in the order they arrive, one at a time; the log's order is that order. Before an input is applied, the clock is moved to the current time. An input is written to the log, with the model answers it caused, after it is applied and before its caller is answered. An input that is refused is written too, since the gate's decisions are part of what happened. An input whose caller got no answer may have been lost, and may be sent again. Each write is synced. If the log cannot be written, the service takes no more inputs until it restarts, since memory no longer matches the log; a last line cut short by a crash is cut off on restart.
3. **Restart.** On start, a state directory with a log is replayed: each record is applied in order, and each model answer comes from the log, not from the model. A log written under a different program is refused, with the module, its version and what to do: start with a fresh state directory, until versioning (a later slice) can resolve the live units.
4. **The clock.** The runtime clock reads whole seconds since `started`. Once a second, the host fires every timer that has fallen due, as one `pass` record. After a stop, the first tick passes the downtime, so the deadlines that ran out meanwhile fire then, in order.
5. **Callers.** An input from a caller carries the header `Suture-Caller: <role> <name>`, which the gateway in front of the host sets after it has authenticated the caller. The gate decides as in G.2. Operator requests (`/world`, `/answers`) carry `Authorization: Bearer <token>`, the value of `SUTURE_OPERATOR_TOKEN`; without that variable, they are refused, and an empty one is an error. A request with more than one `Suture-Caller`, `Authorization` or `Content-Length` header is refused. Reads need no token, so they too must be reachable only through the gateway.
6. **The API.** The bodies are JSON.
   - `POST /inputs` takes `{"new": {"verb", "kind", "label", "attrs", "items"}}`, `{"event": {"verb", "target"}}` or `{"change": {"target", "attr", "value"}}`. It answers 200 with the trace lines the input caused, 409 with the refusal, or 400 with the error.
   - `POST /world` takes `{"kind", "label", "attrs", "items"}`, which is a scenario's `given`.
   - `POST /answers` takes `{"adapter", "outcome", "key" or "target", "data"}`, which is an adapter's answer to the request with that key, or to the oldest for that unit.
   - `GET /pending` lists the requests that wait for an answer.
   - `GET /units/<label>` explains a unit.
   - `GET /trace?from=<n>` gives trace lines from the nth on.
   - `GET /health` answers ok.
7. **Values in JSON** follow the attribute's type: text as a string, a yes or no as a boolean, a count as an integer, an amount as a string with its currency (`"80 EUR"`, `"12.50 EUR"`), a duration or a time as an integer number of seconds (since slice 37, ISO 8601: AM), a reference as the entity's label, and a set as a list of labels.
8. **Adapters.** `adapters.json` in the module's directory maps an adapter's name to `{"url", "key_env"}`. Once an input is written to the log, each new request is posted to its adapter's URL as `{"key", "adapter", "verb", "kind", "unit", "sends"}`. A reply with an `outcome` (and `data`) is that adapter's answer, applied and logged as one. A reply without one means the adapter answers later through `/answers`. A failed post is tried again every 5 seconds while the request waits, and its effect's deadline decides what happens next. An adapter that `adapters.json` does not name is reached through `GET /pending` and `/answers` only. On restart, every waiting request is posted again with the same key, which the adapter uses to do the work once.
9. **Models.** With `--models`, `models.json` configures the backends as in K.2.7.

### L.3 Acceptance

- A served program applies inputs, refuses what the gate or a rule refuses, and answers with the trace.
- Stopping and starting again from the log gives the same units, the same trace and the same pending requests, and no model is asked again.
- A log written under a different program is refused.
- Time passing on the real clock fires a deadline; downtime fires the deadlines it covered.
- A request is posted to its adapter; an immediate reply is applied, and a later one arrives through `/answers`; restart posts waiting requests again with the same key.

## Appendix M. Prototype slice 12: versions and deploys (2026-10-18)

D12 says a deploy is refused until every live break has an explicit resolution. Slice 12 builds that. A served program can be replaced by a newer version of the same module. The deploy check runs the new version against every live unit and lists what it would break. A resolution in the new program's text says how each break is handled. Once nothing is left unresolved, the live units move to the new version in place, and nothing that already happened is undone.

### M.1 What slice 12 adds

| Added in slice 12 | Still waits |
|---|---|
| `apply rule ⟨name⟩ to ⟨Kinds⟩ created from version ⟨n⟩` | resolutions for rules over totals (`per-group`, `global`) |
| every unit records the version it was created under | migrating a value by a formula (`set Booking's seat to …`) |
| `suture deploy <dir> [--state <dir>]`: the deploy check against the live state, without changing it | a deploy while the service runs (stop, then start the new version) |
| `suture serve` with a newer version: the check, then the deploy, logged | going back to an older version |
| replay across versions: a log holds each deploy, and the state directory each version's program | |

### M.2 Semantics that slice 12 settles

1. **Versions.** A deploy replaces a program with another of the same module and a higher version. Different content under the same version is refused: it asks for a new version number. The units' kinds, attributes, parts and requests carry over by name.
2. **A unit's version.** A unit records the version of the program it was created under. A deploy does not change it.
3. **`apply rule R to Ks created from version n`** limits rule R to the units of root kind K created under version n or later. Older units are not checked against R, and R does not hold them back. Only a rule about one entity at a time (`local`) can be applied this way. K must be the rule's subject or own it, and n must be from 1 to the module's version. Each rule may have only one such statement. `suture build` prints it under the rule's read-back.
4. **The deploy check** builds the new version's view of each live unit and lists every break:
   - a live unit of a kind, or with a part of a kind, that the new version does not have;
   - an attribute whose type changed while live units hold a value of it;
   - an attribute the new version adds without a default, on a kind with live units;
   - a rule of the new version that a live unit breaks, unless an `apply` limits the rule to versions after the unit's.
   
   Each break names the units (the first few, then how many more) and how it can be resolved. A deploy with any break is refused, and nothing changes.
5. **What a deploy does.** Attributes the new version removes are dropped from live units, and the report says so. Added attributes take their defaults, and added formulas are computed; a changed formula keeps its old values. The facts units carry are carried over too, and the check reads them so. The pools are rebuilt, and waiting bindings follow their `uses` (a removed one with waiters is a break). Goals and rules are matched by their text, so a goal already met or alerted stays so. Then every live unit settles once under the new version, and the messages they send each other are delivered only after all have. A new `needs` may create parts, a request already sent stays sent, and its answer is awaited as before. The trace and the report say what changed.
6. **The log.** A deploy is the record `{"deploy": {"version": n}}`. The state directory keeps each version's program as `program-v⟨n⟩.json` (the first stays `program.json`). Replay applies the records under the version in force, and at a deploy record it makes the same deploy again. A deploy is checked at the time `suture serve` starts it, after the catch-up pass, and is written only if the new program took over; its program file and the directory are synced first.
7. **`suture deploy`** replays the state directory, runs the check and prints the report or the breaks. It exits with failure if the deploy would be refused. It never writes, so it can run in a pipeline before the new version is started.

### M.3 Acceptance

- A rule that live units break refuses the deploy and names them. `apply … created from version 2` resolves the break: old units are not checked against the rule, and new ones are.
- A removed kind with live units, a changed type and an added attribute without a default are each refused. With a default, the attribute is filled.
- After a deploy, a new need creates parts on live units, and a pending request is answered as before.
- A restart replays across the deploy to the same state. The same version with other content is refused.

## Appendix N. Prototype slice 13: data marks and the manifest (2026-10-18)

D13 asks for least authority: an adapter sees only the fields its effect declares, and `secret` values never enter the journal. Slice 13 builds the marks of §3.2 and §6, the explicit grant of a marked field to an adapter, and the manifest.

### N.1 What slice 13 adds

| Added in slice 13 | Still waits |
|---|---|
| `card: Text secret`, `address: Text personal`: marks on attributes | per-customer keys for personal data, so that erasure works |
| a secret is kept out of the unit, the journal, the log, the trace and explanations; the unit holds a reference | a production secret store (Vault, a KMS) |
| `allow ⟨adapter⟩ to receive ⟨Kind⟩'s ⟨attr⟩[, …]`: a marked field is sent only where it is granted | masking in adapters' own logs |
| personal values masked in the trace, explanations and the service's reads | |
| the manifest: `suture build` prints, and writes beside the program file, what each adapter receives (with its marks) and may answer, the roles and what is admitted | |
| a served program keeps its secrets in `secrets.json` in the state directory, readable only by its user | |

### N.2 Semantics that slice 13 settles

1. **Marks.** An attribute may be marked `secret` or `personal`, after its type and before a default or formula: `address: Text personal = customer's address`. Only a `Text` attribute can be secret. A list, a reference or a set cannot be marked.
2. **A secret is never compared.** A rule, a condition (of a need, a `uses`, an `admit`, a definition, a goal or a reaction), a preference, a decision's scope and a total cannot read a secret. A formula can copy a secret only into another secret. A secret cannot be sourced, and a pool cannot be keyed by it. Each is a compile error naming the attribute.
3. **A marked field is granted.** An effect whose `sends` reads a secret or personal attribute needs `allow ⟨adapter⟩ to receive ⟨Kind⟩'s ⟨attr⟩`, naming the effect's adapter and the attribute as the kind declares it. Without it, the effect is a compile error that says what to write. A grant that no effect uses is a warning. An adapter whose answer sets a secret attribute (`reports issued with token`) needs no grant: it is the source of the value.
4. **A secret is held by reference.** When a secret value comes in (with a new entity, a change or an answer), the runtime keeps it in its secret store and puts a reference (`secret:⟨n⟩`, numbered in arrival order) in the unit. Steps, the journal, the log, the trace and explanations only ever see the reference, shown as `(secret)`. Only a request posted to an adapter that was granted the field carries the value itself.
5. **Personal values are masked** as `(personal)` in the trace, in explanations and in the service's `/units`, `/trace` and `/pending` reads. They are kept as they are in the unit and the log, which replay needs. A request to a granted adapter carries the value.
6. **The secret store.** A scenario or `suture run` keeps secrets in memory. `suture serve` keeps them in `⟨state⟩/secrets.json`, created with mode 0600 and synced before the log line that refers to them is written. A restart reads it back. A log line whose reference is missing from the store is an error at replay.
7. **The manifest** is derived from the program: for each adapter, what it receives (`Payment.amount`, `Order.card (secret)`), what it may answer, with the data each answer carries; then the roles and what is admitted. `suture build` prints it and writes it as `⟨module⟩-v⟨n⟩.manifest`, for the security review of a deploy.

### N.3 Acceptance

- A rule, condition, preference or formula reading a secret is refused, as is a secret that is not `Text`.
- An effect sending a marked field without a grant is refused, with the `allow` line to write.
- A secret given at creation reaches the granted adapter, and appears nowhere else: not in the unit, the log, the trace or the explanation. A restart reads it back from `secrets.json`.
- A personal value is masked in the trace and explanations, and sent to its granted adapter.
- `suture build` prints and writes the manifest.

**N.2.8 Review updates.** An answer with no pending request (late or stray) is sealed by attribute name, so the log never holds a secret it carries. A deploy that adds or removes a `secret` mark on an attribute live units hold is a break (appendix M.2.2).

## Appendix O. Prototype slice 14: pods (2026-10-18)

Slice 11 serves a program as one process. Slice 14 runs it as several pods over one state directory, so that a pod can fail without stopping the service, and the requests to adapters are shared out among the pods.

### O.1 What slice 14 adds

| Added in slice 14 | Still waits |
|---|---|
| `suture serve --pod ⟨name⟩`: several pods over one state directory, on a volume they share | a log replicated over the network (Raft or a database), with no shared volume |
| one log, written by one pod at a time under a lock; every pod keeps up with it | units held by one pod only, so that state is split, not copied |
| the units are shared out among the live pods; a pod posts requests only for its own | snapshots, so a pod does not replay the whole log when it starts |
| failover: a pod that stops beating loses its units to the others, which post their waiting requests again | |
| any pod takes any input, so the gateway may route anywhere | |

### O.2 Semantics that slice 14 settles

1. **One log, one order.** The log stays the only state. A pod appends to it only while it holds an exclusive lock on `⟨state⟩/log.lock`. Holding the lock, it first reads what other pods appended since it last read, and applies it as a replay does (§L.2.3), the model answers included. Then it applies its own input and appends it, with the model answers it caused, before it lets the lock go. Inputs are thus applied in one order, the log's, by every pod.
2. **Every pod holds all of the state.** Each pod applies every line of the log, so its units, trace and explanations are those of the whole program. Reads (`/units`, `/trace`, `/pending`) first catch up under a shared lock, so they see every input acknowledged before them. Since replay is deterministic (§L.2.3), every pod comes to the same state.
3. **The secret store is shared.** `secrets.json` is written under the log lock before the line that refers to it. When a pod catches up, it reads the store again after reading the new lines, so every reference it applies is found.
4. **Pods and their beats.** A pod writes `⟨state⟩/pods/⟨name⟩` every second with the Unix time. A pod is live while its last beat is less than 10 seconds old. A pod that stops cleanly removes its file.
5. **Each unit has one owner.** The live pods are sorted by name; a unit belongs to the pod at the index of the 64-bit FNV-1a hash of its label, modulo their number. A pod posts a request to an adapter only for the units it owns. Ownership is decided anew each time the pod looks, so a pod that comes or goes moves units at once.
6. **Failover.** When a pod stops beating, its units go to the others, which post their waiting requests, as a restart does (§L.2.8). For a short while, two pods may both think they own a unit, and post its request twice. This is why a request carries its idempotency key: an adapter must answer the same key once (§L.2.8, unchanged).
7. **Time.** Any pod may fire the timers that fell due. Holding the lock, it catches up first, so a `pass` already made by another pod is not made twice.
8. **Deploys.** A pod started with a newer version deploys it under the lock, as `open` does (§M.2.6). Other pods meet the `deploy` line when they catch up and apply it from `program-v⟨n⟩.json`. A pod started with an older version than the log's is refused, as before.
9. **One process alone.** Without `--pod`, `suture serve` is a pod named `solo`, and takes the lock in the same way. So two processes on one state directory never corrupt the log, whether or not they were told about each other.

### O.3 Acceptance

- Two pods on one state directory take inputs in turn; each sees the units the other made, and a third pod (or a restart) that replays the log comes to the same state.
- With two live pods, each request is posted by exactly one of them. When one stops beating, the other posts that pod's waiting requests.
- A secret given to one pod reaches its adapter through the pod that owns its unit.
- A timer that falls due fires once, whichever pods tick.
- A line cut short by a pod that crashed is cut off by the next pod to take the lock.

**O.2.10 Review updates.** Every request, not only a read, catches up with the log first, so an input is read against the units and the program other pods made. A pod whose write failed takes nothing, serves nothing and stops beating, so its units go to the others; it comes back only by restarting and replaying the log. A pod forgets the requests it posted for units it no longer owns, so it posts them again if they come back. A pod beats while it posts, so a run of slow posts does not let its beat lapse.

## Appendix P. Prototype slice 15: editor support (2026-10-18)

§7.4 asks for an editor that completes sentences, shows each statement's read-back, and marks errors where they are. Slice 15 builds it as a language server, so that any editor with an LSP client gets it.

### P.1 What slice 15 adds

| Added in slice 15 | Still waits |
|---|---|
| `suture lsp`: a language server over stdin and stdout | a packaged VS Code extension |
| errors and warnings as diagnostics on their lines, for every file of the module | ranges narrower than the line; quick fixes from `did you mean` |
| hover: the read-back of the statement under the cursor, or of the kind named there | go to definition, find references, rename |
| completion: the words the grammar takes next, with the program's kinds, attributes, adapters and states, the canonical phrases, and statement templates on an empty line | completion inside scenario files |

### P.2 Semantics that slice 15 settles

1. **A module is a directory.** The server compiles the `.st` files of the directory an open file is in, as `suture build` does, with the open files as the editor has them, not as saved. Each change publishes the diagnostics of every file of the module, so a file whose error was fixed elsewhere is cleared.
2. **Diagnostics.** An error is shown on its line. Warnings name no line, so they are shown on the `module` line.
3. **Completion is the grammar's.** A word is offered if the statement, with the word added at the cursor, parses, or fails only at its end, wanting more. The words tried are the grammar's own (read from the parser's source, so the list cannot fall behind it), the program's names, and the canonical phrases of the phrasing table (§H.2.2), offered whole or, after their first word, as the rest (`at most` after `is`). Where the grammar takes any word (a name), a made-up word fits too: there, only the program's names of the right sort are offered (adapters after `via` and `allow`; kinds and states elsewhere), not keywords. After `Kind's`, only that kind's attributes are offered; after `its`, any kind's. Nothing is ranked by a model (D4).
4. **Hover is the read-back.** On a rule: what the compiler understood, in other words, and its cost, as `suture build` prints them. On a kind's name, anywhere: its attributes with types, marks, defaults and formulas, its parts, its effect and the rules about it. On an effect: what it sends and may answer, from the manifest (§N.2.7). On `allow`, `role` and `admit`: the adapters part of the manifest and the roles. While the text has errors, hover and completion use the module's last program that compiled.

### P.3 Acceptance

- An error appears on its line as the text is typed, and goes when it is fixed.
- After `every Booking's`, the completions are Booking's attributes; after `is`, the comparisons; after `via`, the program's adapters.
- Hovering a rule shows its read-back; hovering a kind's name shows the kind.

**P.2.5 Review updates.** Only `.st` files are part of a module, whatever else the editor opens beside them. A blank or comment line has no read-back, and nothing is completed inside a comment. Typing `'` after a kind's name offers its attributes, inserting `s` and the attribute.

## Appendix Q. Prototype slice 16: events with data, and the caller in reactions (2026-10-19)

An admitted event (§G.2.4) has, until now, carried only its verb and its caller. A cancellation could not say why, and a refund could not say how much, without an admitted change beside it. Slice 16 gives events data, and lets an event's reactions read the data and the caller.

### Q.1 What slice 16 adds

| Added in slice 16 | Still waits |
|---|---|
| `admit refund Order by support with amount: Money, note: Text` | references, sets and lists as event data |
| `the amount` in the admit's condition and in the event's reactions | event data read by rules or kept in the trace of later steps |
| `the caller` and `the caller's name` in an event's reactions | the caller in rules (by design, §Q.2.5) |
| `support Sam refunds O1 with amount 30 EUR and note ok` in scenarios; `"data"` in an HTTP event | |

### Q.2 Semantics that slice 16 settles

1. **An event's data is declared on its `admit`.** `with name: Type, …` follows the roles and comes before `where`. The types are values (Text, Bool, Money, Count, Time, Duration); a name is given once, and is neither `caller` nor an attribute of the target's kind, which `the …` would also name. Every `admit` of the same event declares the same data, so an event means one thing whoever sends it. Only an event carries data: a new entity or a change is given its attributes.
2. **`the amount` is the event's value.** It is read in the admit's condition (`where the amount is at most its total`) and in the event's reactions (`its refunded becomes the amount`). Nothing follows it: it is a value, not an entity.
3. **The caller in reactions.** An event's reaction reads `the caller` and its attributes, as an admit's condition does (§G.2.3): the caller's facts are carried to the step and read there only; they are not kept on the target. A reaction cannot change the caller: it changes only its own unit. As in admits, the caller is readable only when the reaction's roles have one kind.
4. **The gate checks the data.** An event that leaves out a value, adds one the admit does not name, or gives one of the wrong type is refused at the gate, before its condition is read, and the gate line says which value.
5. **The caller in rules, by design never.** A rule states what must hold of the state at every moment; the caller exists only while an input is applied. A condition about who may act belongs on the `admit`, and a fact about who acted is kept by a reaction (`its by becomes the caller's name`) where a rule can read it.
6. **The log.** An event's line carries its data (`"data": [["amount", …]]`), left out when there is none, so older logs replay unchanged.

### Q.3 Acceptance

- `customer Ada cancels O1 with reason "too late"` sets the order's reason and who cancelled it.
- A refund above the order's total is refused at the gate; one that breaks a rule is refused by the rule; one with a missing, extra or mistyped value is refused at the gate.
- Mismatched data across admits, data named after an attribute, a reference as data, and `the caller` with a kindless role are compile errors.

**Q.2.7 Review updates.** A refusal is read only from the step's refusal note, so an event's Text data that reads like a refusal (`"x: refused by rule cap"`) is traced, quoted, and does not make an applied event look refused.

## Appendix R. Prototype slice 17: goals about parts, and events to parts (2026-10-19)

Until now a goal's subject and an event's target had to be a root kind. A goal like "every Shipment is delivered within two days" had to go through a definition on the owner, and an event on a part through a reaction on the root. Slice 17 lets both name a part.

### R.1 What slice 17 adds

| Added in slice 17 | Still waits |
|---|---|
| `desired within 2 days: every Shipment whose order is express is delivered` | a deadline per part, from that part's creation |
| `admit deliver Shipment by courier`, sent as `courier Cy delivers O1's Shipment` | events to one of several parts of a kind |
| `"part": "Shipment"` in an HTTP event, `part` in the log | |

### R.2 Semantics that slice 17 settles

1. **A goal about a part is about each unit's parts.** Its subject is the current attempt of each part of the kind in a unit whose filter holds there. The goal applies to the unit once some part fits; its deadline runs from that moment, as for a root (§C.3.4); it is met when every part that fits is in all the goal's states. A unit with no part that fits is not a subject. The met, matched and alert marks stay per unit, so a goal alerts at most once per unit.
2. **An event to a part.** An `admit` of an event may name a part kind. The input names the unit and the part kind; the event goes to the unit's one current part of that kind. None, or more than one, is refused at the gate. The admit's condition is read at the part (`it`, `its order`), with the caller and the data as in §G.2.3 and §Q.2.2, and the event's reactions run at the part, as one step of the unit, which a rule may refuse.
3. **New entities and changes stay on roots.** A part is created by its owner's needs, and its attributes are not changed from outside (§G.2.5).
4. **The log.** An event's line carries `"part"` only when it goes to a part, so older logs replay unchanged.

### R.3 Acceptance

- An express order's goal about its Shipment is met when a courier delivers it, and alerts when the deadline passes first; it does not apply to an order that is not express.
- The gate reads the part's state: delivering a delivered Shipment is refused at the gate. An event to the root kind with no admit is refused at the gate; one that breaks a rule is refused by the rule.

**R.2.5 Review updates.** A part counts, for a goal and as an event's target, only when every node on the way to it is a current attempt, so the parts of a replaced owner are left out. An HTTP event's `part` that is not text is a bad request.

## Appendix S. Prototype slice 18: waking only what can be served, and asking models after the step (2026-10-19)

Two costs left in v0 are lifted. A freed amount woke every waiter of its pool key: each one gathered, most were refused, and each refusal was a round of messages. And a choice handled by a model held up the step that needed it for up to the model's `within`, and with it the caller's answer.

### S.1 What slice 18 adds

| Added in slice 18 | Still waits |
|---|---|
| a freed amount, a new entity or a change wakes only the waiters the entity can serve, one at a time | waking by a strict arrival order, where a waiter that does not fit holds back the ones behind it |
| a served program asks its models after the step, as it ticks, and logs each answer as an input (`decided`) | asking models in parallel, in threads of their own |
| a deferred choice whose deadline passes is left to `prefer` | |

### S.2 Semantics that slice 18 settles

1. **Waking.** A change to a shared entity (an amount given back, the entity created, an admitted change) puts it on a queue. When the host is idle, it takes the first entity of the queue and, in arrival order, the first waiter of that entity's pool key that the entity would grant its take now: the take checked against the entity's rules as the entity would check it. That waiter gathers again from scratch, and the host lets everything it causes happen before it looks for the next. A waiter the entity would refuse is not woken and sends nothing. Each waiter is woken at most once per change, so a pick refused for another reason does not loop. When no waiter can be served, the next entity of the queue is taken. A smaller waiter can therefore go ahead of a larger one that does not fit.
2. **Models after the step.** With `defer_models`, which a served program has and a scenario does not, a choice that needs a model (§J, §K) is not put to it in the step. The binding or the slot stays asked; the trace says `choice for ⟨key⟩: model "m" is asked`; and a timer is set for the model's `within`, in whole seconds, at least one. The service asks the models of its own units (§O.2.5) as it ticks, before it fires timers, and applies each answer as an input logged as `{"decided": {"key", "answer"}}`, without moving the clock first. On the answer, the options are gathered again: a pick among them is used; one that is no longer an option, a decline, or any failure leaves the choice to `prefer`, as in §K.2.5; with no option left, a binding waits. If the deadline fires first, `prefer` picks, and a later answer is ignored.
3. **Replay.** A `decided` line replays its answer, so no model is asked again. An input followed by `model` lines, from a log written before this slice, asked its models in its step, and replays so.

### S.3 Acceptance

- Two units come back to a Stock with two orders waiting, one for 2 and one for 1: the first gets them, and the second is not woken, asks nothing and is refused nothing.
- A served input whose choice needs a model is answered before the model is asked; the model's pick arrives at the next tick, is logged as `decided`, and replays without the model.
- A deferred choice whose deadline passes is picked by `prefer`, and the model's later answer is ignored.

## Appendix T. Prototype slice 19: the deferred minors (2026-10-19)

Four small gaps found along the way, each of which let a program or an input go wrong without saying so.

### T.1 What slice 19 adds

| Added in slice 19 | Still waits |
|---|---|
| a duration is at most 100 years, and a larger one, or one that overflows, is a compile error | |
| an input that would make a unit larger than the payload limit is refused before anything changes | a step that grows a unit past the limit on an answer or a timer faults (§L) |
| a second `prefer` for the same choice, and two kinds that share a plural, are compile errors | |
| `attempts 0` is a compile error, like `candidates 0` | |

### T.2 Semantics that slice 19 settles

1. **Durations.** Every duration a program or a scenario names, in any unit, is at most 100 years (3 153 600 000 seconds). Larger ones, and ones whose seconds overflow, are compile errors that name the duration. The runtime adds durations to its clock without wrapping.
2. **Unit size.** A unit is one payload, so it must fit the machine's payload limit (§A, 4096 bytes). A creation, an admitted change and an admitted event are checked as the unit would be after their step: one that would not fit is refused, `refused: ⟨label⟩ would hold ⟨n⟩ bytes, and a unit holds at most 4096`, and nothing is created or changed. A change is checked in every unit that carries the changed facts too, and an event's message, with its data, must fit the limit as well. The unit counts its carried facts and the notes of its last step.
3. **Checked once.** `attempts` must be at least 1. A choice (a slot, or a kind picked with `uses`, per `for a ⟨element⟩`) has at most one `prefer`; a second is an error naming the first's line. A kind's plural may be neither another kind's plural nor another kind's name, so `all ⟨Plural⟩` and the like name one kind; a kind may be its own plural (`kind Sheep (plural Sheep)`).

### T.3 Acceptance

- `within 5300 weeks` and `within 99999999999999999 weeks` are compile errors; `5200 weeks` compiles.
- A Note created with a body of 5000 characters is refused, and nothing is made; changing a Note's body to one is refused, and the Note is unchanged. Neither leaves a fault.
- Two `prefer Carrier:` lines, `kind Crate (plural Boxes)` beside `kind Box`, and `attempts 0` are each a compile error.

## Appendix U. Prototype slice 20: a container image for the pods (2026-10-19)

Slice 14 made pods (`suture serve --pod`), but nothing packaged them, and a stopped process left its pod's beat behind for 10 seconds.

### U.1 What slice 20 adds

| Added in slice 20 | Still waits |
|---|---|
| a `Dockerfile`: the `suture` command, `curl` and `tini` on Debian, run as a non-root user, serving `/app` with its state in `/data` | images published to a registry, and images for other platforms |
| `suture serve` stops on `SIGTERM` and `SIGINT`, and leaves the pods | draining: requests that arrive while it stops are refused by the closed port |
| `deploy/k8s/tickets.yaml`: a StatefulSet of two pods over one `ReadWriteMany` volume | a Helm chart, an operator, and rolling deploys driven by the pods themselves |

### U.2 Semantics that slice 20 settles

1. **The image.** It is built in two stages: the command with `cargo build --release --locked`, then a Debian slim image with `ca-certificates`, `curl` and `tini`. The entry point is `tini -- suture`; the default command is `serve /app --state /data --listen 0.0.0.0:7878`; the user is `10001`; `/data` is a volume; the health check is `GET /health` on port 7878, with 5 minutes to start, since a pod replays the log before it listens. A program's image is built `FROM` it with the program in `/app`. Keys are never in an image: they come from environment variables, as before.
2. **Stopping.** On `SIGTERM` or `SIGINT`, the service finishes the request it is on, fires no timer and posts nothing more, stops, removes its file in `pods/`, so its units go to the other pods at once (§O), prints `stopped; pod ⟨name⟩ left`, and exits with status 0. The signal handler only sets a flag that the serving loop reads, so a stop never cuts a log write short. A service that ends on an error after it joined leaves the pods too.
3. **Pod names.** On Kubernetes a pod is named after its Kubernetes pod, through the downward API, which gives the lower-case names that §O requires.

### U.3 Acceptance

- `suture serve` sent `SIGTERM` exits with status 0, prints `stopped; pod a left`, and leaves no `pods/a`.
- Two containers of the image over one volume are one service: an input to one is read from the other, both are healthy, and `docker stop` on one hands its units to the other at once.
- The Kubernetes manifest passes `kubectl apply --dry-run=client`.

## Appendix V. Prototype slice 21: media for decision models (2026-10-19)

Decision models chose from text and numbers only. This slice lets them see images, sounds, videos and documents. They still only choose among the options (§J, the model class): they never generate, and what a program means never depends on media.

### V.1 What slice 21 adds

| Added in slice 21 | Still waits |
|---|---|
| the type `Media`, held in a unit as a reference: SHA-256, media type, size | media in sentences, formulas, keys or `sends` (by design: only models see media) |
| a media store, in memory or in `⟨state⟩/media`, shared by the pods | removing media from the store |
| `POST /media`, then the reference in an input; `file "…"` in scenarios | streaming uploads; a body over 8 MiB |
| media inlined in base64 for Jev and custom backends, or sent as a reference with `"media": "omit"`; `max_media_bytes` | scores or yes/no questions about media, beside picks |

### V.2 Semantics that slice 21 settles

1. **The type.** `Media` is an attribute type, and an event's data may have it. No path that ends at a Media value may appear in a sentence, a formula, a pool key, a `prefer` or a `sends`; that is a compile error. It may be marked `personal`, not `secret`. It has no literal in a program and no default.
2. **The store.** A Media value is `{sha256, mime, bytes}`. The content is kept once, by its hash: in memory for scenarios and `Service::in_memory`, in `⟨state⟩/media/⟨sha256⟩` for a served program, written whole and renamed, so every pod reads it. Reading checks the hash, so content that changed on disk is not given out. A served input that names media the store does not hold, or holds with another type or size, is refused before it is logged. The content is the identity: the same content sent again, under any type, answers the reference it was first kept with, and every writer renames from a temporary file of its own. `POST /media` takes up to 8 MiB with a `Content-Type` and a `Suture-Caller`, and answers the reference. Replay never reads the store: units hold references, and model answers are journaled.
3. **Models.** Before a model is asked, the host attaches the content of every Media value among the chooser's facts and the options' attributes (`Choice::media`), as far as the store has it. Marked facts are left out as before (§N), so a personal photo is never shown. A backend in `inline` mode sends each as `{"media", "bytes", "sha256", "base64"}`; in `omit` mode without `base64`. If an inline backend is missing content, or the content is over `max_media_bytes` (6 MiB, by default and at most, so the request in base64 stays within the 10 MB line curl reads), it fails without posting, and `prefer` picks, with the reason in the trace.
4. **Scenarios.** `file "⟨name⟩"` is a literal in scenarios only. It is read relative to the scenario file's directory, and its type comes from the extension: `.png`, `.jpg`/`.jpeg`, `.gif`, `.webp`, `.mp3`, `.wav`, `.ogg`, `.mp4`, `.webm`, `.pdf`, `.txt`. A missing file or an unknown extension fails the step.

### V.3 Acceptance

- In a scenario, a Claim with `photo file "wreck.png"` (content saying "total") is written off by a model that reads the photo, and one with `file "dent.png"` repaired; the trace shows only the reference.
- `rule …: no Claim whose photo is "x" …`, `sends the claim's photo` and `where the scan is "x"` on Media data are compile errors.
- A personal photo is not given to the model, which declines; `prefer` picks.
- Served: `POST /media` without a `Content-Type` is a 400; with one, the content is kept by its hash; an input naming unknown media is refused with 409 and not logged; the model picks at the next tick; the log holds the hash and not the content; a restart without model or media replays to the same unit.
- The Jev request carries the photo with its base64; `omit` leaves the base64 out; missing content or content over `max_media_bytes` fails without posting.

## Appendix W. Prototype slice 22: what the claims evaluation asked for (2026-10-19)

The claims comparison (eval/claims/RESULTS.md) found one bug and three gaps. Change 2, a third outcome, cost Suture more than plain Rust, because every rule named one kind. This slice fixes those four things.

### W.1 What slice 22 adds

| Added in slice 22 | Still waits |
|---|---|
| a fetch nobody waits on stops | the explanation of a waiting step naming the fetch it waits for |
| `has a ⟨Slot⟩ [that is ⟨state⟩]`, and a last `that` shared by `has a A or a B` | a slot through a path (`its Outcome's …`) |
| `taking 1`, named after the taking kind's plural | taking a constant amount (`taking 5 EUR`) |
| read-backs: when each source is asked, and how each slot moves on | |

### W.2 Semantics that slice 22 settles

1. **Stopping a fetch.** When a fetch's deadline passes, the runtime first checks whether any node that has not failed still needs the value: a rule that reads the source holds the node back, or the node is idle and waits on such a rule. If none does, the fetch ends with no more sends and no `otherwise`, and the trace says `⟨attr⟩: no step waits on it any more: stopped asking ⟨adapter⟩`. A later step that needs the value fetches it again as usual.
2. **Rules over a slot.** `⟨np⟩ has a ⟨Slot⟩ [that is ⟨state⟩]` compiles to `has a ⟨A⟩ … or has a ⟨B⟩ …` over the slot's alternatives, and `has no ⟨Slot⟩` to `has no ⟨A⟩ and has no ⟨B⟩`. The state is checked against each alternative, and one alternative without it is a compile error. Strictness, sources and held steps follow from the expanded condition. In `has a A or a B that is S`, the last `that` applies to each kind, unless an earlier kind has its own `that`. The paraphrase reads `has an outcome (a repair, a writeoff or a cash) that is sent`.
3. **Taking a count.** `taking ⟨count literal⟩` is allowed. The running total is named after the taking kind's plural in lower case (`repairs`), and read as `the total repairs taken from each Garage`. Any other literal is an error that suggests `taking 1`.
4. **Read-backs.**
   - A `sourcing:` line ends with `asked only when ⟨a K⟩ would be ⟨state⟩`, grouped by state, from the `no … has a K that is S` of the rules that read it. It says `asked only when a step they hold back would go ahead` when no such step can be named.
   - Each slot in `slots:` is listed with or without decision models. It also states the policy the runtime applies: it moves on to the first alternative not yet tried, in `prefer` order, after the last of `attempts` failures or a timeout, after each alternative's patience, or when a rule on facts that never change refuses one; and one that went out is never given up for time alone.

### W.3 Acceptance

- A cancelled order whose credit fetch times out does not ask the bureau again.
- `no O has a D that is sent` over a slot `D is A or B` compiles to an `or` over A and B, and its paraphrase names both. A state that only A has is an error.
- `no O has an A or a B that is sent` applies `sent` to both, and `an A that is done or a B that is sent` keeps each state.
- `taking 1` names the total `repairs`, and `taking 2 EUR` is an error.
- `suture build` on examples/credit says `asked only when a Payment would be sent`. A slot's read-back states its attempts.
- The claims workflow uses `taking 1` and one fraud rule over `Outcome`, and its thirteen scenarios pass.

## Appendix X. Prototype slice 23: a waiting step names its fetch (2026-10-19)

The last open item of the claims evaluation's list (eval/claims/RESULTS.md, item 4): `suture explain` said that a step waits on a rule, but not that the rule waits on a fetch.

### X.1 What slice 23 adds

| Added in slice 23 | Still waits |
|---|---|
| `waiting: rule ⟨r⟩, on ⟨Kind⟩'s ⟨attr⟩ (⟨where the fetch stands⟩)` in `suture explain` and the served explanation, which share one function | nothing more from the evaluation's list but a design partner's workflow |

### X.2 Semantics that slice 23 settles

1. **What is named.** For a part waiting on rule ⟨r⟩, each sourced fact that ⟨r⟩ reads (`sources`, defines included) is looked up on every live node of the source's kind, as the runtime does when it fetches. A fact is named when a model is picking its source, when a fetch is open (`asking ⟨adapter⟩, send n of attempts`), when its sources failed (`⟨adapter⟩ failed`), when it has no value yet (`not fetched yet`), or when its value is too old for the rule at the time of the explanation (`2 hours old, too old for the rule`). A fact that is in and fresh enough is not named, because then the rule waits on its value. The same line is not repeated.

### X.3 Acceptance

- In the credit program, an order just placed explains `Payment: waiting: rule credit, on Order's credit (asking bureau, send 1 of 3)`, and a credit two hours old against `at most 1 hour old` explains `(2 hours old, too old for the rule)`.

## Appendix Y. Prototype slice 24: a fixed refusal moves on, or is warned about (2026-10-20)

The returns comparison (eval/returns/RESULTS.md) found a trap. `rule window: no Item has a Refund that is sent unless its Refund's created - its unit's created is at most 90 days` reads only facts that never change, but a late refund waited for ever instead of moving on to store credit, and nothing said so.

### Y.1 What slice 24 adds

| Added in slice 24 | Still waits |
|---|---|
| a slot moves on at once when a rule on fixed facts refuses its alternative's send, or its being done, not only its creation | a state form for kinds; relations across units with a filter |
| the refused part's own unchangeable attributes, `created` included, count as fixed | |
| a formula that reads a changeable fact, a part, a slot, a binding or a total is not fixed | |
| `suture build` warns about each part that a fixed refusal leaves waiting for good | |

### Y.2 Semantics that slice 24 settles

1. **Moving on.** When sending a part's request, or marking a part with no effect done, newly breaks a rule whose `fixed` is set, and the part is its slot's current alternative, nothing of it has gone out, and the slot has an alternative not yet tried, the part is withdrawn (its grants given back) and the slot moves on as in F.2.15. The trace says `⟨Slot⟩: ⟨Kind⟩ refused by rule ⟨r⟩, trying ⟨Next⟩`, or asks the slot's model.
2. **What is fixed.** A path is fixed when its last step is an attribute that is not changeable and it goes through carried facts, stays in its unit's node, or goes to a part that the rule refuses. Such a part exists whenever the rule breaks, unless an `or` in the `no … has` has a branch that is no `has`. So `its Refund's …` is fixed only when every `has a K` in the rule's `no … has` names a Refund there, and no such `or` is in it, and `its Payback's …` only when each names an alternative of that slot. A bool state on such a part is fixed in the same way. An attribute is changeable (G.2.6) when a reaction, an admitted change, an effect's answer or a source sets it. A formula is set once it can be known, so a formula that reads a changeable attribute, a part, a slot, a binding, a candidate, an element, the caller, an event's data or a total counts as changeable too, transitively.
3. **The warning.** For each fixed rule and each kind K it refuses that is no slot's alternative, or is its slot's last alternative, `suture build` warns: `rule ⟨r⟩: a K it refuses waits for good, because the rule reads only facts that never change[, and K is the last alternative of ⟨Slot⟩]. If none is wanted then, say so on the need: `⟨Owner⟩ needs ⟨K or Slot⟩ while …``. A kind no need asks for is not named.

### Y.3 Acceptance

- `no Order has a Shipment that is sent` on a restricted country moves the delivery to a Refund at once, and the Shipment never waits.
- A rule through `its Refund's amount` or `its Refund's created`, refusing a Refund, is fixed; one refusing a Shipment through `its Refund's amount` is not.
- The returns port drops its `after 1 minute` patience and its three `1 minute passes`, and states its restock condition on the need. Its eighteen scenarios pass, and `suture build` warns only about `rule kept` (a preferred refund past 90 days is not paid, as in Solidus).
- The claims port warns about `rule limit` (a payout over the limit is never paid), and a write-off over the limit now tries a repair instead of waiting.

## Appendix Z. Prototype slice 25: state attributes (2026-10-20)

The returns comparison's change 2, a new reception state, cost Suture +4 −2 against Solidus's +2 −1. Each reception state was a Bool, and nothing said that they exclude each other.

### Z.1 What slice 25 adds

| Added in slice 25 | Still waits |
|---|---|
| `⟨attr⟩: ⟨state⟩ \| ⟨state⟩… [= ⟨state⟩]` in a kind | transitions declared on the attribute (`from awaiting`); admits still guard them |
| `it is ⟨state⟩` and `has a K that is ⟨state⟩` for those states, also through references | an admit and a reaction in one statement |
| `⟨np⟩ is ⟨state⟩` as a reaction's action | |
| values that are not states are refused at every input | |

### Z.2 Semantics that slice 25 settles

1. **Declaring.** A state attribute lists two or more lower-case states separated by `|`. It is a `Text` that holds one of them: the first, or the one after `=`. A state must be new on its kind. It cannot be an attribute, a state of another of the kind's state attributes, an effect's mark or outcome, or `done` or `failed`. The default must be one of the states.
2. **Reading.** A state is read like a Bool attribute's state: `⟨np⟩ is [not] ⟨state⟩` compiles to `Cond::State`, and the runtime holds it when the attribute's value is that state. `its reception is received` compiles to a comparison with the same meaning, and a text that is not a state is an error there. Through a reference, a shared entity's states are its Bool attributes and the states of its state attributes. Wherever the compiler matches names (G.2.6, I.2.4, F.2.15), a state counts as a read of every state attribute that holds it, so the stale-copy error, watches, strictness and fixed rules apply to state attributes as they do to Bools.
3. **Setting.** In a reaction, `⟨np⟩ is ⟨state⟩` sets the attribute to that state. `is not ⟨state⟩` is an error, because it names no state to go to. `⟨attr⟩ becomes …` and an admitted change may also set it.
4. **Inputs.** A given value or an admitted change that is not one of the states is refused: `Copy.condition is one of shelved, lent, lost, mending, not burnt`. So is an answer's field that would set one. A reaction's `becomes` with a literal that is no state is a compile error, and one whose value is computed leaves the attribute unchanged and notes why. An answer field cannot be named after a state, and a deploy that drops a state some live unit holds is reported as a break.

### Z.3 Acceptance

- A kind with `condition: shelved | lent | lost | mending` starts shelved. Reactions move it from state to state, and only one state holds at a time.
- A wrong default, a repeated state, `done`, a clash with an attribute or another state attribute, `is not ⟨state⟩` in a reaction, and comparing with a text that is no state are errors.
- A rule reading a shared entity's state that its reactions change is the stale-copy error. One that an admitted change sets is watched.
- The returns port uses `reception: awaiting | received | given | lost | wrong | short | expired`. Change 2 is replayed as `| transit`, an admit and a reaction: +3 −1, against +4 −2 before.

## Appendix AA. Prototype slice 26: filtered relations across units, and sibling totals (2026-10-20)

The returns comparison left three Solidus behaviours unported. Each one reads the other return items of the same inventory unit: a new return item cancels the older ones, a second completed return item is refused, and a unit with a reimbursed return item makes the others ineligible. In v0, each return item is its own unit, and a relation across units had no `that` filter and could be read only by its own unit.

### AA.1 What slice 26 adds

| Added in slice 26 | Still waits |
|---|---|
| `that ⟨cond⟩` on a relation across units: `the number of each Unit's Items that are back` | a rule that reads another unit's total |
| a unit reads the totals of a unit it refers to: `the number of its unit's Items [that …]`, `the total ⟨attr⟩ of its unit's Items` | a total per period read from another unit |
| an event or change that would overfill a filtered total is refused by the total's rule | refusing a new entity, or an answer, that would overfill one: its contribution waits |
| a formula keeps another unit's total as it was at creation | ordering within a relation (`the newest Item`) |

### AA.2 Semantics that slice 26 settles

1. **Filtered totals.** `the number of each ⟨Kind⟩'s ⟨Kinds⟩ that ⟨cond⟩` and `the total ⟨attr⟩ of … that ⟨cond⟩` compile to the implicit binding of F.2.11, with the filter as its `held` condition. The filter is read at each entity of the relation, in its own unit, with no caller or event data. The entity adds its amount (or 1) while the filter holds: when the filter comes to hold, at any status, the binding asks to take, and when it stops holding, a granted take is given back (F.2.10) and a pending one is dropped. The total is named with its filter, `number of Items that are back`, and a binding id carries it: `Unit(number of Items that are back)`. While its filter does not hold, the binding does not keep its parts waiting.
2. **Refusing.** A filtered total is per-group, and its owner refuses a take that would break one of its rules, as in F.2.8. When an admitted event or change would start such a take, the host asks the owner first, and a refusal refuses the input with that rule: `I2 refused staff Sam receives it: U1 would break rule once with its total number of Items that are back: …`. A new entity's contribution, and one an answer or a timer starts, waits for room, as an unfiltered one does.
3. **Sibling totals.** From a kind K that refers to a shared kind S through a path ending in a reference, `the number of ⟨path⟩'s ⟨Rs⟩ [that …]` and `the total ⟨attr⟩ of ⟨path⟩'s ⟨Rs⟩ [that …]` read S's total, where R refers to S through exactly one attribute. They compile to `Num::Taken` with the path and S as its `kind`, and the binding is marked `shared`. A unit's fact, as other units carry it, holds its shared totals. Facts with none encode as before. When a take or a give-back changes a shared total, the host refreshes every unit that carries the owner's fact, in the order they started carrying it. A formula that reads a sibling total keeps its value at creation, so `seq: Count = the number of its unit's Items` numbers the items of a unit in the order they were made.
4. **Rules.** A rule that reads a sibling total, directly or through definitions, is an error: `rule r: it reads `number of Items`, a total of each Unit that other units change without this rule being checked. …`. A refresh is never refused, so such a rule could break unseen. Needs, admits, goals, formulas and the definitions they use may read one.
5. **Cost.** A take or a give-back on a shared total sends one refresh to each unit that carries the owner, that is, to the owner's siblings. A unit with many referring units pays for each change. `suture build` names each shared total in `choices:` (`…, and each Item reads it`).

### AA.3 Acceptance

- A newer Item supersedes the older ones through `seq` and `the number of its unit's Items`, and an older one can no longer be received.
- `rule once: the number of each Unit's Items that are back is at most 1` refuses a second Item's `receive` by rule `once`, and leaves the Items of other Units alone.
- A paid Item makes its siblings unclean. Unpaying it gives the take back, the siblings are refreshed, and they are clean again.
- A part counts toward a filtered total once its effect's outcome holds, and an answer that would overfill the total leaves the contribution waiting.
- The returns port expresses the three Solidus behaviours it skipped.

## Appendix AB. Prototype slice 27: shares of an amount, negative amounts and dates (2026-10-20)

The returns port carried each unit's `discount` as given, because Solidus computes it as a share of the order's adjustments, `adjustments × price ÷ item total`, and v0 could not multiply an amount by an amount. Promotions are negative amounts, which v0 could not write, and scenarios could not say what day it is.

### AB.1 What slice 27 adds

| Added in slice 27 | Still waits |
|---|---|
| `a * b / c` over amounts is an amount: a share of `a` | multiplying two amounts on their own, or by a ratio such as `b / c` |
| negative amounts: `-3 EUR`, `-0.50 EUR` | negative counts and durations |
| dates in scenarios: `2026-03-01`, `2026-03-01 14:30`, in UTC | dates in a program, and conditions that read the clock |
| `when it is 2026-03-01` in scenarios | other time zones |

### AB.2 Semantics that slice 27 settles

1. **Shares.** `a * b / c` where `a`, `b` and `c` are all amounts is an amount: `a × b ÷ c`, computed exactly and rounded to the cent, half away from zero, as Solidus's `BigDecimal` rounding does when it stores the result. The expression must be written in that order, so `price * order's promo / order's items` is a share and `price * (order's promo / order's items)` is an error. A share of nothing is nothing: when `a × b` is zero, the share is 0, even when `c` is zero, as Solidus's calculator returns 0 for an order with no items. Otherwise, when `c` is zero, the share is unknown, so a formula holding it has no value. Multiplying two amounts otherwise is an error that names the form. Totals over elements (`the total of … over …`) accept the same form.
2. **Negative amounts.** A `-` or `−` directly before an amount makes it negative, in source and in scenarios: `promo: Money = -2.50 EUR`, `with promo -10 EUR`. Only amounts may be negative. `-3` alone is an error. Read-backs show `-2.50 EUR`. A `-` between two values is still a subtraction.
3. **Dates.** In a scenario's given values and `expect` conditions, `YYYY-MM-DD`, optionally followed by `H:MM` or `HH:MM`, is a `Time`. The scenario clock's 0 is 2026-01-01 00:00 UTC (F.2.12), so a date is the seconds from then, and dates before it are negative. A date in a program is an error, `a date is written only in scenarios`: a served program's clock counts from when its log started (§ serving), so a date there would not mean the day it names. A day that does not exist, an hour above 23 or a minute above 59 is an error. Four digits, a `-`, two digits, a `-` and two digits always read as a date, never as a subtraction of counts. Read-backs show `2026-03-01`, with the time only when it is not midnight.
4. **The scenario clock.** `when it is ⟨date⟩` (or `given` or `and`) moves the clock forward to that time, firing timers on the way as `when … pass` does. The trace shows `… it is 2026-03-01`. A date before the clock's present fails the step: `it is already past 2026-02-01: the clock only moves forward`.

### AB.3 Acceptance

- `share: Money = price * order's promo / order's items` gives -3.33 EUR for a 10 EUR unit of a 30 EUR order with a -10 EUR promotion, and -1.67 EUR for a 5 EUR unit.
- A share of a zero item total is unknown, unless the promotion is zero too, and then it is 0; and `a * b`, `a * b / 2` over amounts, `-3`, `2026-02-30` and `24:00` are errors.
- `when it is 2026-02-01 12:30` moves the clock, an entity created after it has that `created`, and going back fails the step. A date in a program is an error.
- The returns port computes the promotion share instead of carrying `discount`.

## Appendix AC. Two fixes found by the leave eval (2026-10-21)

Porting frappe/hrms leave applications (eval/leave) found two bugs.

1. **Verbs ending in a consonant and `y`.** The third person of `apply` is `applies` (and `deny` → `denies`, `carry` → `carries`), and of `pay` is `pays`. Before, it was `applys`, so `employee E applies …` and `when a employee applies an Application` matched no `admit`. The error for a reaction with no `admit` names the verb in its base form, `apply`.
2. **An expectation about a total the program does not keep.** A total across units is kept only when a rule, need, admit, definition or formula reads it. An `expect` that reads one the program does not keep fails its step with `the program keeps no total ⟨name⟩ of each ⟨Kind⟩: no rule, need, admit, definition or formula reads it`. Before, it read 0.

### AC.1 Acceptance

- `admit apply Leave by staff` with `when a staff applies a Leave: …` compiles, and `third_person` gives `applies`, `pays` and `fixes`.
- `expect the number of U's Items that are lost is 0`, where no rule reads that total, fails with that message.

## Appendix AD. Prototype slice 28: spans (2026-10-21)

The leave comparison (eval/leave) could not port hrms's overlap check: no two open or approved leaves of an employee share a day. Six of its tests need it. A relation filter reads only the sibling, never the reader, so `the number of its employee's Leaves that end after its from` cannot be written, and a pairwise comparison would read every sibling. Instead, each leave counts toward each day it spans, and a per-group total per day bounds them, which also gives capacities (a room that seats two bookings a day).

### AD.1 What slice 28 adds

| Added in slice 28 | Still waits |
|---|---|
| `for each ⟨Kind⟩ and ⟨period⟩ from their ⟨attr⟩ to their ⟨attr⟩, …`: an entity counts toward every period it spans | hours, or spans shorter than a day |
| an admitted new entity whose filter holds is refused by a filtered total's rule | consecutive runs across entities (hrms's longest continuous leave) |
| traces name a span by its dates | moving a span when its times change |

### AD.2 Semantics that slice 28 settles

1. **Spans.** After a rule's period, `from their ⟨a⟩ to their ⟨b⟩` (or `its`) names two Time attributes of the relation's kind. The contribution's total is named `⟨total⟩ per ⟨period⟩ from its ⟨a⟩ to its ⟨b⟩`, so it differs from the one counted by `created`. Its read-backs are `count(l in Leave where l.employee = e and l is held, per day from l.from to l.to)` and `the number of its Leaves that are held on any one day they span`. Two attributes that are not Times of that kind are an error: `a Leave spans the periods with two of its times, and `days` is not a Time attribute of it`.
2. **Taking a span.** The entity reads the two times when it asks to take, and names them in its take: `@span day ⟨from⟩ ⟨to⟩`, in seconds. The owner adds the amount to its total of each period the span covers, first to last, both ends included, in UTC (F.2.12), and checks its rules once per period. A rule broken in any period refuses the whole take. Giving it back subtracts it from each. A span that ends before it starts covers no period. A span of more than 3660 days is refused with no rule, so the contribution waits. The times are read once: a later change to them does not move a granted take. The cost is one total per period covered, kept by the owner, as for any total per period.
3. **Admitted new entities.** A new entity made through an `admit` whose contribution to a filtered total holds when it is made is checked with its owner before it is made, as an event is (AA.2.2), and the input is refused by the rule that would break. An unfiltered total, and a new entity made without a gate (`given`, `when a new …`, or by the program), wait for room as before (F.2.9), so a claim over its policy's yearly limit is still made and waits.

### AD.3 Acceptance

- `rule apart: for each Employee and day from their from to their to, the number of their Leaves that are held is at most 1` refuses a second leave that shares a day, by rule `apart`, and allows one that starts the day after, and one for another employee.
- Cancelling a leave gives back every day, and a leave inside those days is then allowed.
- `per week` counts a booking from a Friday to a Tuesday in both weeks.
- An unfiltered span total still makes the new entity, whose seat waits.
- Approving a leave into a day another approved leave holds is refused by the rule.
- The leave port ports hrms's `test_overlap`.

## Appendix AE. Prototype slice 29: required attributes (2026-10-21)

The leave comparison found that an application could be made without its approver, which has no default. The gate refused it only because a condition read the missing approver's user, which was unknown. Until now a left-out attribute was simply unknown.

### AE.1 What slice 29 adds

| Added in slice 29 | Still waits |
|---|---|
| an attribute with no default, formula or source must be given when its entity is made | optional attributes without a default (`approver: Approver or none`) |
| an admitted input that leaves one out is refused at the gate | |

### AE.2 Semantics that slice 29 settles

1. **Required.** An attribute written with no `=`, no source, and not set by an effect's answer (`reports priced with offer: Money`) is required. Every new root entity, and every element of its lists, must be given it, by `given`, by `when a new …`, by an admitted input or over HTTP. An element's reference to its owner (`parcel: Parcel` on an Item of a Parcel) is set for it and is not given. State attributes have their first state as a default, and formulas are computed. The error names every missing attribute in alphabetical order: `a new Leave needs its approver and days: they have no default`.
2. **At the gate.** An input with a caller that leaves a required attribute out is refused at the gate with that error, before its `admit` is read, and the gate's log records it. Without a caller, the creation fails with the same error.
3. **Deploys and logs.** A new attribute without a default, on a kind with live units, is already an error (chapter 18), so a deploy never leaves an entity without a required attribute. A log written before slice 29 may hold a creation that left one out. Replaying it now refuses that creation, and the inputs about it after it, as replay refuses any input the program no longer admits.

### AE.3 Acceptance

- `staff Sam files a new Leave L1` without its approver is refused at the gate: `a new Leave needs its approver: it has no default`, and L1 is not made.
- Defaults, states, formulas and an element's owner are not required, and an element without its own required attribute is refused.
- The leave port drops `its approver's user is not ""`, and hrms's `test_leave_approver_mandatory` still passes.

## Appendix AF. Prototype slice 30: sources a model reads (2026-10-21)

Extraction models such as Cloudflare's Clairic (`@cf/cloudflare/clairic`) read typed values out of text and images in a single pass, with no generation. Each answer has a value, a probability that the value is absent (`p_null`), and a confidence. That makes such a model a source rather than a decision: it gives an attribute its value. Until now a source was always an adapter, so reading a claim's amount off its photo needed custom code outside the program.

### AF.1 What slice 30 adds

| Added in slice 30 | Still waits |
|---|---|
| `source Claim's amount via model "reader@1" within 5 seconds`, with `sends` naming what it reads, media included | one call reading several attributes at once |
| `asking "…"`: the instructions the model gets for the value | estimates and forecasts (p10–p90) as values |
| `at least 0.8 sure`: the confidence below which a value is not used | lists as values |
| scenario steps `model "reader@1" reads 1200 EUR [at 0.4][, then …]`, `… reads nothing`, `… is late` | |
| a `workers-ai` backend for Clairic in `models.json`, and a `value` pointer for the custom backend | |

### AF.2 Semantics that slice 30 settles

1. **A source like any other.** A model source is fetched when a strict rule needs it, is good for its `good for`, falls back on `otherwise`, and competes with other sources under `decide K's attr`, exactly as an adapter source does. The model answers in one of two ways. It *reads* a value, which becomes the attribute's value. Or it is *unread*, which is a failure: the model found nothing, was late, failed, gave a value that is not of the attribute's type, or was less sure than `at least … sure` allows. A source via a model writes no `reports` line.
2. **What the model receives.** It receives the `sends` values, which may include media (a model reads a photo; an adapter cannot), and the attribute's name, type and `asking` text. Secret and personal attributes are not sent to a model in this slice.
3. **When it is asked.** Scenarios and `suture run` ask the model as soon as the fetch is made. A served program asks it after the step, the way it posts to an adapter. It logs the answer as that request's answer (`read` with the value, or `unread`), so replay gives the same value and never asks the model again. The fetch's deadline is the source's `within`.
4. **Confidence is policy.** `at least 0.8 sure` is in the program, so the readback states it and it is the same in every deployment. A model's `min_confidence` in `models.json` still applies on top of it. A value with no confidence given passes the bound only when the source sets none.
5. **Backends.** `"backend": "workers-ai"` posts `{state, images, fields: {attr: {type, instructions}}}` to its `url` with the key from `key_env`. It reads `answers.attr` (or `result.answers.attr`) and takes `p_null` of 0.5 or more as nothing found. Types map as follows: Text to `text`; Count to `number` with `integer`; Money to `number` with `decimals: 2` and the module's currency as `unit`; Time to `datetime`; Duration to `duration` in seconds. Images are sent inline as data URLs, at most four. The published post documents the field types but not the image field's name, so `images` is this slice's reading and may change. A custom backend reads with a `value` JSON pointer and an optional `confidence` pointer. Jev picks; it does not read, and a read asked of it is unread.

### AF.3 Acceptance

- In a claim, `source Claim's estimate via model "reader@1"` reads the estimate off the photo, and the payout rule holds the payout until it is read. The claims eval's change 3 does this, in Suture and in the baseline.
- A read below `at least 0.8 sure`, a `reads nothing` and a late model all fail the fetch, and `otherwise` applies.
- A served program logs the read as an answer, and replay does not ask the model again.
- The workers-ai backend sends the typed field and the image through a fake transport, never the network, and reads `value`, `p_null` and `confidence`.

### AF.4 Found while building it

A formula still cannot read a sourced value. Change 3 first had the amount itself read off the photo, and `payout = amount - deductible` was refused. Allowing a formula over a source with no `good for`, set once the value is fetched, was tried. Then the yearly total over `payout` held every step of the claim with no reason given. That waits for a slice of its own. Slice 31 settles it (appendix AG).

## Appendix AG. Prototype slice 31: formulas over values a source reads once (2026-10-21)

Slice 30 found that a formula could not read a sourced value (AF.4). The claim's amount could not be read off a document, because `payout = amount - deductible` and the yearly total over `payout` follow from it. Allowing the formula alone made the yearly total hold the claim with no reason given. Nothing asked for the amount, and nothing named the wait.

### AG.1 What slice 31 adds

| Added in slice 31 | Still waits |
|---|---|
| a formula that reads an attribute whose sources have no `good for` | a formula over a source with `good for`, kept fresh |
| a total's contribution that fetches the values its amount reads, and waits for them | a `becomes` over a sourced value |
| a value read once as a fixed fact, so a rule over it refuses for good once it is known | an effect that waits for a value it sends but no rule reads |
| `explain` and goal alerts naming the wait: `Claim waits for its amount, to count toward a Policy's total payout` | |
| a fix: a fetch's earlier timers are dropped from the unit | |

### AG.2 Semantics that slice 31 settles

1. **Formulas over a value read once.** A formula may read an attribute whose sources all have no `good for`. It is computed as formulas are (B.3): once every value it reads is known. When a fetch's answer or an `otherwise use` gives the value, the unit's formulas are computed again, and each one that can now be known is set once. A formula over a source with `good for` is an error, because the value would be read again and the formula would fall behind: `Claim.payout reads amount, which its source reads again every 1 day; a formula is computed once, so it reads only a source with no `good for``. So is a rule's `with amount at most … old` when a formula reads `amount`.
2. **Contributions wait for their amount.** A binding whose `taking` amount reads a sourced value through formulas (`Binding.sources`) cannot be asked while the value is unknown. It fetches that value, as a strict rule would (I.2.5): `amount: for a Policy's total payout of Claims per year, asking model "invoices@1" to read it (…)`. Its entity waits. An unfiltered contribution keeps it waiting until granted (F.2.5). A filtered one keeps it waiting while its filter holds and the amount is unknown; while the filter is unknown, it does not keep it waiting (AA.2.1). The fetch is kept going while the contribution waits, as for a rule (W.2.1). `suture explain` and a goal's alert name it: `Claim waits for its amount, to count toward a Policy's total payout of Claims per year`.
3. **Fixed once known.** A sourced attribute that no source keeps for a `good for` and no rule wants fresh is read once, so it is not a changeable attribute (F.2.15), and a formula over it is not one either. A rule over it can be fixed. A fixed rule moves a slot on, or withdraws a part (Y.2.1), only when it is broken on what is known. Nothing unknown counts against it then. While the value is unknown, the strict rule holds the step back, and once the value refuses, the slot moves on. In the claims eval, a write-off over the policy limit still gives way to a repair once the amount is read.
4. **Timers of earlier fetches.** Each fetch wanted its own `:stale` or `:again` timer in the unit, and none was dropped. A value asked again every hour for weeks made the unit outgrow its 4096 bytes and fault, for any source, before this slice too. A fetch's new timer now replaces those of the attribute's earlier fetches. The host has started them already, and a timer for a fetch that is over is ignored (I.2.6), so nothing changes but the size.

### AG.3 Acceptance

- `payout: Money = amount - 100 EUR` over `source Claim's amount via model "reader@1"` is computed once the amount is read. The claim counts toward its yearly total with it, and a second claim with no room waits for room.
- An amount the model does not read keeps the claim waiting for 15 days of hourly asks. The alert and `explain` name the wait, and the unit keeps one timer for the amount.
- A rule over the payout holds a write-off while the amount is unknown, and moves the slot on to a repair once the amount read refuses it.
- The claims eval's change 4 reads the amount off the invoice, in Suture and in the baseline.

### AG.4 Found while building it

- A filtered contribution did not keep its entity waiting while it could not be asked. A claim's payment went out with its payout missing from the request (`pay via bank ()`). `bound` now counts such a contribution as waiting. An effect still sends without a value that is unknown and that no rule or contribution reads. That waits for a later slice.
- In the claims eval, Suture books a repair's garage bay while the amount is still unread, since the bay is the Repair's binding and only the Claim waits. The baseline reads the amount before anything else, so it books nothing. Neither the requirements nor the author chose this.

## Appendix AH. Prototype slice 32: days in a row across entities (2026-10-21)

The leave eval (appendix AC) could not port hrms's consecutive leave. A leave type may last at most so many days in a row, and the leaves just before and after an application count with it, across applications (`get_consecutive_leave_details`). A run is not a total of any one day, so the spans of slice 28 could not say it.

### AH.1 What slice 32 adds

| Added in slice 32 | Still waits |
|---|---|
| `the days in a row of their ⟨Kinds⟩ [that …]`, in `for each ⟨Kind⟩ and day from their ⟨time⟩ to their ⟨time⟩, …` | runs counted in working days, without holidays |
| the read-back `run(count(…, per day from … to …))` | runs over weeks or months |
| | reading a run outside a rule, as a sibling total |

### AH.2 Semantics that slice 32 settles

1. **Days in a row.** In a rule over the days a span covers, `the days in a row of their Leaves that are held` compiles to the same per-day total as `the number of their Leaves that are held` (AD.2.2). The rule reads it differently. For each day the take covers, it reads the number of days in a row, through that day, on which the count is above nothing. The run looks back and ahead from the day until a day with nothing, at most the 3660 days a span may cover. Leaves that meet, one ending the day before the next starts, make one run. A day with nothing ends a run. The run's length is a Count, in calendar days. Anywhere else, the phrase is an error: `` `the days in a row of its Leaves` counts the days a span covers: write it in `for each Employee and day from their ⟨time⟩ to their ⟨time⟩, …` ``.
2. **Refusing and giving back.** A run rule is checked as any rule over a span is: once per day the take covers, by the owner, and an admitted new entity is refused at the gate (AD.2.3). A give-back can only shorten a run, so it is never refused, and a run it splits is two runs.
3. **Cost.** The owner keeps the per-day total it keeps for the span. A check reads the days of the run around each covered day, so it costs the length of the run, as one lookup per day.

### AH.3 Acceptance

- In the leave eval, `rule consecutive: for each Allocation and day from their from to their to, the days in a row of their Applications that are held is at most its type's longest or its type's longest is 0` ports `test_max_consecutive_leaves_across_leave_applications`. Two applications meet around a third, which is refused because the three would make twelve days in a row against ten. A one-day application that leaves a gap is admitted. After one is rejected, an application that joins what is left is admitted.
- The same rule covers `test_max_continuous_leaves`, one application longer than its type allows, so the port's `short` definition is gone.

### AH.4 Found while building it

- hrms counts a run per employee and leave type. The port counts it per allocation, which is per employee and type for a period, so a run across two allocations is not joined. hrms counts the run's leave days less holidays. The port counts calendar days.
- A refusal names the total behind the run, `E would break rule run with its total number of Leaves that are held per day from its from to its to: …`. The paraphrase after it says "days in a row".

## Appendix AI. Prototype slice 33: kept totals (2026-10-22)

A total across units is kept only when the program reads it (AC.4). The leave eval's dashboard totals were kept by two definitions that nothing used. Slice 33 lets a program say what it keeps.

### AI.1 What slice 33 adds

| Added in slice 33 | Still waits |
|---|---|
| `keep the total ⟨attr⟩ of each ⟨Kind⟩'s ⟨Kinds⟩ [that …]`, `keep the number of each …` | kept totals per period |
| `suture explain` and `GET /units/…` show a unit's running totals | a total of `all ⟨Kinds⟩` with no rule |
| a `keep` editor template | |

### AI.2 Semantics that slice 33 settles

1. **Keeping.** `keep ⟨total⟩` compiles the total as a rule's bound over each entity would (F.2.8, AA.2.1), and keeps its implicit binding. It adds no rule and no state. Entities count toward it, are held while its filter holds, and give back as they would for a rule. No bound refuses them. A `keep` of a total that a rule or definition already reads adds nothing.
2. **Readers.** For each running total of a unit's kind outside periods, explain shows a line under the unit: `its total days of Applications that are approved: 4`, or `its number of Applications that are open: 1`. A total nothing has added to shows nothing, as `0`. Totals per period are not shown.
3. **Errors.** A `keep` names a total of each entity, `the total … of each K's Ks` or `the number of each K's Ks`. Anything else is a parse error that shows both forms. A total that is not across units, such as one over a unit's own parts, is an error: `` `keep …`: it is not a total across units, which K keeps as it changes ``. An `expect` about a total that is not kept now ends `…, and no `keep` keeps it`.

### AI.3 Acceptance

- The leave port replaces its two unused definitions with two `keep`s: +2 −2 lines, 16 scenarios passing.

## Appendix AJ. Prototype slice 34: a model picks the model (2026-10-22)

A `decide` names one model for a scope, and a `whose` moves a choice to another model by a fixed condition (J.2). Some routing depends on what only a model can judge: how unusual a photo looks, or how hard a case is. Slice 34 lets a fast System One model, one that only picks, choose which model makes a choice.

### AJ.1 What slice 34 adds

| Added in slice 34 | Still waits |
|---|---|
| `decide … with model "a@1" or model "b@1"[ or prefer] [within …], picked by model "r@1" [within …]` | a router that sees the choice's options |
| the router's pick, journaled under `⟨key⟩?model` | routing among `whose` scopes |
| `model "r@1" picks "b@1"` and `picks prefer` in scenarios | a router's own `at least … sure` (slice 35, AK) |

### AJ.2 Semantics that slice 34 settles

1. **Candidates.** A routed `decide` names two or more candidates, each once: models, and at most one `prefer`. The first is the default. The first `within` bounds each candidate's answer, and the second bounds the router's. Both default to 1 second, and both are from 1 millisecond to 10 minutes. `or` without `picked by` is an error, and so is `picked by` with one candidate. Ranking and ties are as for any `decide` (J.2.3).
2. **The router's choice.** When a routed choice has more than one option, the router is asked first. Its choice has the same point, chooser, facts and media as the choice, with `?model` added to its point and key. Its options are the candidates by name, `prefer` for `prefer`, with no attributes. The router never sees the options of the choice itself.
3. **Then the candidate.** The router's pick is resolved like any model's (K.2.5). Anything other than a candidate's name, including declining, being late or unsure, or failing, gives the first candidate. If the pick is a model, that model is asked as a single `decide` would ask it, and its answer is resolved against the choice's options, best first under `prefer`. If the pick is `prefer`, the first option is picked, `by prefer (as model "r@1" picked)`. The trace shows the router's pick as `model for ⟨key⟩: ⟨candidate⟩ by …`. A routed choice is never worse than an unrouted one, because every pick is still checked by the rules.
4. **Journal and serving.** Both answers are journaled in order, the router's under `⟨key⟩?model`, so replay asks neither. A served program defers the router like any model (S.2.2). When its answer is logged, the picked model is deferred under the choice's own key and asked in the same pass. A unit's request to a model for a slot or a source names the decision it routes, by index, so the host finds the candidates.

### AJ.3 Acceptance

- Bindings and slots route (runtime/tests/routing.rs), and source choices take the same path. The router's pick, `prefer`, an unoffered name and a late router each behave as AJ.2.3 says. A served route logs two `decided` records and replays without a model. A copy of the claims eval with its assessor routed and the router unscripted passes all 16 scenarios.

## Appendix AK. Prototype slice 35: how sure a decision model must be (2026-10-22)

A decision model's minimum confidence lived only in `models.json`, which is per deployment (K.2.7). A source states its own in the program, `at least 0.8 sure` (AF.2.1). Slice 35 gives a `decide` and its router the same bound, so how sure a model must be is part of the program, shown in `suture build` and the same in every deployment.

### AK.1 What slice 35 adds

| Added in slice 35 | Still waits |
|---|---|
| `decide … with model "m@1" [within …] at least 0.8 sure` | a confidence from a function model registered in Rust |
| `…, picked by model "r@1" [within …] at least 0.7 sure` | per-candidate bounds in a route |
| `model "m@1" picks Fast at 0.9` in scenarios | |

### AK.2 Semantics that slice 35 settles

1. **Where it goes.** `at least ⟨0…1⟩ sure` follows a decide's `within`, and the router's after the router's `within`. In a route, the first bound is for every candidate and the second is for the router. It is stored in thousandths, as a source's is. `with prefer` alone takes no bound, the same as `within`.
2. **How it applies.** The bound goes with the choice to the model. A backend holds a pick to the stricter of its `min_confidence` and the program's bound. A pick below it is `unsure` and leaves the choice to the fallback, `prefer` or, for a router, the first candidate (AJ.2.3). Under a program bound, a pick that gives no confidence is not used: `it gave no confidence, and the program needs at least 0.8 sure`. This matches a source, whose bound also refuses a value with no confidence. A function model registered in Rust gives none, so it cannot make a bounded choice.
3. **Scripts and the journal.** A scripted pick takes `at ⟨0…1⟩`, as a scripted read does. The answer is journaled after the bound is applied, as `unsure` or a pick, so replay does not depend on the bound.
4. **Requests.** A unit's request to a model for a slot or a source names its decision, by index, when the decision is routed or bounded (AJ.2.4).

### AK.3 Acceptance

- In runtime/tests/routing.rs, a confident router and candidate decide. An unsure router falls to the first candidate, an unsure candidate falls to `prefer`, and a pick with no confidence is not used. A bounded single model on a slot falls to `prefer`, and `with prefer at least … sure` is an error.
- In runtime/tests/backends.rs, the stricter of `min_confidence` and the program's bound applies, and a function model under a bound is not used.

## Appendix AL. Prototype slice 36: a decision's own fallback (2026-10-22)

When a model was no help, `prefer`'s first option was taken (K.2.5). Slice 36 lets a `decide` name the option instead, the way a source names `otherwise use …`, so a model that isn't sure enough falls back to what the program chose.

### AL.1 What slice 36 adds

| Added in slice 36 | Still waits |
|---|---|
| `decide … with model "m@1" [within …] [at least … sure] otherwise Writeoff` | a fallback for a choice among pooled entities |
| `…, picked by model "r@1" … otherwise model "careful@1"` | a fallback per candidate in a route |

### AL.2 Semantics that slice 36 settles

1. **What it names.** `otherwise ⟨Option⟩` follows the bound. It names one of the choice's options as the program knows them: a slot's alternative or, for a source chosen among adapters, an adapter. A binding's options are entities that exist only as the program runs, so its fallback can only be `prefer`. The decide must name its choice point. A router's `otherwise` names one of its candidates. `otherwise prefer` is the default, and `with prefer` alone takes no `otherwise`.
2. **When it is taken.** For every reason a model is no help (K.2.5): it declines, is late, is unsure under either bound, fails, gives no confidence under a program bound, or picks what it was not offered. The trace says `by otherwise (…why…)`. If the fallback is no longer an option when the choice is made, for instance because a rule refused it, `prefer` picks as before.
3. **Determinism.** The fallback is applied to the journaled answer, as `prefer` is, so replay takes the same option.

### AL.3 Acceptance

- In runtime/tests/routing.rs, an unsure model on a slot falls to its `otherwise`, a sure one is used, and an unsure router falls to its named candidate. An unknown option, `with prefer otherwise …`, a router fallback that is not a candidate, and a fallback on a pooled binding are errors.

## Appendix AM. Prototype slice 37: times and durations as ISO 8601 (2026-10-22)

Programs and scenarios already wrote dates as ISO 8601 (`2026-03-01`). The API did not: it sent and took a time as seconds since the runtime clock's own 0, 2026-01-01 (F.2.14), and a duration as seconds (L.2.7). No other system counts from that day. Slice 37 makes every value that leaves or enters a running program use the standard forms.

### AM.1 What slice 37 adds

| Added in slice 37 | Still waits |
|---|---|
| a time in JSON is RFC 3339 (`"2026-03-01T14:30:00Z"`), on input and output | time zones in program text |
| a duration in JSON is ISO 8601 (`"PT2H30M"`, `"P3D"`) | calendar durations: months and years |
| facts sent to a decision or reading model use the same forms | |

### AM.2 Semantics that slice 37 settles

1. **Times.** A time is read as an ISO date (`2026-03-01`, meaning midnight UTC) or an RFC 3339 date and time, with `T` or a space, minutes or seconds, an optional fraction of a second (dropped), and `Z`, an offset such as `+02:00`, or no offset (UTC). The offset is applied, so `2026-03-01T16:30:00+02:00` and `2026-03-01T14:30:00Z` are the same time. A time is written in UTC with seconds and `Z`. A time given as a number is an error that names the form.
2. **Durations.** A duration is read as ISO 8601: `P`, then weeks and days, then `T` with hours, minutes and seconds, each a whole number, in that order and at most once; a leading `-` makes it negative. `P1M` and `P1Y` are errors, because a month or a year has no fixed length. A duration is written with days, hours, minutes and seconds, leaving out zeros, and `PT0S` for none. A number is an error that names the form.
3. **Models.** A fact that is a time or a duration goes to a model in these forms. A model may read a duration as a number of seconds, as before, or as ISO 8601. The program's semantics, the journal and replay are unchanged: values are still whole seconds on the runtime clock inside.
4. **Program text is unchanged.** `2026-03-01 14:30` and `3 days` stay as they are. The trace and `explain` still show times the short way.

### AM.3 Acceptance

- In runtime/tests/service.rs, times with `Z`, an offset, a fraction or none, and dates, read to the same seconds, and impossible dates and numbers are errors. Durations round-trip through `PT0S`, `P1D`, `PT2H30M`, `P1DT12H` and `-PT45S`, and `P1M`, `P1Y`, `PT`, out-of-order or repeated units and numbers are errors.

## Appendix AN. Prototype slice 38: days between dates (2026-10-22)

A program could subtract two times, giving a Duration, but it could not count days. A Duration cannot be divided by a Duration, and a count of seconds is not a count of calendar days anyway. The leave port therefore took an application's `days` as given, next to its `from` and `to`, and nothing tied them together (eval/leave/RESULTS.md). Slice 38 counts days from two times.

### AN.1 What slice 38 adds

| Added in slice 38 | Still waits |
|---|---|
| `the days from ⟨Time⟩ to ⟨Time⟩`, a Count, anywhere a calculation goes | holiday lists, and counting working days |
| as a formula: `days: Count = the days from its from to its to` | half days, and fractional counts |
| | time zones: days are counted in UTC |

### AN.2 Semantics that slice 38 settles

1. **The form.** `the days from a to b` is a calculation of type Count. Both `a` and `b` must be Times; anything else is an error that names the end that is not. Each end is one value (an atom, E.2.3), so `the days from its from to its to - 1` subtracts 1 from the count; a calculated end is put in brackets, `the days from its from to (its to + 1 day)`, and reads back the same way. In the IR it is `Bin(a, Days, b)`, so it is read and watched like any other calculation.
2. **The count.** It counts calendar dates in UTC, both ends included: the date of `b` minus the date of `a`, plus 1. From a day to the same day is 1, and a time of day does not matter. When `b` is before `a`, the count is 0 or negative; that is not unknown and not refused by itself, so a program states what it allows with a rule (`every Leave's days is more than 0`). Unknown when either time is.
3. **As a formula.** In a kind, `days: Count = the days from its from to its to` is computed once at creation (E.2.5), so a caller no longer gives it, and totals and rules over `days` read the computed count.

### AN.3 Acceptance

- In runtime/tests/days.rs, the count is inclusive across a month, a year and a leap day, and ignores the time of day. A total of computed days is held to a rule, and a `to` before the `from` is refused by a rule. The form reads back as written, with brackets around a calculated end, and an end that is not a Time is an error.
- eval/leave computes `days`, its scenarios no longer give it, and all 16 still pass.

## Appendix AO. Prototype slice 39: a way in for coding agents (2026-10-22)

Most code is now written by coding agents, and an agent has never seen Suture in training. Suture's case for agents is that it is compact (a port is 7 to 12 times shorter than the code it replaces, so a whole system fits in context) and understandable (every rule reads back in plain words, so a person can check an agent's work). Slice 39 makes the language itself fit in context too, gives the agent a short loop, and measures it (eval/agents/RESULTS.md).

### AO.1 What slice 39 adds

| Added in slice 39 | Still waits |
|---|---|
| the agent book, docs/agents/suture.md: the language in about 4.5k tokens, printed by `suture help all` | the book generated from the grammar |
| `suture help [section | all]`, the book built into the binary | |
| `suture check <dir> [--json]`: compile and run every scenario beside the module; traces only for failures | the steps around a failure shown first; a fix with each error |
| `suture init <dir>`: AGENTS.md, CLAUDE.md and a starter module | |
| a Claude Code skill, docs/agents/skill/suture | |
| the agent eval, eval/agents | more tasks and repeated runs |

### AO.2 Semantics that slice 39 settles

1. **One book, read whole.** The book is meant to be loaded once and kept in context: the loop, one module that uses most of the language, every declaration and sentence form, what the runtime does, recipes (a requirement to a form), limits (what cannot be written yet, and what to write instead), scenarios, models, and serving. A test fails if it grows past 20 KB. Its sections are its `## ⟨topic⟩: ⟨what it covers⟩` headings, which `suture help` lists.
2. **The book is tested.** It is compiled into the binary, and its code blocks follow the guide's rules (§7.5): every `suture` block compiles and every `scenario` block passes, checked by runtime/tests/guide.rs. A limit an agent hits in the eval goes into `limits` until a slice removes it.
3. **`check`.** It compiles the module, then parses and runs each `.scenario` file in the directory, sorted by name. The text output has one line per error (`error: file:line: message`), one `✓` line per passing scenario, the whole trace and the failures for each failing one, and a count. `--json` gives `{passed, errors: [{file, line, message}], warnings, scenarios: [{file, title, passed, failures, trace}]}`, with the trace only for a failing scenario. The exit status is 0 when everything compiles and passes, and 1 otherwise. A scenario file that does not parse is an error, and the other files still run.
4. **No MCP server.** An agent with a shell needs none, and tool descriptions cost context on every turn. The commands are the interface.
5. **`init`.** It writes only files that are missing, never through an existing link, and the starter module only if the directory has no `.st` file. The module is named after the directory if that is a lower-case word, else `app`. The skill's text is AGENTS.md's, checked by a test.

### AO.3 Acceptance

- runtime/tests/guide.rs: every example in the book holds. cli/src/help.rs: the book stays under 20 KB and every section is reachable. cli/src/init.rs: `init` keeps existing files, and its module passes `check`; the skill matches AGENTS.md.
- eval/agents/RESULTS.md: on a task the book does not cover, a fresh agent with the book read only the book, and used 43% fewer tokens, 54% fewer tool calls and 72% less time than with the guide, meeting the same requirements.

## Appendix AP. Prototype slice 40: no two alike (2026-10-22)

In the agent eval (appendix AO), every run failed the same requirement: "a reviewer reviews a talk at most once". The only way to write it was to compare each Review with the others, and a filter cannot name the outer entity. Slice 40 adds the sentence that says it.

### AP.1 What slice 40 adds

| Added in slice 40 | Still waits |
|---|---|
| `rule ⟨name⟩: no two ⟨Kinds⟩ [that ⟨cond⟩] have the same ⟨attr⟩[, ⟨attr⟩ …] [and ⟨attr⟩]` | the same value in attributes that change after creation |
| a uniqueness keyed by a reference: one count per referred entity and value | uniqueness inside a list (`no two lines have the same product`) |
| without a reference: `, checked globally`, one count per value at `all ⟨Kinds⟩` | uniqueness that ignores case or spaces in a text |

### AP.2 Semantics that slice 40 settles

1. **What it means.** No two entities of the kind that both hold the filter have equal values in every named attribute. Equal means equal values: the same referred entity, text, number, amount, time or state.
2. **How it is kept.** It is a total across units (appendix F.2.11) with a group. The first named attribute that refers to another kind picks the unit that keeps the count, as in `the number of each Talk's Reviews`. The other attributes' values form the group, and the count is per group. The rule, on that kind, is that no group's count is more than 1. Its cost is per group. When no named attribute is a reference, the count is kept at `all ⟨Kinds⟩`, and the rule must end with `, checked globally` as any rule about `all` must (appendix F.2.13).
3. **When it refuses.** An entity counts when it is created, and while its `that` filter holds, as other totals across units do (appendix AA.2.1). An input that would make two alike is refused by the rule, naming it. An admitted input is refused even with no filter, unlike an unfiltered total (appendix AD.2.3). When the count is the same total as another rule's (`no two Reviews have the same talk` and `the number of each Talk's Reviews is at most 5`), admitted inputs are refused under both. An entity that stops holding the filter gives its count back, so a withdrawn Review no longer blocks a new one.
4. **Attributes that change.** A named attribute must never change after creation: no reaction, effect answer, admitted change or repeated source may set it. A change would move the entity to another group, which the count cannot follow yet, so the checker refuses the rule and says so.
5. **The read-back** is the sentence: `no two Reviews have the same talk and reviewer`, with its cost.

### AP.3 Acceptance

- runtime/tests/unique.rs:
  - a second Review of a talk by the same reviewer is refused by the rule, while one by another reviewer, or of another talk, is not;
  - a Review that stops holding the filter frees the pair;
  - a global uniqueness on a text refuses a second Member with the same email.
- lang tests: the checker refuses an unknown or changing attribute, a kind given in the singular, and a uniqueness without a reference that is not `checked globally`.
- The book's limits no longer list pair uniqueness. Its recipes and the guide's rules chapter show the sentence, tested.

## Appendix AQ. Prototype slice 41: when a time passes (2026-10-22)

The agent eval's task A (appendix AO) needed a loan that becomes overdue on its own. The only way to write it was a reminder part, with an effect that nobody answers, `attempts 1`, and a reaction to its timing out. Slice 41 says it directly.

### AQ.1 What slice 41 adds

| Added in slice 41 | Still waits |
|---|---|
| `when a ⟨Kind⟩'s ⟨time⟩ passes: ⟨action⟩[ and ⟨action⟩]` | a time on a part (`when a Line's due passes`) |
| an admitted change to the time moves when it passes | a time set by a reaction, an answer or a source |
| scenarios: `when 15 days pass` reaches it | `before`/`after` offsets (`when 2 days before its due`), written as a formula for now |

### AQ.2 Semantics that slice 41 settles

1. **What passes.** The time is a `Time` attribute of a kind that is no part: given at creation, or a formula such as `due: Time = created + 14 days`. It passes when the clock reaches it. The reaction's actions then apply to the entity, and the step settles. A passing is a fact, as a time-out is (appendix C.3.3): it is always recorded, and a reaction whose actions would break a rule is held back (`reaction held: rule …`), whether it passes on a timer, at creation or on a change.
2. **A time already past** when the entity is created passes in the step that creates it.
3. **It passes once per value.** Only an admitted change (`admit change Loan's due by librarian`) may set the attribute after creation. A change to a later time moves the passing there, and one to a time already past passes at once. A change to the value it already holds does nothing. A passing that was scheduled for an old value does nothing. The checker refuses a passing on an attribute that a reaction, an effect's answer or a source sets.
4. **How it is kept.** The unit asks the host for a timer `⟨label⟩!passes:⟨attr⟩@⟨time⟩`, as sources and slots do (appendix I.2.5). The firing is a journaled step, so a replay passes at the same times.
5. **What it does not do.** It has no condition of its own: guard what it sets with a definition, such as `define: a Loan is late when it is overdue and it is not returned`.

### AQ.3 Acceptance

- runtime/tests/passes.rs:
  - a Loan becomes overdue when its due passes, and not before;
  - a due already past is overdue at creation;
  - an admitted change before the due moves it, and the old time does nothing;
  - a replay of the log passes at the same times.
- lang tests: the checker refuses a passing on an unknown or non-Time attribute, on a part's kind, and on an attribute a reaction sets.
- The book's recipe "becomes overdue by itself" uses the sentence, tested.

## Appendix AR. Prototype slice 42: the book loads by need (2026-10-22)

Slice 39's book (appendix AO) was read whole, and each new form made it longer. At 19.8 KB it was about to pass its 20 KB guard. Slice 42 splits it. A small core is always read, and it names every form, because an agent cannot use a form it does not know exists. The details load when the module in front of the agent uses them, or when an error points at them.

### AR.1 What slice 42 adds

| Added in slice 42 | Still waits |
|---|---|
| docs/agents/core.md, `suture help`: the loop, a line per form with its topic, recipes, limits | the form list generated from the grammar |
| docs/agents/topics/*.md, `suture help ⟨topic⟩`: ten topics | a topic per form |
| `suture help --for ⟨dir or file⟩`: the topics a module uses | the forms an error expected, not only its statement's topic |
| `suture check` errors end with `(suture help ⟨topic⟩)`, and `--json` gives `topic` | |

### AR.2 Semantics that slice 42 settles

1. **The core is read whole.** It holds the loop, every form on one line with the topic that explains it, the requirement-to-form table and the limits. A test fails past 10 KB, and another fails if a form names a topic that does not exist. A new form costs the core one line.
2. **Topics are read by need.** Each is a file whose first line is `# ⟨topic⟩: ⟨what it covers⟩`: `kinds`, `gate`, `parts`, `rules`, `reactions`, `time`, `scenarios`, `models`, `serve` and `examples`. A topic may grow as its forms do. `suture help all` prints the core and then every topic in order. Every code block in them follows the guide's rules (§7.5) and is checked by runtime/tests/guide.rs.
3. **`--for`.** For a directory, it reads each `.st` file with the tolerant parser (statements that do not parse are skipped) and notes any `.scenario` file. It prints the topics whose forms appear, in book order: `kinds` for a module line or kind; `gate` for roles and admits; `parts` for needs, slots, uses and effects; `rules` for rules, definitions, `keep` and `apply`; `reactions` for reactions; `time` for periods, goals, times that pass and days between dates; `scenarios` for scenario files; `models` for decisions or sources by a model, and `allow`. A path that does not exist is an error. `serve` and `examples` are printed only by name. A file names only its own topics.
4. **Errors name a topic.** The topic is that of the statement on the error's line, found by its first word (a `rule` with `taken from each` is `parts`, one with `for each … and day` is `time`); for a scenario file, it is `scenarios`; an error on no statement, such as a missing `module` line, names `all`.
5. **AGENTS.md and the skill** say: read `suture help` once; before editing a module, `suture help --for .`; and `suture help ⟨topic⟩` for anything else.
6. **Scenario lines,** found by the eval. A scenario's error names the line it is on, not the scenario's title line. An `and` line after an `expect` is read as another expect when it reads as one (`and T1 is not withdrawn`, `and no alert for T1`); otherwise it is an input, as before (`and payments answers paid for T1`).

### AR.3 Acceptance

- cli/src/help.rs:
  - the core stays under 10 KB, and every topic its form list names exists;
  - `--for` gives a module's topics even when a statement does not parse, and a scenario file's;
  - errors map to the topics of their statements.
- runtime/tests/guide.rs: every example in the core and the topics holds.
- eval/agents/RESULTS.md: a fresh agent on task B with the core and topics, against the whole book.

## Appendix AS. Prototype slice 43: many alike in one scenario line (2026-10-22)

The agent eval's task B needed 14 talks to pass a 600-minute limit. The agent wrote them one per line and miscounted once. A scenario can now create a range.

### AS.1 What slice 43 adds

| Added in slice 43 | Still waits |
|---|---|
| `given ⟨n⟩ ⟨Kinds⟩ ⟨A1⟩ to ⟨An⟩ [with …]` | values that vary along the range |
| `when ⟨role⟩ ⟨Name⟩ ⟨verb⟩s ⟨n⟩ new ⟨Kinds⟩ ⟨A1⟩ to ⟨An⟩ [with …]` | a range that is refused |

### AS.2 Semantics that slice 43 settles

1. **A range is n creations.** The parser expands it into one creation per name, in order, each with the same values and list elements. The scenario runs as if they were written one per line; a `given` range never passes the gate, and an admitted one passes it once per entity.
2. **Names.** The first and last names share a prefix, end in numbers, and are exactly n apart (`T1 to T14` for 14). n is at most 1000. Anything else is an error that shows the form.
3. **The kind is in its plural** (`Talks`, `People`), resolved against the program's plurals when the scenario runs.
4. **No refusal.** `is refused …` after a range is an error: the entity that must be refused is written on its own line.
5. An `and` after a range continues it as `given` or `when`, as after a single creation.

### AS.3 Acceptance

- lang/tests/scenario_lines.rs: a range expands into its names, keeps `given`, has its caller, and leaves `and 3 days pass` alone; a miscount is an error.
- runtime/tests/ranges.rs: 13 accepted talks of 45 minutes and two admitted ones fill a program; the next accepted talk is refused by the rule.

## Appendix AT. Prototype slice 44: the trace around a failure (2026-10-22)

A failing scenario's whole trace is long, and in the agent eval the agent filtered it with grep. `suture check` now shows only what explains each failure.

### AT.1 What slice 44 adds

| Added in slice 44 | Still waits |
|---|---|
| `check` prints an excerpt per failing scenario; `--json` adds `excerpt` beside the whole `trace` | an excerpt that follows the cause across units |
| `Report::excerpt` and `Report::failed_at` in the runtime | |

### AT.2 Semantics that slice 44 settles

1. **Around a failure:** the trace lines of the failing step and the six before it.
2. **The reason:** before those, the latest line that refuses, waits or alerts, other than a scenario's own `✓`/`✗` checks. A line naming an entity the failure names (such as `T2`) is preferred.
3. **`…`** marks each left-out stretch. Excerpts of successive failures never repeat a line.
4. The summary line says that `suture run` prints a whole trace. A passing scenario shows no trace, as before.

### AT.3 Acceptance

- runtime/tests/excerpt.rs: a failure after 12 creations keeps the failed expect, its steps and the refusal of the entity it names, in fewer than half the trace's lines.

## Appendix AU. Prototype slice 45: `and` with `or`, grouped (2026-10-22)

Until now, one sentence could not mix `and` with `or`, so a mixed condition needed a `define` even when its parts had no business name. The Solidus port and the core's limits named this. Parentheses now group a condition.

### AU.1 What slice 45 adds

| Added in slice 45 | Still waits |
|---|---|
| `(⟨cond⟩)` wherever a condition joins others: `it is not cancelled and (it is paid or it is free)` | precedence without parentheses (never: readers disagree) |
| read-backs keep the group in both the formula and the paraphrase | |

### AU.2 Semantics that slice 45 settles

1. **A group is one condition.** Inside it, `and` or `or` joins as anywhere else, and a group may hold a group.
2. **Parentheses that open a calculation** (`(a + b) * 2 is at most 9`) stay a calculation: a `(` is read as a group only when what is inside is a whole condition and what follows the `)` is not `is`, `are` or an operator.
3. **Mixing without parentheses is still an error**, which now shows the grouped form.
4. **Read-backs** wrap a group of the other joiner in parentheses: `o is not cancelled and (o is paid or o is free)`.

### AU.3 Acceptance

- runtime/tests/groups.rs: a rule, a definition and a calculation with parentheses compile and hold in a scenario; the paraphrase keeps the group; the ungrouped sentence is an error.

## Appendix AV. Prototype slice 46: one admit for several verbs (2026-10-22)

The returns eval's second round found that a condition shared by several events had to be written once per event. One admit can now name several verbs.

### AV.1 What slice 46 adds

| Added in slice 46 | Still waits |
|---|---|
| `admit return, renew or lose Loan by member[, staff] [with …] [where …]` | one statement for an admit and its reaction |
| `admit place or import a new Order by staff` | |

### AV.2 Semantics that slice 46 settles

1. **One admit per verb.** The parser expands the statement into one admit per verb, with the same kind or kinds, roles, data and condition. Everything that holds for an admit holds for each: each event needs its reaction. The build's `admitted:` list shows the statement once, as written.
2. **Verbs** are lowercase words joined by `,` and `or` before the kind or before `a new`.
3. **`change`** keeps its own admit: naming it among other verbs is an error.

### AV.3 Acceptance

- runtime/tests/verbs.rs: one admit for three events gives four admits, admits two of the events and refuses the third at the gate by its shared condition; `change` among verbs is an error.

## Appendix AW. Prototype slice 47: weekdays (2026-10-22)

The leave eval needed working days. Holiday lists need dates as data across units, and half days a count that is not whole: both wait for a design decision. A count of weekdays needs neither.

### AW.1 What slice 47 adds

| Added in slice 47 | Still waits |
|---|---|
| `the weekdays from ⟨Time⟩ to ⟨Time⟩`: a Count of Mondays to Fridays | holiday lists; half days; another working week |

### AW.2 Semantics that slice 47 settles

1. **The count.** It counts Mondays to Fridays from the first time's UTC date to the second's, both ends counted. It is 0 when the second is before the first; unlike `the days from`, it is never negative.
2. Everything else is as for `the days from` (appendix AN): it is a Count, it can be a formula or appear in a rule, each end is one value, and the read-back says `the weekdays from … to …`.

### AW.3 Acceptance

- runtime/tests/days.rs: a week is 5, Friday to Monday is 2, a weekend is 0 (refused by `every Leave's days is more than 0`), 2026 is 261, and a week before 1970 is 5.

### AW.4 Not in this slice: order within a relation

The returns port's `seq` stands for "the newest Item of its unit". Writing it directly needs a relation filter that compares each sibling with the reader (`… that are created after it`), which is also what holiday lists across units need. That is a larger change than a slice, so it waits for a design decision.

## Appendix AX. Prototype slice 48: `no two` on values that change (2026-10-22)

Slice 40's `no two` (appendix AP.2.4) refused any attribute that could change after creation. Changing an email, or moving a slot to another hour, could not be checked. A count now follows a change made by a caller.

### AX.1 What slice 48 adds

| Added in slice 48 | Still waits |
|---|---|
| `no two` over attributes an admitted change or an event's reaction sets | values an effect's answer, a source or an outcome's reaction sets |
| the count moves to its new group, and a clash is refused | |

### AX.2 Semantics that slice 48 settles

1. **What may change.** An attribute `no two` names may be set by an admitted change or by the reaction to an admitted event. One that an effect's answer, a source read more than once, or a reaction to an outcome, timeout or passing time sets is an error: no input carries that change, so nothing could refuse it.
2. **The count moves.** When a compared value changes, the entity gives back its old take and asks for one in the new group, in the same step. The host checks that take before the input is admitted (as in appendix AP.2.3), so a change that would make two alike is refused by the rule and nothing changes.
3. **References** cannot be changed by a caller (appendix G.2.5), so the count always stays with the same entity.
4. The trace says `⟨Kind⟩ moves its count from ⟨entity⟩`.

### AX.3 Acceptance

- runtime/tests/unique.rs:
  - changing an email to another person's is refused, and changing it to a new one frees the old;
  - changing a slot's hour moves it within the room's group;
  - a value an answer sets is an error;
  - `withdrawn`, which an event sets, may be compared.

## Appendix AY. Prototype slice 49: a router sees the options (2026-10-22)

Slice 34's router (appendix AJ) got the chooser's facts and the candidate models, but not the options the picked model would choose among. Which model fits often depends on those options: a choice among costly carriers deserves the careful model.

### AY.1 What slice 49 adds

| Added in slice 49 | Still waits |
|---|---|
| `Choice.among`: a router's choice carries the decision's options, with their facts | |
| Jev: the instructions end with `The model you choose will choose among: Cheap (fee 5 EUR, zone EU); …` | |
| the custom format: `among`, a list like `options` | |

### AY.2 Semantics that slice 49 settles

1. **What a router sees.** It sees the options the rules allow, in `prefer` order, with the facts a model would get, and their media in the same store. It still picks only a candidate model's name or `prefer`.
2. A choice that is not a router's has an empty `among`, and the custom format sends no `among` field.
3. Nothing in the language changes, and nothing new is journaled: the router's answer is journaled as before (appendix AJ.2.5).

### AY.3 Acceptance

- runtime/tests/backends.rs: Jev's instructions and the custom request carry the options; a plain choice sends no `among`.
- runtime/tests/routing.rs: a router that picks by the options' fees sends a choice with a 9 EUR carrier to `dearest@1`.

### AY.4 Not in this slice: a reaction that sets another entity

A reaction can set only its own unit's entities. Setting a referenced shared entity would be a write from one unit into another, and the runtime is built on each unit owning its state. The way around it (a `define` on that entity, from a total of what refers to it) stays the recipe until that is decided.

## Appendix AZ. Prototype slice 50: yes or no, and scores, from a model (2026-10-22)

Slice 30's model sources (appendix AF) read a typed value, and Jev could not read at all. Slice 22 left "scores or yes/no questions about media, beside picks" for later (appendix V). Both are still decisions: the model picks yes or no, or one point of a scale.

### AZ.1 What slice 50 adds

| Added in slice 50 | Still waits |
|---|---|
| `source ⟨Kind⟩'s ⟨state⟩ via model …`: a yes or no sets a Bool | a state an adapter sources |
| `scored from ⟨n⟩ to ⟨n⟩` on a Count a model reads | scores that are not whole |
| Jev reads a yes or no and a score, as a `choice` question | Jev reading any other value |

### AZ.2 Semantics that slice 50 settles

1. **Yes or no.** A model source may set a Bool. The value read is a JSON boolean, or `"yes"` or `"no"`. An adapter source of a Bool is still an error.
2. **Scores.** `scored from a to b` (whole numbers, a < b) is allowed on a Count a model reads. A value off the scale is not used, and the fetch fails with `it scored n, outside the scale from a to b`. The host checks it, so it holds for every backend.
3. **Backends.**
   - Jev: one `choice` question named `pick`, with criteria `yes` and `no`, or each point of a scale of at most 100 points. The instructions end with `Answer one of: …`.
   - Custom: `"type": "yes or no"`, and `"scale": {"from", "to"}`.
   - Workers AI: a `boolean` field; a scale is named in the instructions.
4. Nothing new is journaled: the answer is the source's `read` or `unread`, as before.

### AZ.3 Acceptance

- runtime/tests/judged.rs:
  - a yes and a score let a payout go ahead, and a no holds it;
  - a score off the scale is not used;
  - an adapter state, a scale on a non-Count, and a reversed scale are errors;
  - Jev's request and answers, for yes or no and for a scale;
  - a custom backend's field and its boolean answer.

## Appendix BA. Counting the work of an input (2026-10-23)

The cost of one input should not grow with the number of units. It is counted, not timed: `state::WORK` counts units decoded and steps taken.

### BA.1 What is measured

runtime/tests/work.rs builds a shop of 100 and of 1000 members and bookings, and checks that each input costs the same (units decoded, steps) in both: a new booking, an adapter answer, a cancel, and a minute with no timer due. When every request times out, each timer costs one step.

### BA.2 Not flat yet

- A choice among a pool decodes every candidate of the pool, and stops looking after `candidates` of them, so a pool whose first candidates are full refuses, and its refusals fill the unit (fixed in slice 52, appendix BC).
- A change to a total that units read through a reference refreshes each of them. A `checked globally` total and shared stock do not: their takers do not read the total.
- Waiters, pending requests, the trace, the gate log and the kernel journal are scanned or kept in full.

These are the next slices' work.

## Appendix BB. Prototype slice 51: hardening (2026-10-23)

A security review of slices 1-50 found the service trusted too much of what reached it. Slice 51 closes what it found.

### BB.1 What slice 51 adds

| Added in slice 51 | Still waits |
|---|---|
| `SUTURE_GATEWAY_TOKEN`, sent as `Suture-Gateway` | callers authenticated by the service itself |
| a media quota, and a role for uploads | a quota per caller |
| scenario files only from the module's directory | |
| at most 8 parentheses, and 32 nested calculations | |
| personal values hashed in `no two` groups | erasing a personal value on request |
| tighter `curl`, redacted adapter errors, a private state directory | an HTTP client in the runtime |

### BB.2 Semantics that slice 51 settles

1. **The gateway.**
   - With `SUTURE_GATEWAY_TOKEN` set, every request but `GET /health` must carry it as `Suture-Gateway`, compared in constant time, or it is answered 401.
   - Listening on an address beyond loopback without the token, `suture serve` does not start.
   - A header line that starts with a space or tab (a folded line) is refused, and so is a second `Suture-Gateway`.
   - `GET /trace` returns at most 1000 lines; `next` reads on.
2. **Media.**
   - `POST /media` needs a caller whose role is one of the module's (403 otherwise).
   - The store keeps at most 1 GiB in all, counted afresh before each upload it writes, since pods share it; past that, an upload is refused.
3. **Scenario files.** `file "⟨name⟩"` names a file in the module's directory: no absolute path and no `..`; after symlinks it must still be inside, be a plain file, and be at most the media limit.
4. **Nesting.**
   - A line with more than 8 parentheses open at once is a lexical error. The parsers try a `(` as a group and then as a calculation, so each level can double their work.
   - A statement may nest at most 32 calculations and conditions (`the total of the total of …`), so no input exhausts the parser's stack.
5. **Personal values.**
   - A `no two` group whose value is a personal attribute is named by the first 12 hex digits of the value's SHA-256, `(personal 3f9a…)`, never by the value.
   - A mistyped answer, and a model's mistyped or unsure read, show a personal value as `(personal)`.
   - A total grouped by a personal value before slice 51 starts again under its new name, as a renamed total does.
   - The hash is not keyed: whoever reads the trace can check a guess at a value with few possibilities. A key per state directory waits.
6. **The state directory.** A new one is made with mode 0700. `secrets.json.tmp` is removed and made anew with mode 0600, so one left by a crash cannot keep a wider mode.
7. **Adapters.**
   - `curl` runs with `-q` (no `~/.curlrc`), `--proto =https,http`, no redirects, and answers of at most 4 MiB.
   - An adapter's failure reaches the trace as `HTTP ⟨status⟩` or curl's exit code, never as the body or curl's message, which may echo what was sent.
8. The LSP reads a message of at most 16 MiB.

### BB.3 Acceptance

- runtime/tests/hardening.rs:
  - behind a gateway, only requests with its token are served, and a folded header is refused;
  - the trace is read a page at a time;
  - only a module's role uploads media;
  - a scenario reads files only inside its module: `..`, an absolute path, and a symlink out are refused;
  - a unique personal value never shows in the trace or the gate log.
- lang lex: deep parentheses are refused before parsing; lang/tests/nesting.rs: deep `the total of` is refused, not a stack overflow.

## Appendix BC. Prototype slice 52: pools that are full first (2026-10-23)

Appendix BA.2 found that a choice among a pool looked only at its first `candidates` in creation order. When those were full, each was asked, refused, and added to the request's skip list, which is kept in the unit. A pool of a few hundred full candidates made the unit pass 4096 bytes, and the step faulted.

### BC.1 What slice 52 adds

| Added in slice 52 | Still waits |
|---|---|
| a pool offers only candidates that would grant the take | an index of the candidates with room |
| candidates decoded only as far as the choice needs | |
| work counted per thread | |

### BC.2 Semantics that slice 52 settles

1. **Only candidates with room.**
   - For a binding that is neither held nor unique, a candidate counts toward `candidates` only if the take would be granted now: the host steps a copy of it, as a wake does (appendix F.2.9).
   - A full candidate is passed over, not asked and refused, so the skip list stays short.
   - When none has room, the first choice still asks the best full one, so the trace names the rule that refused it; after that refusal the binding waits, as before.
   - A model is offered only candidates with room.
   - A held or unique binding is offered its first candidate as before, since its refusal is the rule's answer (appendix AA.2.2, AP.2.2).
2. **Cost.** The pool is read in creation order and stops after `candidates` with room, so a choice costs one step for each full candidate before them. A pool whose early candidates are full still costs in proportion to them.
3. **Counting.** `state::Work` counts per thread, so tests that run side by side do not count each other's work.

### BC.3 Acceptance

runtime/tests/work.rs:
- a pool of 600 Titles, all full but the last: the Loan holds the last, and the next Loan waits;
- a take from a global total, and from shared stock, costs the same at 100 and 1000 readers or holders.

## Appendix BD. Prototype slice 53: what a served host keeps (2026-10-23)

A served host kept everything it had done: the kernel's journal of every inject and reduction, every trace line, every gate decision, every model answer and every timed-out request. Its memory grew with its age, not with its units.

### BD.1 What slice 53 adds

| Added in slice 53 | Still waits |
|---|---|
| a served machine journals nothing | snapshots, so a restart need not replay the whole log |
| the trace and the gate keep their last 100 000 lines | a replay that streams the log |
| model answers and timed-out requests are not kept for ever | |
| readers found without a scan | indexes of pending requests and waiters |

### BD.2 Semantics that slice 53 settles

1. **No kernel journal when served.**
   - The service's log is what replays (appendix L.2.3), so its machine uses `NoJournal`, which keeps no record.
   - A scenario's machine keeps its in-memory journal, so tests can still recover the kernel from it.
2. **Trimming.**
   - Between inputs, `suture serve` keeps the last 100 000 trace lines and gate decisions once there are twice as many. Every line went to standard output when it happened.
   - A line keeps its number: `GET /trace?from=n` reads from line n, or from the first line still kept. `trace_dropped` counts the lines no longer kept.
   - The model answers are forgotten once the service has logged them.
   - The last 100 000 timed-out requests are kept; a late answer to an older one is answered `has no pending request`, not noted as late.
3. **Readers.** Whether a unit already reads another is found in a set, so a new reader of a popular entity costs no scan of its readers.

### BD.3 Acceptance

runtime/tests/memory.rs:
- a trimmed trace keeps its line numbers, and `/trace` skips the lines no longer kept;
- a served machine journals nothing.

## Appendix BE. Prototype slice 54: slow callers and slow adapters (2026-10-23)

The service read each connection, applied it, and posted to adapters on one thread. A caller that sent its headers slowly held every other caller for up to 5 seconds, and a slow adapter held the service for up to 10 seconds per post.

### BE.1 What slice 54 adds

| Added in slice 54 | Still waits |
|---|---|
| connections read on threads of their own | inputs applied side by side |
| adapter posts on 8 threads | model asks off the serving thread |
| | an HTTP client in the runtime instead of `curl` |

### BE.2 Semantics that slice 54 settles

1. **Connections.**
   - A thread accepts connections, and each connection is read on a thread of its own, at most 64 at once; past that, a connection is answered 503.
   - The gateway token is checked as the request is read.
   - The serving thread alone applies requests, one at a time, in the order they were read, so the log's order is still the order of the inputs. It waits at most a tenth of a second for one, then runs the timers that fell due.
2. **Posts.**
   - Once serving starts, a request to an adapter is handed to one of 8 posting threads, if the transport can run on another thread; `curl` can. A transport that cannot, as in tests, posts on the serving thread, as before.
   - The serving thread applies each answer when it comes, as an input, as if it had come at once. A failed post is tried again after `RETRY_SECS`, as before.
   - `suture serve` starts the posting threads before it posts the requests that were waiting at a restart, so a slow adapter does not hold up the start.
   - A request is marked posted while its post runs, so it is not posted twice. A post still running when the service stops is posted again after a restart, with the same idempotency key (appendix L.2.6).

### BE.3 Acceptance

- runtime/tests/hardening.rs: a caller that sends half its headers holds up no other request.
- runtime/tests/http.rs: while an adapter holds a payment, bookings are still answered, and its answer is applied when it comes.

## Appendix BF. Prototype slice 55: slow models (2026-10-23)

After slice 54, a model was still asked on the serving thread, so a slow one held the service for up to its `within`.

### BF.1 What slice 55 adds

| Added in slice 55 | Still waits |
|---|---|
| models asked on threads of their own, 8 at once | |
| a backend that makes itself on another thread | |

### BF.2 Semantics that slice 55 settles

1. **Models.**
   - Once serving starts, the model of a waiting choice (appendix S.2.2), or of a source's request (appendix AF.2.3), is asked on a thread of its own, if its backend can be made there. Jev, a custom backend and Workers AI can, over `curl`. A registered function, a scripted answer, or a backend over a test transport is asked on the serving thread, as before.
   - At most 8 models think at once; the rest are asked when one answers.
   - The serving thread logs and applies each answer when it comes, as `decided` for a choice and as the request's answer for a source. A source's answer is judged (type, scale, how sure) as it is applied.
   - A choice whose deadline passes while its model thinks is made by `prefer`, as before; the late answer is logged and noted as `nothing waits for it`.
2. **Backends.** `Backend::on_another_thread` gives what makes the same backend on another thread, with its transport's `on_another_thread`. It is a function, not a backend, since a backend's transport need not be `Send`.

### BF.3 Acceptance

runtime/tests/deferred.rs: while a model's API holds its answer, ticks and inputs go on, and its pick is applied when it comes.

