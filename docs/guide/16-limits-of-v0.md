# 16. Limits of v0

Suture v0 is a prototype. Everything earlier in this guide runs and is tested. This chapter lists what is designed (in `docs/superpowers/specs/2026-10-14-suture-language-design.md`) but not built yet, so you do not go looking for it.

## Running for real

- **Pods copy the state.** Several pods (chapter 20) share one log on one volume, and each applies every input: they keep the service up and share out the adapter traffic, but do not split the work of applying inputs. A log replicated over the network, without a shared volume, is a later slice.
- **The log only grows.** It is never compacted or snapshotted, so a restart, or a new pod, replays every input since the start.
- **Deploys happen at start.** A new version is deployed when `suture serve` starts it (chapter 18), not while the service runs. Going back to an older version is not supported.
- **Resolutions are few.** A rule over totals (`per-group` or `global`) cannot apply from a version, and a value cannot be migrated by a formula; change the rule, or give the attribute a default. A changed formula keeps the values it gave on live units.
- **Totals are not checked at a deploy.** A new or tighter total (`per-group` or `global`) is checked from the next take on; a renamed `checked globally` total starts again from zero.
- **Whole seconds.** The clock and every timer count whole seconds.
- **Small units.** A unit, with its parts, lists, carried facts and the trace notes of its last step, holds at most 4096 bytes. An input that would make one larger is refused before anything changes; an answer that would is a step fault, kept in the journal, and the unit keeps its state.
- **HTTP only.** The service speaks plain HTTP/1.1, one request per connection, with a `Content-Length` body, and leaves TLS to the gateway in front of it. Adapters are reached over HTTP through `curl`.
- **One request at a time.** Inputs, reads, timers, adapter posts and model asks take turns, so a slow adapter holds up the service for as long as its post takes (at most 10 seconds), and a slow model for its `within`. Models are asked after the input's step, so the input's own answer does not wait for them.
- **Build the image yourself.** The `Dockerfile` (chapter 22) is not published to a registry, and is built for the machine that builds it.
- **No `suture test`.** Tests run through `cargo test` and `suture run`.
- **Alerts** appear only in the trace and in `suture explain`; they are not sent anywhere.

## Security

- **Callers are named, not authenticated.** The gate checks roles and conditions. A scenario says who the caller is, and a served program trusts the `Suture-Caller` header, which the gateway must set after it has authenticated the caller; beyond localhost, the gateway proves itself with `SUTURE_GATEWAY_TOKEN`. Operator requests need one shared token.
- **Media has one quota.** The media store keeps at most 1 GiB in all, whoever sent it; there is no quota per caller.
- **The secret store is a file.** `secrets.json` is protected by file permissions only; it is not encrypted, and there is no key management service behind it (chapter 19).
- **Personal values are masked by name.** A value is masked when any kind marks an attribute of that name personal, so masking errs on the side of hiding. A personal value is not erased on request: the log keeps it.

## Language

| Not yet | Instead, for now |
|---|---|
| changing a reference, a set, a formula, `created`, a part's attribute, or an attribute a pool key reads | model the change as a new entity |
| refreshing carried facts that a shared entity's own reactions or answers change | a rule that reads such a fact from another unit is a compile error; carry it with a formula or change it with `admit change` |
| events to one of several parts of a kind (a part per list element) | admit the event on the root and react there |
| a rule that reads the total of a unit it refers to (`the number of its unit's Items`) | state the rule about that unit: `the number of each Unit's Items … is at most …` |
| refusing a new entity, or an answer, that would overfill a filtered total | its contribution waits for room |
| ordering within a relation (`the newest Item`) | number them with a formula: `seq: Count = the number of its unit's Items` |
| dates in a program (`opens: Time = 2026-03-01`), and conditions that read the clock | count from `created`: `its created + 30 days`; scenarios may write dates |
| multiplying two amounts, except as a share `a * b / c` | a Count factor, or a share |
| negative counts and durations | count the other way, or subtract |
| sets of values (`set of Text`) | a set of a small kind (`set of Zone`) |
| references, sets or lists as an event's data | pass the name as Text, or admit a new entity |
| `attempts` per part kind | one module-wide setting |
| closest patterns for `define`, `desired` and `when` | they get the phrasing table; rules get closest patterns too |
| sourcing an attribute of a kind other units refer to or pick with `uses` | source it on the unit that reads it, or carry it with a formula |
| a source whose answer carries more than one value | one attribute per source |
| sourcing a `Bool` state | source a Text or Count and compare it in the rule |
| a formula, a `becomes`, or a total's filter that stays fresh over a sourced fact | read the sourced fact in the rule itself; a formula may read a value read once, with no `good for` |
| prefetching | facts are fetched when a step waits on them |
| keys from anywhere but an environment variable | `models.json` names the variable with `key_env` |
| a model that sees the whole unit | it sees the entity that chooses, with its own facts, and the options with theirs; put the media a model needs on the entity that chooses |
| media in sentences, formulas, keys or `sends` | only decision models see media; a sentence reads a Text or a Count about it, given with the media |
| removing media from the store | media is kept as long as the state directory is |
| sending media to adapters | send a Text that names where the adapter finds it |
| two `decide`s with a `whose` at the same rank, even when their conditions exclude each other | give one of them a nearer scope or a choice point |

## By design, never

Some things are left out on purpose, and will stay out:

- **Control flow.** No `if`, loops, calls or explicit steps.
- **`the caller` in rules.** A rule holds of the state at every moment; the caller exists only while an input is applied. Put the condition on the `admit`, or keep who acted with a reaction.
- **`now` in sentences.** A rule must not change truth while nothing happens. Time enters through deadlines, patience, goals and `created`.
- **A `uses` with no pool key.** It would make every choice read every candidate.
- **Undoing the outside world.** A requested effect always counts, and a timed-out request is never replaced automatically.
- **A model deciding what a program means.** Parsing, checking, read-backs, errors and suggestions are fixed code, and the same program always gets the same result.
