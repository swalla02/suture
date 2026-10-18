# The Suture guide

Suture is a language for writing systems as statements, not steps. You write what exists, what each thing needs, the rules that must always hold, the goals to reach and by when, what the outside world does, and who may do what. You never write the order of steps, status machines, retries, locks, compensations or crash recovery. The runtime does that work, and it can say at any moment why something is not done yet.

This guide teaches the language as it runs today (prototype v0, slices 1 to 11). Every `suture` program block and every scenario in it is compiled and run by the test suite (`runtime/tests/guide.rs`), so if an example here stops working, the build fails.

## How to read it

If you are new, read chapters 1 and 2 in order: they build a small working system and explain the ideas behind it. Chapters 3 to 12 cover one construct each and can be read in any order. Chapter 13 collects modelling advice, chapter 14 covers facts fetched from outside, chapter 15 decision models, chapter 16 says what v0 does not do yet, and chapter 17 runs a program as a service.

| | Chapter | What you learn |
|---|---|---|
| 1 | [Getting started](01-getting-started.md) | build, run and explain a ticketing system in ten minutes |
| 2 | [How Suture thinks](02-how-suture-thinks.md) | units, steps, refusal, waiting, the journal; what replaces the code you would write |
| 3 | [Kinds and values](03-kinds-and-values.md) | `module`, `kind`, types, defaults, formulas, lists, references, sets |
| 4 | [Needs, parts and slots](04-needs-parts-and-slots.md) | `needs … while …`, a part's life, `is … or …`, `prefer` and patience |
| 5 | [Effects and reactions](05-effects-and-reactions.md) | talking to the outside world, deadlines, time-outs, `when …` |
| 6 | [Sentences](06-sentences.md) | noun phrases, conditions and numbers: the grammar every sentence shares |
| 7 | [Rules](07-rules.md) | the rule forms, read-backs, cost tiers, refusal |
| 8 | [Definitions and goals](08-definitions-and-goals.md) | `define`, `desired within …`, alerts |
| 9 | [Shared entities and choices](09-shared-entities-and-choices.md) | `uses`, pools, `taking`, `prefer lowest …`, totals across units |
| 10 | [Permissions](10-permissions.md) | roles, `admit`, `the caller`, events, changes, watches |
| 11 | [Scenarios](11-scenarios.md) | testing a program in its own language |
| 12 | [The `suture` command](12-the-suture-command.md) | build output, traces, explanations, errors and suggestions |
| 13 | [Modelling patterns](13-modelling-patterns.md) | how to turn a requirement into declarations, and the traps |
| 14 | [Sourcing](14-sourcing.md) | `source … via …`, lifespans, `with … at most … old`, recovery |
| 15 | [Decision models](15-decision-models.md) | `decide … with model`: the levels, slots and sources, deadlines, Jev and custom backends |
| 16 | [Limits of v0](16-limits-of-v0.md) | what is designed but not built yet |
| 17 | [Serving a program](17-serving.md) | `suture serve`: inputs over HTTP, adapters, the log and restarting, the real clock |
| | [Quick reference](reference.md) | every form on one page |

## Three promises

Suture makes three promises, and the rest of the language exists to keep them:

- **Consistent:** no rule is ever broken. A step that would break one never happens.
- **Observable:** anything that is not done can say why: which need is missing, which rule holds it back, which answer it waits for.
- **Repeatable:** every input, answer, timer and choice is journaled, and replaying the journal gives the same state.

And one non-promise: Suture uses no AI to decide what your program means. Parsing, checks, read-backs, errors and suggestions follow fixed rules, so the same program always gets the same result. Decision models can make the choices a program leaves open (chapter 15), but only by picking among options that are already allowed.

## Examples in the repository

- `examples/tickets/`: the ticketing system of chapter 1.
- `examples/shop/`: an online shop with stock, carriers, payments, refunds, cancellation, lost parcels and French customs forms.
- `examples/workspaces/`: provisioning team workspaces on clusters, with accounts, repositories, billing and storage.
- `examples/credit/`: the credit check of chapter 14, with a bureau that answers late, wrong or not at all.
