# Suture

Suture is a declarative language for building software that runs business processes: orders, bookings, provisioning, anything with steps, outside services, shared resources and rules that must never be broken.

You do not write the steps. You declare what exists, what each thing needs, the rules that must always hold and the goals to reach. The runtime works out the order, waits when a rule says so, retries what times out, gives back what a cancelled step took, and records every step so the same inputs always give the same result.

Suture is a v0 prototype: a Rust compiler and runtime, a guide whose every example is tested, and five example systems. It is not production software yet.

## Where Suture fits

Suture is not infrastructure, and it does not replace your databases or providers. It replaces the code in the middle: the services written to coordinate steps, call providers, retry, compensate, check permissions, keep a state machine per entity and log what happened. Today, that code is most of a backend team's work.

![Suture runs as pods in a Kubernetes cluster that Terraform provisions. Its runtime pods sit in the middle and link every other pod: platform services, your services and adapters, the ingress, and decision models served by vLLM or SGLang on a GPU node pool](docs/architecture.svg)

Everything stands on the same base: Terraform provisions the cluster and its CPU and GPU node pools, and Kubernetes schedules every pod. Suture's runtime pods, with their journal on a persistent volume, are the one place that links all the others:

| Around Suture | Runs as | Linked by |
|---|---|---|
| callers: apps, staff tools, partner systems | outside, through the ingress | `admit`: only admitted inputs get in |
| platform services: PostgreSQL, Redis, Vault, Prometheus | pods | `source` for facts; API keys from Vault as environment variables; metrics and alerts out |
| your services, and adapters to payment, mail and carrier providers | pods, which call out | `effect`, with a deadline and the outcomes it may report |
| decision models | vLLM or SGLang pods on GPU nodes, or a hosted API | `decide`: pick one of the options the rules allow |

Suture links services; it does not prompt a model. There are no prompts and no generated code. A model only ever picks among options the rules already allow, every pick is checked and journaled, and with every model off, `prefer` decides and the system still runs. The compute engines are the motor for those picks, not the logic.

In v0, `suture serve` runs a program as one process: inputs over HTTP, requests posted to adapters, the real clock, and a log on disk that a restart replays; `suture deploy` checks a new version against the live units before it is started; `secret` and `personal` fields reach only the adapters granted them, and `suture build` writes a manifest of what each adapter receives. Several pods can serve one state directory, taking turns on its log, each posting the requests of the units it owns, and taking over those of a pod that stops (guide chapter 20).

## A program

Here is a whole ticketing system ([`examples/tickets/tickets.st`](examples/tickets/tickets.st)). Fans book seats, the system charges them, finds a session with room and mails the ticket:

```text
module tickets version 1, currency EUR

role fan is a Fan
role box

kind Fan     { name: Text }
kind Show    { name: Text, price: Money }
kind Session { show: Show, day: Text, seats: Count }
kind Booking { fan: Fan, show: Show, quantity: Count,
               total: Money = show's price * quantity,
               cancelled: Bool = no }
kind Payment { booking: Booking, amount: Money = booking's total }
kind Ticket  { booking: Booking }

Booking needs Payment, Ticket while it is not cancelled
Ticket  uses  a Session where the Session's show is the Booking's show,
              taking the Booking's quantity

effect charge Payment via payments within 30 seconds
  sends    its amount
  reports  charged | fails with declined
effect issue Ticket via mailer within 10 minutes
  sends    the booking's fan's name
  marks    sent
  reports  issued

define: a Booking is paid when its Payment is charged
define: a Booking is issued when its Ticket is issued

rule seats: the total quantity taken from each Session is at most its seats
rule early: no Booking has a Ticket that is sent unless it is paid

desired within 1 day: every Booking that is not cancelled is paid and issued

when a Payment is declined: its Booking is cancelled
when a fan or box cancels a Booking: the Booking is cancelled

prefer Session: highest seats

admit book a new Booking by fan where the Booking's fan is the caller
admit cancel Booking by fan where the Booking's fan is the caller
admit cancel Booking by box
admit add a new Show or Session by box
admit change Session's seats by box
```

Nothing says "charge first, then issue": rule `early` holds the ticket back until the payment is charged. Nothing says "release the seats on cancellation": a cancelled booking no longer needs its ticket, so the unsent ticket is withdrawn and gives its seats back. Nothing says "retry the mailer": the effect's deadline does that. There is no `if`, no loop and no call order anywhere.

## What the compiler tells you

Rules are written in controlled English: a fixed set of sentence patterns, each with exactly one meaning, read by an ordinary parser. For every rule, `suture build` prints the formula it understood, a paraphrase, and what checking it costs, so you can see that it read what you meant:

```text
rule seats: the total quantity taken from each Session is at most its seats
  understood as:   for each Session s: taken_quantity(s) ≤ s.seats
  in other words:  each session on its own: the total quantity taken from it is at most its seats
  cost:            per-group

rule early: no Booking has a Ticket that is sent unless it is paid
  understood as:   for each Booking b: exists(t in b.Ticket where t is sent) implies b is paid
  in other words:  no booking ever has a ticket that is sent unless it is paid
  cost:            local
```

It also lists every choice the program leaves open, such as which session a ticket takes, with who decides it. A sentence it cannot read gets the closest pattern it knows, with the line.

## What the runtime shows you

Scenarios are written in the same style, and `suture run` prints every step the runtime took:

```text
scenario "a fan books two seats"
  gate: admitted: fan Ada books a new Booking B1
  B1  placed: cancelled no, fan Ada, quantity 2, show Hamlet, total 80 EUR
  B1  needs Payment
  B1  needs Ticket
  B1  Payment → charge via payments (amount 80 EUR)
        choice: Fri for B1/Ticket#1@Session
  Fri granted B1/Ticket#1@Session: 2 quantity taken
  B1  Ticket gets Session Fri
  B1  Ticket waits: rule early
        payments: charged   (B1/Payment#1)
  B1  Payment charged
  B1  Ticket → issue via mailer (name "Ada")
        mailer: issued   (B1/Ticket#1)
  B1  Ticket issued
  B1  goal met: every Booking that is not cancelled is paid and issued
  ✓ expect B1 is paid
  passed
```

`suture explain` says where one entity stands and why: what it waits on, which rule holds it, and what is due when.

## What it guarantees

- **Rules hold.** Every change is checked against the rules before it happens. A step that would break one waits, or is refused, and the trace names the rule.
- **Same inputs, same result.** Every input, answer, timer and choice is journaled. Replaying a journal rebuilds exactly the same state, and a journal does not replay under a changed program.
- **Only admitted inputs.** Callers have roles, and each `admit` says who may create, change or cancel what.
- **No AI needed.** Every open choice has a deterministic default (`prefer`). Decision models are optional, and they only ever pick among options the rules already allow.

## The language, construct by construct

| Construct | Says |
|---|---|
| `kind` | what exists, with typed attributes, defaults and formulas |
| `needs`, slots | what each thing needs, under conditions; or one of several alternatives, tried in order with patience |
| `uses … taking` | a binding to a shared entity, such as stock, seats or a carrier, picked from a pool and checked by its rules |
| `effect` | how a step talks to an outside service: what it sends, what it may report, and its deadline |
| `rule` | what must always hold, per entity, per group, or per period |
| `define`, `desired` | named states, and the goals to reach within a deadline, with alerts when they are missed |
| `when` | reactions to outcomes and admitted events |
| `admit`, `role` | who may do what |
| `source` | facts fetched from outside when a rule needs them, with how long they are trusted and what to do when the fetch fails |
| `decide … with model` | which decision model, if any, makes an open choice, at app, workflow, step, choice or condition level |

## Decision models

A program is full of small choices the rules leave open: which carrier, which stock, which delivery method, which credit bureau to ask. `prefer` makes them by fixed criteria. A `decide` can hand a choice to a decision model instead:

```text
decide Carrier with model "carriers@1"
decide Return with model "routing@1" within 300 milliseconds
decide Carrier for Orders whose total is at least 1000 EUR with model "careful@1"
```

A model is given the options, best first under `prefer`, and picks one by name. Its pick is checked by the rules like any other. If it declines, is late, is unsure or fails, `prefer` picks, and the trace says why. Models are pinned by name and version in the compiled program, and every pick is journaled, so replay never calls a model.

By default, models are reached through the [Jev](https://docs.typesafe.ai/api) decision API. A custom JSON format, or any Rust backend, adapts other services. Backends are configured in a `models.json` beside the program, and the key is read from an environment variable. See [chapter 15](docs/guide/15-decision-models.md).

## Try it

With Rust 1.85 or later:

```sh
git clone https://github.com/swalla02/suture
cd suture
cargo build --release -p suture-cli

target/release/suture build   examples/tickets
target/release/suture run     examples/tickets --scenario examples/tickets/tickets.scenario
target/release/suture explain examples/shop --scenario examples/shop/orders.scenario O1
target/release/suture serve   examples/tickets --listen 127.0.0.1:7878
```

`suture serve` takes inputs over HTTP and keeps its log in `examples/tickets/.suture`; [chapter 17](docs/guide/17-serving.md) walks through a session with `curl`.

| Example | Shows |
|---|---|
| [`tickets`](examples/tickets) | the tutorial: bookings, payments, shared seats, cancellation |
| [`shop`](examples/shop) | an online shop with stock, carriers and refunds; `france.st` adds customs and excise for some orders only, without touching `shop.st` |
| [`workspaces`](examples/workspaces) | multi-tenant workspace provisioning |
| [`credit`](examples/credit) | a credit check fetched from a bureau, trusted for a day, with a fallback |
| [`returns`](examples/returns) | a decision model picking how a return is handled, with `prefer` behind it |

## Learn it

**[The Suture guide](docs/guide/README.md)** is the place to start: a tutorial, one chapter per construct, modelling patterns, the limits of v0, and a quick reference. Every program, error and scenario in it is compiled and run by the test suite.

The design is in the [language spec](docs/superpowers/specs/2026-10-14-suture-language-design.md). Its appendices B to P describe each prototype slice.

## Inside

```text
src/        the kernel: an interaction-net machine with a journal, replay and explanations
lang/       the compiler: lexer, controlled-English parser, checker, read-backs, suggestions
runtime/    the interpreter and host: units, needs, rules, deadlines, shared entities,
            sources, choice points, model backends, and the service (log, HTTP API,
            adapters, clock), on top of the kernel
cli/        the `suture` command: build, run, explain, serve
docs/       the guide, the spec, the kernel IR (ir-spec.md), and research notes
examples/   the example systems above
```

```sh
cargo test --workspace     # compiler, runtime, guide, examples, replay
```

Each running entity, with everything it owns, is one agent on the kernel. Messages between them, outside answers, timers and model picks all enter through the journal, which is why a run can be replayed and explained step by step. The kernel's design is in [`docs/ir-spec.md`](docs/ir-spec.md). The research notes Suture grew from are in [`docs/design-notes.md`](docs/design-notes.md).
