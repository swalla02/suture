# models: decision models choose among allowed options; extraction models read one value

Suture never prompts a model and never generates code. A model only picks among options the rules allow, or reads one typed value, and every answer is journaled.

```suture
module claims version 1, currency EUR

role adjuster

kind Claim    { note: Text }
kind Repair   { claim: Claim }
kind Writeoff { claim: Claim }

Claim needs Outcome
Outcome is Repair or Writeoff
prefer Outcome: Repair, then Writeoff
decide Outcome with model "assessor@1" within 2 seconds at least 0.8 sure otherwise Writeoff

admit file a new Claim by adjuster
```

```scenario
scenario "an unsure model leaves the claim to the fallback":
  when   model "assessor@1" picks Repair at 0.6
  and    adjuster Ann files a new Claim C1 with note "roof"
  expect C1 has a Writeoff
```

- `decide ⟨choice⟩ with model "⟨name⟩@⟨version⟩" [within …] [at least ⟨0…1⟩ sure] [otherwise ⟨Option⟩]`. Without a model's help (declined, late, unsure, failed) the `otherwise` option is taken, or `prefer`'s first.
- `decide … with model "a@1" or model "b@1", picked by model "router@1"`: a model picks the model.
- `source ⟨Kind⟩'s ⟨attr⟩ via model "invoices@1" within 10 seconds` with `sends its invoice`, `asking "…"`, `at least 0.8 sure`, `otherwise …` reads a value, from `Media` too.
- In scenarios models are scripted: `model "assessor@1" picks Repair at 0.9`, `reads 120 EUR at 0.9`, `declines`, `is late`. Backends (`models.json`) are for `suture run --models` and `suture serve --models` only.
