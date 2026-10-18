# 15. Decision models

A program is full of small choices that the rules leave open: which stock a shipment takes, which carrier it goes with. `prefer` ranks the options with fixed criteria (chapter 9). A **decision model** can pick instead: a function that is given the options and picks one by name. It can be a trained model or a lookup table. Suture does not care, because a model only ever picks among options that are already allowed, and every pick is checked by the rules as before.

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

The other errors: naming a kind that no `uses` picks, naming a slot (v0 picks a slot's alternative with `prefer` and patience only), a scope in which the choice is never made, and a model name without `@version`.

## When the model is no help

`prefer` is always behind the model. It picks when:

- the model declines (a stand-in with no pick queued declines);
- the model picks something it was not offered;
- no model of that name is available.

The trace says which:

```text
      choice: Cheap for O1/Shipment#1@Carrier by prefer (model "carriers@1" declined)
      choice: Cheap for O1/Shipment#1@Carrier by prefer (model "carriers@1" picked Slow, which it was not offered)
      choice: Cheap for O1/Shipment#1@Carrier by prefer (model "carriers@1" is not available)
```

A model cannot break a rule either. Its pick is still asked to grant the take, and a pick that would break a rule is refused. The option is then dropped, and the same model is asked again among the rest. A model that picks `Small` (1 unit) for an order of 2 sees `Small` refused, and is asked again with only `Big` left.

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

Returning `None` declines. In v0 a model answers at once; deadlines for slow models, real model backends, and models for slots and sources are designed but not built (chapter 16).
