# Language gaps the airline policy hits

What `suture/airline.st` cannot say directly, and how it says it instead. Slice 64 (spec appendix BO) closed the rest of the gaps Stage 1 found: version 1 of the module, with its workarounds, is at tag `rules-airline-frozen`.

| # | Gap | Workaround in `airline.st` | Cost of the workaround |
|---|---|---|---|
| 1 | A defined value is read only in its own unit, and nothing adds one up across units. | Each bag's `fee` is a defined value; the harness adds the ticket and the bags' fees. | The trip's total is not a Suture value: no rule can bound it, and `suture explain` does not show it. |
| 2 | A `define` is not read on a candidate's grant, and a defined value not on a candidate at all. | What the free allowance needs is stored as formulas, `free` on the Trip and `saving` on the Bag, which a candidate carries. | None here; a value that changes after creation could not be stored this way. |
| 3 | The compiler bounds no unit size. A unit holds 64 KiB, and a granted row is carried with everything it refers to. | `candidates 16`, and the Waiver's pool leaves out bags that save nothing. | A large enough pool would still fault at run time, not at compile time. |
| 4 | A pool gathers 8 candidates by default before ranking. | `candidates 16`. | With 8, a 10-bag case could silently miss its best bag. |
| 5 | The policy text says "Not accepted", or gives no fee (over 100 lbs, over 115 in), where RuleArena's script charges one. | The formulas follow the script, the ground truth. Suture could refuse such a bag, but then S1 and R would differ on bags the generator never makes. | A refusal the text asks for is not modelled. |

Item 1 matters most for tax, where the answer is a sum of computed lines.
