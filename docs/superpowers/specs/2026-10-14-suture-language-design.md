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
7. **The stand-in.** In v0 the host holds a registry of models by `name@version`. Scenarios play them as they play adapters: `when model "carriers@1" picks Hermes[, then DHL]` queues picks, and each time that model is asked (a choice, a re-ask after a refused take, a woken waiter) it takes the next one; with none queued, it declines. A binding with no pool key (F.2.11) chooses nothing and is not a choice point. Scripting a model that no `decide` names fails the scenario. Rust callers may register any function from the options to a pick.
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
7. **Values in JSON** follow the attribute's type: text as a string, a yes or no as a boolean, a count as an integer, an amount as a string with its currency (`"80 EUR"`, `"12.50 EUR"`), a duration or a time as an integer number of seconds, a reference as the entity's label, and a set as a list of labels.
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
