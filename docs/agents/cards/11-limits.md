# limits: what cannot be written yet, and what to do instead

Say so in your report when a requirement needs one of these, instead of forcing it.

| Cannot (yet) | Instead |
|---|---|
| read the clock (`now`, "is past its due date") in a sentence | a part whose effect times out (`suture help time`), or a `desired within` alert |
| a reaction that sets an attribute of another entity it references | derive it: a `define` on that entity reading a total of its references, e.g. `define: a Book is lent when the number of its Loans that are open is at least 1` needs a total the program keeps |
| holidays and half days in a count of days | `the days from … to …` counts every calendar date; take the working days as an input attribute, and say so |
| `and` and `or` in one sentence | name one side with a `define` |
| an unfinished request taken back once sent | guard the state it sets with the fact that ends it (`late when overdue and not returned`) |
| loops, functions, arbitrary code | none, by design: write rules and needs; outside work is an effect, played in scenarios |
| a model writing text or code | none, by design: models pick or read one value |
