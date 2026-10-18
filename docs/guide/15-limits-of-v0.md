# 15. Limits of v0

Suture v0 is a prototype. Everything earlier in this guide runs and is tested. This chapter lists what is designed (in `docs/superpowers/specs/2026-10-14-suture-language-design.md`) but not built yet, so you do not go looking for it.

## Running for real

- **Scenarios only.** `suture run` runs scenarios, with the outside services played by the scenario. There are no real adapters yet, `suture run` cannot read inputs from a file, and the clock moves only when a scenario says so.
- **No resuming.** Every step is journaled and every scenario's journal replays to the same state, but nothing yet resumes a host from a journal after a crash, and there is no database backend.
- **No `suture test` or `suture deploy`.** Tests run through `cargo test` and `suture run`. Deploy checks against live state (refusing a new version until every rule that live entities would break has a resolution) are designed for a later sub-project.
- **Alerts** appear only in the trace and in `suture explain`; they are not sent anywhere.

## Security

- **Callers are named, not authenticated.** The gate checks roles and conditions, but in v0 the scenario says who the caller is. Real authentication is a later sub-project.
- **`secret` and `personal` attributes,** the secret store and masking in explanations are designed, not built.
- **The adapter manifest** (the fields each adapter receives and the outcomes it may report) is enforced by the language (`sends`, `reports`), but not yet written out as a separate file.

## Language

| Not yet | Instead, for now |
|---|---|
| `the caller` in rules and reactions | put the condition on the `admit` |
| events with data (`cancels an Order with reason …`) | an admitted change of an attribute, plus the event |
| changing a reference, a set, a formula, `created`, a part's attribute, or an attribute a pool key reads | model the change as a new entity |
| refreshing carried facts that a shared entity's own reactions or answers change | a rule that reads such a fact from another unit is a compile error; carry it with a formula or change it with `admit change` |
| goals about parts, and events sent to parts | state the goal, and admit the event, on the root |
| a `that` filter on a relation across units (`their Refunds that are refunded`) | filter on the subject's own relations |
| sets of values (`set of Text`) | a set of a small kind (`set of Zone`) |
| `attempts` per part kind | one module-wide setting |
| closest patterns for `define`, `desired` and `when` | they get the phrasing table; rules get closest patterns too |
| sourcing an attribute of a kind other units refer to or pick with `uses` | source it on the unit that reads it, or carry it with a formula |
| a source whose answer carries more than one value | one source per attribute |
| prefetching, or a model choosing between sources | facts are fetched when a step waits on them, from their one source |
| `decide ⟨choice⟩ with model "…"` | `prefer`; the choice interface is in place, the model backend is not |
| waking only as many waiting bindings as a freed amount can serve | every waiter of the pool key is woken, in arrival order, and gathers again |

## By design, never

Some things are left out on purpose, and will stay out:

- **Control flow.** No `if`, loops, calls or explicit steps.
- **`now` in sentences.** A rule must not change truth while nothing happens. Time enters through deadlines, patience, goals and `created`.
- **A `uses` with no pool key.** It would make every choice read every candidate.
- **Undoing the outside world.** A requested effect always counts, and a timed-out request is never replaced automatically.
- **A model deciding what a program means.** Parsing, checking, read-backs, errors and suggestions are fixed code, and the same program always gets the same result.
