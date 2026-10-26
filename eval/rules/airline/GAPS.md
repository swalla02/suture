# Language gaps the airline policy hit

What `suture/airline.st` could not say directly, and how it says it instead. Each item is a candidate for the language slice that comes before tax (spec, Stage 1 step 3).

| # | Gap | Workaround in `airline.st` | Cost of the workaround |
|---|---|---|---|
| 1 | A formula cannot compare or branch (`fee = 30 if weight > 50`). | Every fee is a row of a table, and a pool (`uses … where`) picks the row whose bounds hold. | The overweight and oversize tables become data (`charges.csv`), not sentences. |
| 2 | No `max`. | A bag matches one size row and one weight row; `prefer Charge: highest amount` takes the higher. | Each table needs a $0 row, or a bag within the limits gets no row. |
| 3 | A sentence cannot read what a part was granted; `taking` cannot read the candidate. | The harness adds up the amounts of the granted rows. | The total is not a Suture value: no rule can bound it, and `suture explain` does not show it. |
| 4 | A `define` cannot be read through a reference (`its trip is far`), on a list element, or on a candidate inside a `where`. | The conditions are written out in full. | The Waiver's condition is seven lines, and the policy's "complimentary" is not one named state. |
| 5 | A taken total (`the total waivers taken from …`) is known only in its own unit. | The overweight pick sits on the Bag, behind a `weigh` event that the harness sends after the Allowances. | The order of inputs carries meaning: weighed too early, a bag pays the 50 lbs allowance. |
| 6 | Only one kind may rank a pool kind with `prefer`. | Two row kinds, Charges and Graces, for paid and free allowances; the size rows appear in both. | Duplicated rows, which can drift apart. |
| 7 | A unit holds at most 4096 bytes, and a granted row is carried with everything it refers to. | One row per region, not a set of regions; each bag picks its own fee row; one Allowance per free place; the Waiver's pool leaves out bags that save nothing. | Four changes of shape for size alone, each found by a fault at run time, not by the compiler. |
| 8 | A pool cannot rank by a computed difference (what the allowance saves). | Three hand-derived tiers: heavy and snug, heavy and close, plump and snug. | The tiers hold for these tables only; a new fee could reorder them, and nothing would say so. |
| 9 | A pool gathers 8 candidates by default before ranking. | `candidates 16`. | With 8, a 10-bag case could silently miss its best bag. |
| 10 | Signed numbers, shares and margins (from the spec's known list). | Not needed here. | — |

Items 3, 5 and 7 are the most serious. Item 3 puts the last step of the answer outside the language. Item 5 makes correctness depend on input order. Item 7 turned a size limit into four redesigns, and the compiler caught none of them.
