# models: optional, and never generative

A decision model picks among options the rules allow; an extraction model reads one typed value. Scenarios script them; `models.json` backs them only with `--models`.

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

Also: `decide … with model "a@1" or model "b@1", picked by model "router@1"` (the router sees the options the model it picks will choose among), and `source ⟨Kind⟩'s ⟨attr⟩ via model "invoices@1" within 10 seconds` with `sends its invoice`, `asking "…"`, `scored from 1 to 5` (a Count), `at least 0.8 sure`, `otherwise use ⟨literal⟩`; on a Bool the model answers yes or no.
