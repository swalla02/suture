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
