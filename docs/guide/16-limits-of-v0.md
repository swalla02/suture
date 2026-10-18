# 16. Limits of v0

Suture v0 is a prototype. Everything earlier in this guide runs and is tested. This chapter lists what is designed (in `docs/superpowers/specs/2026-10-14-suture-language-design.md`) but not built yet, so you do not go looking for it.

## Running for real

- **One process.** `suture serve` (chapter 17) runs a program as one process, applying one input at a time. Several runtime pods sharing the work, with failover between them, are a later slice.
- **The log only grows.** It is never compacted or snapshotted, so a restart replays every input since the start.
- **No versioning yet.** A log written under another program is refused; a deploy that resolves live units under a changed program is designed but not built.
- **Whole seconds.** The clock and every timer count whole seconds.
- **HTTP only.** The service speaks plain HTTP and leaves TLS to the gateway in front of it. Adapters are reached over HTTP through `curl`.
- **No `suture test` or `suture deploy`.** Tests run through `cargo test` and `suture run`. Deploy checks against live state (refusing a new version until every rule that live entities would break has a resolution) are designed for a later sub-project.
- **Alerts** appear only in the trace and in `suture explain`; they are not sent anywhere.

## Security

- **Callers are named, not authenticated.** The gate checks roles and conditions. A scenario says who the caller is, and a served program trusts the `Suture-Caller` header, which the gateway must set after it has authenticated the caller. Operator requests need one shared token.
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
| a source whose answer carries more than one value | one attribute per source |
| sourcing a `Bool` state | source a Text or Count and compare it in the rule |
| a formula, a `becomes`, or a total's filter that stays fresh over a sourced fact | read the sourced fact in the rule itself |
| prefetching | facts are fetched when a step waits on them |
| asking a model without holding up the step | a model is asked in the step that needs it, which waits up to the model's `within`; keep it short |
| keys from anywhere but an environment variable | `models.json` names the variable with `key_env` |
| a model that sees the whole unit | it sees the entity that chooses, with its own facts, and the options with theirs |
| two `decide`s with a `whose` at the same rank, even when their conditions exclude each other | give one of them a nearer scope or a choice point |
| waking only as many waiting bindings as a freed amount can serve | every waiter of the pool key is woken, in arrival order, and gathers again |

## By design, never

Some things are left out on purpose, and will stay out:

- **Control flow.** No `if`, loops, calls or explicit steps.
- **`now` in sentences.** A rule must not change truth while nothing happens. Time enters through deadlines, patience, goals and `created`.
- **A `uses` with no pool key.** It would make every choice read every candidate.
- **Undoing the outside world.** A requested effect always counts, and a timed-out request is never replaced automatically.
- **A model deciding what a program means.** Parsing, checking, read-backs, errors and suggestions are fixed code, and the same program always gets the same result.
