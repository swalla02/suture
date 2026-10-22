# limits: what cannot be written yet, and what to do instead

Say so in your report when a requirement needs one of these, instead of forcing it.

| Cannot (yet) | Instead |
|---|---|
| read the clock (`now`, "is past its due date") in a sentence | a part whose effect times out (`suture help time`), or a `desired within` alert |
| a reaction that sets an attribute of another entity it references | derive it: a `define` on that entity reading a total of its references, e.g. `define: a Book is lent when the number of its Loans that are open is at least 1` needs a total the program keeps |
| holidays and half days in a count of days | `the days from … to …` counts every calendar date; take the working days as an input attribute, and say so |
| `and` and `or` in one sentence | name one side with a `define` |
| "at most once per reviewer per talk": uniqueness of a pair, or a count filtered by the caller | not yet: model the pair as its own entity the caller creates, and say the duplicate pair is not prevented |
| a negative `Count` (`score - 3`) | a `Count` result below 0 is not used as negative; compare totals instead (`the total score … is at least the number … * 3`), or use `Money` |
| a rule refusing the creation of an entity whose own parts would break it | a rule refuses an input, but the system's own work (a part, a take) waits until the rule allows it; to refuse a request outright, put the condition in its `admit … where` |
| an unfinished request taken back once sent | guard the state it sets with the fact that ends it (`late when overdue and not returned`) |
| loops, functions, arbitrary code | none, by design: write rules and needs; outside work is an effect, played in scenarios |
| a model writing text or code | none, by design: models pick or read one value |
