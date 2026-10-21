# 15. Decision models

A program is full of small choices that the rules leave open: which stock a shipment takes, which carrier it goes with, which way an order is delivered, which service to ask for a fact. `prefer` ranks the options with fixed criteria (chapter 9). A **decision model** can pick instead: a function that is given the options and picks one by name. It can be a trained model or a lookup table. Suture does not care, because a model only ever picks among options that are already allowed, and every pick is checked by the rules as before.

`decide` says which model handles which choices. It can be set for the whole app, for one workflow, for one step of a workflow, for one decision, or for any of these under a condition. The most specific one wins.

## The program

```suture
module shop version 1, currency EUR

role staff

kind Product  { name: Text }
kind Stock    { product: Product, units: Count }
kind Carrier  { name: Text, zone: Text, fee: Money }
kind Order    { product: Product, zone: Text, quantity: Count, total: Money }
kind Shipment { order: Order }

Order    needs Shipment
Shipment uses  a Stock where the Stock's product is the Order's product,
               taking the Order's quantity
Shipment uses  a Carrier where the Carrier's zone is the Order's zone

effect send Shipment via carriers within 7 days
  sends    its Carrier's name
  reports  delivered

rule stock: the total quantity taken from each Stock is at most its units

prefer Carrier: lowest fee
prefer Stock:   highest units

admit place a new Order by staff
```

With no `decide`, `prefer` picks: the cheapest carrier and the fullest stock.

```scenario
scenario "prefer picks":
  given  a Product Kettle with name Kettle
  and    a Stock Small with product Kettle and units 1
  and    a Stock Big with product Kettle and units 9
  and    a Carrier Cheap with name Cheap, zone EU and fee 5 EUR
  and    a Carrier Fast with name Fast, zone EU and fee 9 EUR
  when   staff Sam places a new Order O1 with product Kettle, zone EU, quantity 1 and total 50 EUR
  expect O1's Shipment's Carrier's name is "Cheap"
```

## The levels

```text
decide [⟨Kind⟩] [for ⟨Kinds⟩ [whose ⟨cond⟩]] with model "⟨name⟩@⟨version⟩"
decide [⟨Kind⟩] [for ⟨Kinds⟩ [whose ⟨cond⟩]] with prefer
```

| Level | Written | Covers |
|---|---|---|
| app | `decide with model "planner@3"` | every choice in the module |
| workflow | `decide for Orders with model "orders@2"` | every choice in a unit whose root is an Order |
| step | `decide for Shipments with model "shipping@1"` | the choices a Shipment makes, or a part under it |
| decision | `decide Carrier with model "carriers@1"` | the choice of a Carrier, wherever it is made |
| both | `decide Stock for Shipments with model "stock@4"` | the choice of a Stock by a Shipment |
| condition | `… for Orders whose total is at least 1000 EUR …` | the same, only while the condition holds |

A workflow and a step are written the same way: `for Orders` names a root kind, so it covers a whole workflow, and `for Shipments` names a part kind, so it covers one step of it.

### The app

```suture
decide with model "planner@3"
```

Now `planner@3` makes every choice. In scenarios, models are played like adapters: `model "planner@3" picks Fast, then Big` queues the stand-in's next picks, one per choice it handles. Here the shipment chooses its carrier first, then its stock:

```scenario
scenario "the app's model picks":
  given  a Product Kettle with name Kettle
  and    a Stock Small with product Kettle and units 1
  and    a Stock Big with product Kettle and units 9
  and    a Carrier Cheap with name Cheap, zone EU and fee 5 EUR
  and    a Carrier Fast with name Fast, zone EU and fee 9 EUR
  when   model "planner@3" picks Fast, then Small
  and    staff Sam places a new Order O1 with product Kettle, zone EU, quantity 1 and total 50 EUR
  expect O1's Shipment's Carrier's name is "Fast"
  expect the total quantity taken from Small is 1
```

The trace names the model and the options it had:

```text
      choice: Fast for O1/Shipment#1@Carrier by model "planner@3" (of Cheap, Fast)
      choice: Small for O1/Shipment#1@Stock by model "planner@3" (of Big, Small)
```

The options come in `prefer` order, best first, so a model can see what `prefer` would have picked.

Each time a model is asked, it takes the next queued pick: once per choice, and once more for each re-ask (after a refused take, or when a waiting binding is woken). A total across units, such as `the total amount of their Refunds` (chapter 9), chooses nothing and never asks a model. Scripting a model that no `decide` or source names fails the scenario.

### One decision

```suture
decide Carrier with model "carriers@1"
```

Naming the choice point is more specific than any scope, so carriers now go to `carriers@1`, while `planner@3` keeps the stock:

```scenario
scenario "the carrier's model beats the app's":
  given  a Product Kettle with name Kettle
  and    a Stock Small with product Kettle and units 1
  and    a Stock Big with product Kettle and units 9
  and    a Carrier Cheap with name Cheap, zone EU and fee 5 EUR
  and    a Carrier Fast with name Fast, zone EU and fee 9 EUR
  when   model "planner@3" picks Big
  and    model "carriers@1" picks Fast
  and    staff Sam places a new Order O1 with product Kettle, zone EU, quantity 1 and total 50 EUR
  expect O1's Shipment's Carrier's name is "Fast"
  expect the total quantity taken from Big is 1
```

### A workflow, and a step of it

```suture
decide for Orders with model "orders@2"
decide Stock for Shipments with model "stock@4"
```

`for Orders` beats the app level for every choice in an order's workflow, and `for Shipments` is nearer the shipment that chooses, so it beats `for Orders`:

```scenario
scenario "the step's model beats the workflow's":
  given  a Product Kettle with name Kettle
  and    a Stock Small with product Kettle and units 1
  and    a Stock Big with product Kettle and units 9
  and    a Carrier Cheap with name Cheap, zone EU and fee 5 EUR
  and    a Carrier Fast with name Fast, zone EU and fee 9 EUR
  when   model "stock@4" picks Small
  and    model "orders@2" picks Big
  and    model "carriers@1" picks Cheap
  and    staff Sam places a new Order O1 with product Kettle, zone EU, quantity 1 and total 50 EUR
  expect the total quantity taken from Small is 1
```

### Under a condition

```suture
decide Carrier for Orders whose total is at least 1000 EUR with model "careful@1"
```

A `whose` narrows a scope to the entities it holds for, read when the choice is made. Large orders now go to `careful@1`, and the rest stay with `carriers@1`:

```scenario
scenario "a large order goes to the careful model":
  given  a Product Kettle with name Kettle
  and    a Stock Small with product Kettle and units 1
  and    a Stock Big with product Kettle and units 9
  and    a Carrier Cheap with name Cheap, zone EU and fee 5 EUR
  and    a Carrier Fast with name Fast, zone EU and fee 9 EUR
  when   model "careful@1" picks Fast
  and    model "carriers@1" picks Cheap
  and    staff Sam places a new Order O1 with product Kettle, zone EU, quantity 1 and total 5000 EUR
  and    staff Sam places a new Order O2 with product Kettle, zone EU, quantity 1 and total 50 EUR
  expect O1's Shipment's Carrier's name is "Fast"
  expect O2's Shipment's Carrier's name is "Cheap"
```

`with prefer` takes a scope back from the models: `decide Stock for Orders whose total is at most 10 EUR with prefer` lets `prefer` pick the stock of small orders.

## Which one wins

For each choice, every `decide` that applies is ranked:

1. one that names the choice point beats one that does not;
2. then one scoped nearer the part that chooses beats one further up: the part itself, then its owner, and so on up to the root, then the app;
3. then one with a `whose` beats one without.

The compiler checks every choice point against every `decide`. Two that tie can both apply, and neither wins, so the program does not compile:

```suture fails
decide Carrier with model "fleet@1"
```

```errors
main.st:31: `decide Carrier with model "fleet@1"` and `decide Carrier with model "carriers@1"` (line 27) can both decide a Shipment's Carrier, and neither is more specific
```

In v0, two `decide`s with a `whose` at the same rank always count as a tie, even when their conditions could never both hold.

The other errors: naming something that no `uses`, slot or source picks, naming a sourced attribute with only one source, a scope in which the choice is never made, a model name without `@version`, a `within` on `with prefer`, and a `within` outside 1 millisecond to 10 minutes.

## When the model is no help

`prefer` is always behind the model. It picks when:

- the model declines (a stand-in with no pick queued declines);
- the model picks something it was not offered;
- no model of that name is available;
- the model does not answer within its deadline;
- a backend is unsure (its confidence is below its `min_confidence`), or fails.

The trace says which:

```text
      choice: Cheap for O1/Shipment#1@Carrier by prefer (model "carriers@1" declined)
      choice: Cheap for O1/Shipment#1@Carrier by prefer (model "carriers@1" picked Slow, which it was not offered)
      choice: Cheap for O1/Shipment#1@Carrier by prefer (model "carriers@1" is not available)
      choice: Cheap for O1/Shipment#1@Carrier by prefer (model "carriers@1" did not answer within 1000 ms)
      choice: Cheap for O1/Shipment#1@Carrier by prefer (model "carriers@1" was unsure, at 0.41)
      choice: Cheap for O1/Shipment#1@Carrier by prefer (model "carriers@1" failed: HTTP 401: …)
```

A model is asked only when there is a choice. With one option, that option is taken, and the trace shows no model.

A model cannot break a rule either. Its pick is still asked to grant the take, and a pick that would break a rule is refused. The option is then dropped, and the choice is made again among the rest. A model that picks `Small` (1 unit) for an order of 2 sees `Small` refused; only `Big` is left, so `Big` is taken without asking.

## Pinned and replayed

Every `decide`, with its model's name and version, is part of the compiled program. A journal recorded under `planner@3` does not replay under `planner@4`, just as it does not replay under a changed rule. Every pick is journaled, so replay never calls the model again and always gives the same state.

`suture build` lists who handles each choice point, most specific first, ending with the `prefer` fallback:

```text
choices:
  Shipment uses a Stock: pool key Stock's product is its order's product, up to 8 candidates, ranked by `prefer Stock` (1 criterion)
    decided by: for Shipments: model "stock@4"; otherwise `prefer Stock`
  Shipment uses a Carrier: pool key Carrier's zone is its order's zone, up to 8 candidates, ranked by `prefer Carrier` (1 criterion)
    decided by: for Orders whose total is at least 1000 EUR: model "careful@1"; model "carriers@1"; otherwise `prefer Carrier`
```

## Models from Rust

Outside scenarios, a host registers models as functions of the choice. A `Choice` holds the choice point, the unit, the binding's key and the options with their attributes, best first under `prefer`:

```text
host.models.register("dearest@1", Box::new(|c: &Choice| {
    c.options.iter().max_by_key(|o| fee(o)).map(|o| o.label.clone())
}));
```

Returning `None` declines. A `Choice` also holds the kind of the entity that chooses and its facts. `host.models.register_backend` takes anything that implements `Backend`, which is how the Jev and custom backends below are built.

## Slots

A slot's alternative is a choice too (chapter 4). `decide Return …` hands it to a model; an app-level or workflow-level `decide` covers it as well.

```suture
module returns version 1, currency EUR

kind Applicant   { name: Text }
kind Application { applicant: Applicant, paid: Bool = no }
kind Courier     { application: Application }
kind Collection  { application: Application }
kind Locker      { application: Application }

Application needs Return
Return      is    Courier or Collection or Locker

effect send Courier via couriers within 2 days
  sends    the application's applicant's name
  marks    sent
  reports  delivered | fails with lost
effect notify Collection via mailer within 1 hour
  sends    the application's applicant's name
  reports  notified
effect open Locker via lockers within 1 hour
  sends    the application's applicant's name
  reports  opened

rule prepaid: no Application has a Courier that is sent unless it is paid

prefer Return: Courier, then Collection after 3 days, then Locker
decide Return with model "routing@1"
```

The model is asked when the slot is first filled, and again each time it moves on: after a failure, after its patience, or after a refusal on fixed facts. It is offered the alternatives not yet tried, in `prefer` order. Patience is still per alternative.

```scenario
scenario "the model sends an unpaid application's return to a locker":
  given  an Applicant Ada with name Ada
  when   model "routing@1" picks Courier, then Locker
  and    a new Application A1 with applicant Ada
  and    lockers answers opened
  expect A1's Locker is opened
```

```text
A1  Return: asking model "routing@1"
      choice: Courier for A1?Return#1 by model "routing@1" (of Courier, Collection, Locker)
A1  Return: trying Courier
A1  needs Courier
A1  Return: Courier refused by rule prepaid, asking model "routing@1"
      choice: Locker for A1?Return#2 by model "routing@1" (of Collection, Locker)
A1  Return: trying Locker
```

While the model is asked, the slot is empty and nothing is created for it. Its answer, or `prefer`'s in its place, comes back to the unit as a message, so it is journaled like any answer, and the unit checks it against the options once more.

## Deadlines

Every model answer has a deadline: 1 second, or what `within` says.

```text
decide Return with model "routing@1" within 300 milliseconds
decide Order's credit with model "sources@1" within 2 seconds
```

A model that is later than that leaves the choice to `prefer`, like one that declines. In scenarios, `model "routing@1" is late` plays a late answer, and `model "routing@1" declines` a decline:

```scenario
scenario "a late model leaves the return to prefer":
  given  an Applicant Ada with name Ada
  when   model "routing@1" is late
  and    a new Application A1 with applicant Ada and paid yes
  and    couriers answers delivered
  expect A1's Courier is delivered
```

```text
      choice: Courier for A1?Return#1 by prefer (model "routing@1" did not answer within 1000 ms)
```

The deadline is part of the program, like the model's name. The journal holds the answer, or its absence, so replay never waits.

### Served programs ask after the step

In a scenario, a model is asked in the step that needs it. A served program (chapter 17) does not hold the step up for it. The choice waits, and the trace says so:

```text
      choice for O1/Shipment#1@Carrier: model "carriers@1" is asked
```

The input is answered at once. The service then asks the model as it ticks, and logs its answer as an input of its own. If no answer has been logged when the deadline runs out, `prefer` picks, as for a late model. The options are gathered again when the answer arrives, so a pick that stopped being an option meanwhile is left to `prefer` as well. With pods (chapter 20), the pod that owns the unit asks its model.

## Sources

An attribute with more than one source (chapter 14) is a choice point named `K's a`: the model picks which source to ask.

```suture
module credit version 1, currency EUR

role staff

kind Customer { name: Text }
kind Order    { customer: Customer, total: Money, credit: Money }
kind Payment  { order: Order }

Order needs Payment

effect capture Payment via payments within 30 seconds
  sends    the order's total
  marks    sent
  reports  captured

source Order's credit via bureau within 2 seconds
  sends      the customer's name
  reports    found | fails with unknown
source Order's credit via backup within 2 seconds
  sends      the customer's name
  reports    found | fails with unknown
  otherwise  use 0 EUR

rule credit: no Order has a Payment that is sent unless its total is at most its credit

decide Order's credit with model "sources@1"

admit place a new Order by staff
```

The model is asked when a fetch starts, and again when a source fails, among those the fetch has not tried yet. When one source is left, it is asked without the model; when all have failed, the `otherwise` applies.

```scenario
scenario "the model asks the backup first":
  given  a Customer Bo with name Bo
  when   model "sources@1" picks backup
  and    staff Sam places a new Order O1 with customer Bo and total 50 EUR
  and    backup answers unknown
  and    bureau answers found with credit 80 EUR
  and    payments answers captured
  expect O1's credit is 80 EUR
```

```text
O1  credit: asking model "sources@1" for a source
      choice: backup for O1~credit#1?source by model "sources@1" (of bureau, backup)
O1  credit: asking backup (name "Bo")
      backup: unknown   (O1~credit#1)
O1  credit: backup failed (unknown), asking bureau (name "Bo")
```

## Real backends

A scenario's stand-ins and Rust functions are enough to test a program. To run it against a real decision model, put a `models.json` beside the module and run with `--models`:

```text
{
  "routing@1": { "model": "jev-latest", "key_env": "JEV_API_KEY", "min_confidence": 0.6 },
  "sources@1": { "backend": "custom", "url": "https://models.example.com/pick",
                 "key_env": "PICK_KEY", "pick": "/result/pick", "confidence": "/result/p" }
}
```

```text
suture run examples/returns --scenario examples/returns/returns.scenario --models
```

A scenario's own scripted answers still come first; `--models` answers the rest. Without `--models`, a model that no scenario scripts is not available, and `prefer` picks. Endpoints and keys are host settings, not program text: changing them does not change the program, and the key is read from the environment variable `key_env` names, never from the file.

### Jev, the default

A model with no `backend`, or `"backend": "jev"`, is asked through Jev's decision API (`POST https://api.typesafe.ai/v1/systemone`, with the key as a bearer token). The fields are `model` (default `jev-latest`), `url`, `key_env` (default `JEV_API_KEY`) and `min_confidence`. Each choice is one `choice` question named `pick`. The entity that chooses is the state, and the options are the criteria, each described by its facts:

```text
{
  "model": "jev-latest",
  "state": { "unit": "O1", "chooser": "Shipment", "facts": { "weight": 3 } },
  "questions": {
    "pick": {
      "type": "choice",
      "instructions": "Choose the Carrier for O1 (Shipment). The options, best first by the program's own preference: Cheap, Fast.",
      "criteria": { "Cheap": "fee 5 EUR, zone EU", "Fast": "fee 9 EUR, zone EU" }
    }
  }
}
```

The pick is `answers.pick.choice`. With a `min_confidence`, a pick whose `answers.pick.confidence` is lower is not used, and `prefer` picks:

```text
{ "model": "jev-1.13.0",
  "answers": { "pick": { "type": "choice", "choice": "Fast", "probabilities": { "Cheap": 0.1, "Fast": 0.9 }, "confidence": 0.9 } },
  "usage": { "input_tokens": 120, "output_tokens": 1 } }
```

An HTTP error, an answer without `answers.pick.choice`, or a missing key fails the model, and `prefer` picks.

### A custom format

For another API, `"backend": "custom"` posts the same state with the options as a list, and reads the pick (and, optionally, the confidence) at the JSON pointers you name:

```text
{ "point": "Carrier",
  "state": { "unit": "O1", "chooser": "Shipment", "facts": { "weight": 3 } },
  "options": [ { "label": "Cheap", "facts": { "fee": "5 EUR", "zone": "EU" } },
               { "label": "Fast",  "facts": { "fee": "9 EUR", "zone": "EU" } } ] }
```

`url` and `pick` are required; `key_env`, `confidence` and `min_confidence` are optional. Anything else, an unknown field among them, is an error when the file is read, before any scenario runs. For a format that JSON pointers cannot reach, implement `Backend` in Rust.

Whatever the backend, the model only ever picks among the options it is offered. Its pick is checked, journaled, and still subject to every rule.

## Media

A model can look at an image, listen to a recording or read a document before it picks. Give the entity that chooses, or the options, an attribute of type `Media`:

```text
kind Claim    { photo: Media, note: Text }
kind Repair   { claim: Claim }
kind Writeoff { claim: Claim }
Claim   needs Outcome
Outcome is    Repair or Writeoff
prefer Outcome: Repair, then Writeoff
decide Outcome with model "assessor@1"
```

A unit holds only a reference to the media: its type, its size and the SHA-256 of its content. The content is kept in the media store, once, by that hash. So the trace shows the reference:

```text
C1  placed: note "door", photo (image/png, 48213 bytes, sha256 9f2c41d07a1b)
      choice: Repair for C1?Outcome#1 by model "assessor@1" (of Repair, Writeoff)
```

No sentence, formula, pool key or adapter's `sends` can read media: what the program means never depends on it, and a model still only picks among the options. In scenarios, `file "dent.png"` gives media, read from beside the scenario file; the type comes from the extension (`.png`, `.jpg`, `.gif`, `.webp`, `.mp3`, `.wav`, `.ogg`, `.mp4`, `.webm`, `.pdf`, `.txt`). A served program takes media through `POST /media` (chapter 17).

```text
when adjuster Ann files a new Claim C1 with photo file "dent.png" and note "door"
```

A backend gets each piece of media among the facts as an object, with its content in base64:

```text
"photo": { "media": "image/png", "bytes": 48213, "sha256": "9f2c…", "base64": "iVBORw0KGgo…" }
```

Two settings in `models.json` change that. `"media": "omit"` sends the reference alone, for a model that reads media from somewhere else. `"max_media_bytes"` caps how much content one choice sends: 6 MiB by default, and at most, so that the request stays within what curl reads. If a choice has more media than that, or media the store does not hold, the model is not asked and `prefer` picks:

```text
      choice: Repair for C1?Outcome#1 by prefer (model "assessor@1" failed: the media is 9437184 bytes, over the 8388608 this model takes)
```

A model from Rust finds the content in `Choice::media`, by hash.

## Models that read

Some models do not pick. An extraction model, such as Cloudflare's Clairic, reads a typed value out of text and images: an amount off a photo of a garage's estimate, or a date out of an email. Each answer comes with a confidence and nothing else, so such a model is a source, like the bureaus of chapter 14, but it can read media:

```suture
module claims version 1, currency EUR

role staff

kind Claim  { note: Text, amount: Money, estimate: Money, approved: Bool = no }
kind Payout { claim: Claim }

Claim needs Payout

effect pay Payout via bank within 1 day
  sends    the claim's amount
  marks    sent
  reports  paid

source Claim's estimate via model "reader@1" within 10 seconds
  sends      its note
  asking     "the repair cost the garage's estimate shows"
  at least 0.8 sure
  otherwise  ask again after 1 hour

rule estimate: no Claim has a Payout that is sent unless its amount is at most its estimate or it is approved

admit file a new Claim by staff
admit change Claim's approved by staff
```

A source `via model` has no `reports` line: the model reads the value or it does not. `sends` says what it reads, and unlike an adapter's it may name media: `sends its photo, its note`. `asking` is the instructions it gets for the value, and `at least 0.8 sure` is the confidence below which a value is not used. The bound is program text, so the read-back states it and it holds in every deployment. Everything else is as for any source: the value is fetched only when a step its rules hold back would go ahead, `good for` and `otherwise` apply, and `decide K's a` can choose between a model and an adapter.

A model that finds nothing, is less sure than the bound, is late, fails, or gives a value of the wrong type has not read it, and the fetch fails:

```scenario
scenario "an unsure reading is read again an hour later":
  when   model "reader@1" reads 1200 EUR at 0.5, then 1200 EUR at 0.9
  and    staff Sam files a new Claim C1 with note "door, 1200 EUR" and amount 1200 EUR
  expect C1's Payout is not sent
  when   1 hour passes
  expect C1's estimate is 1200 EUR
  expect C1's Payout is sent
```

```text
C1  estimate: asking model "reader@1" to read it (note "door, 1200 EUR")
      model "reader@1" did not read C1's estimate: it read 1200 EUR at 0.50, and the source needs at least 0.8 sure
      reader@1: unread   (C1~estimate#1)
C1  estimate: model "reader@1" did not read it: asking again in 1 hour
```

In a scenario, `model "reader@1" reads 1200 EUR [at 0.9][, then …]`, `… reads nothing` and `… is late` script the model. A value without `at` gives no confidence, so a source with a bound does not use it. Scenarios ask the model as soon as the fetch is made. A served program asks it after the step, as it posts to an adapter, and logs the answer as that request's answer, `read` with the value or `unread`. Replay takes the value from the log and does not ask the model again.

Secret and personal attributes are not sent to a model. A formula cannot read what a model reads, as for any source (chapter 14).

### Backends that read

`"backend": "workers-ai"` reads with Clairic on Cloudflare Workers AI. `url` is your account's `https://api.cloudflare.com/client/v4/accounts/⟨account⟩/ai/run/@cf/cloudflare/clairic`, and the key comes from `key_env`, which defaults to `CLOUDFLARE_API_TOKEN`:

```text
{ "reader@1": { "backend": "workers-ai", "url": "https://api.cloudflare.com/client/v4/accounts/…/ai/run/@cf/cloudflare/clairic" } }
```

The facts are sent as the state's text, with up to four images inline as data URLs, and the attribute as one typed field:

```text
{ "state": "Claim C1\nphoto: image 1\nnote: door",
  "images": ["data:image/png;base64,iVBORw0KGgo…"],
  "fields": { "estimate": { "type": "number", "decimals": 2, "unit": "EUR", "instructions": "the repair cost the garage's estimate shows" } } }
```

Text is a `text` field, Count a `number` with `integer`, Money a `number` with two decimals in the module's currency, Time a `datetime`, and Duration a `duration` in seconds. The value is read at `answers.estimate.value`, or at `result.answers.estimate.value` as the REST API wraps it, with its `confidence`. A `p_null` of 0.5 or more means the value is not in the input, so nothing is read. Cloudflare's post documents the field types, but not the name of the images field, so `images` may change.

A custom backend reads too when it has a `value` pointer. It posts `{ "point": "read", "state": { "unit", "kind", "facts" }, "field": { "name", "type", "instructions" } }` and reads the value at `value` and the confidence at `confidence`. Jev picks, and does not read.
