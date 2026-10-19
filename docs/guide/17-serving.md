# 17. Serving a program

So far, every program in this guide ran inside a scenario: the scenario played the callers, the outside services and the clock, and nothing was left once it ended. `suture serve` runs a program as a service. It takes inputs over HTTP, posts requests to adapters, follows the real clock, and keeps a log on disk, so a restart picks up exactly where it stopped.

In a cluster, this is the Suture pod in the [architecture diagram](../architecture.svg). The gateway in front of it authenticates callers, and the adapters beside it talk to the providers.

## Start it

```sh
export SUTURE_OPERATOR_TOKEN=change-me
suture serve examples/tickets --listen 127.0.0.1:7878
```

```text
tickets v1 on http://127.0.0.1:7878, state in examples/tickets/.suture
  replayed 0 trace lines; operator requests need the token
```

From then on, every line of the trace is printed as it happens, as in `suture run`.

| Flag | Default | What it sets |
|---|---|---|
| `--state ⟨dir⟩` | `⟨dir⟩/.suture` | where the program and its log are kept |
| `--listen ⟨addr⟩` | `127.0.0.1:7878` | the address to listen on; port `0` takes any free port |
| `--models` | off | read `models.json` and use its backends (chapter 15) |

## The world

Entities that nobody admits, such as the shows and sessions a box office sets up, or a shop's carriers, are the world. A scenario's `given` creates them. On a service, the operator does, with the token:

```sh
curl -s localhost:7878/world -H "Authorization: Bearer $SUTURE_OPERATOR_TOKEN" \
  -d '{"kind": "Show", "label": "Hamlet", "attrs": {"name": "Hamlet", "price": "40 EUR"}}'
curl -s localhost:7878/world -H "Authorization: Bearer $SUTURE_OPERATOR_TOKEN" \
  -d '{"kind": "Session", "label": "Fri", "attrs": {"show": "Hamlet", "day": "Friday", "seats": 100}}'
curl -s localhost:7878/world -H "Authorization: Bearer $SUTURE_OPERATOR_TOKEN" \
  -d '{"kind": "Fan", "label": "Ada", "attrs": {"name": "Ada"}}'
```

Without `SUTURE_OPERATOR_TOKEN` in its environment, the service refuses every operator request.

## Inputs from callers

A caller's input names the caller in the `Suture-Caller` header, as `⟨role⟩ ⟨name⟩`. The service trusts that header, so only the gateway may set it, after it has authenticated the caller. The gate then decides as in chapter 10:

```sh
curl -s localhost:7878/inputs -H 'Suture-Caller: fan Ada' \
  -d '{"new": {"verb": "books", "kind": "Booking", "label": "B1",
               "attrs": {"fan": "Ada", "show": "Hamlet", "quantity": 2}}}'
```

```json
{"trace": ["gate: admitted: fan Ada books a new Booking B1",
           "B1  placed: cancelled no, fan Ada, quantity 2, show Hamlet, total 80 EUR",
           "B1  needs Payment",
           "B1  needs Ticket",
           "B1  Payment → charge via payments (amount 80 EUR)",
           "      choice: Fri for B1/Ticket#1@Session",
           "Fri granted B1/Ticket#1@Session: 2 quantity taken",
           "B1  Ticket gets Session Fri",
           "B1  Ticket waits: rule early"]}
```

There are three kinds of input, one per form of `admit`:

| Input | Body |
|---|---|
| a new entity | `{"new": {"verb": "books", "kind": "Booking", "label": "B1", "attrs": {…}, "items": {"lines": [{…}]}}}` |
| an event | `{"event": {"verb": "cancels", "target": "B1", "data": {"reason": "ill"}}}`; `data` only when the `admit` declares some |
| a change | `{"change": {"target": "Fri", "attr": "seats", "value": 120}}` |

The answer is 200 with the trace lines the input caused, 409 with `refused` when the gate or a rule refuses it, or 400 when the request itself is wrong (an unknown field, or a value of the wrong type).

### Values

| Type | In JSON |
|---|---|
| `Text` | a string |
| `Bool` | `true` or `false` |
| `Count` | an integer |
| `Money` | a string with the module's currency: `"80 EUR"`, `"12.50 EUR"` |
| `Duration`, `Time` | an integer number of seconds |
| a reference | the entity's label: `"Hamlet"` |
| a set | a list of labels: `["EU", "UK"]` |

## Adapters

Every effect and source names an adapter. `adapters.json`, beside the program, says where each one is reached:

```json
{
  "payments": { "url": "http://payments-adapter.shop.svc/charge", "key_env": "PAYMENTS_TOKEN" },
  "mailer":   { "url": "http://mail-adapter.shop.svc/send" }
}
```

Each new request is posted to its adapter once the input that caused it is in the log:

```json
{"key": "B1/Payment#1", "adapter": "payments", "verb": "charge", "kind": "Payment", "unit": "B1", "sends": {"amount": "80 EUR"}}
```

If the adapter can answer at once, its reply is the answer: `{"outcome": "charged"}`, with `"data": {…}` when the outcome carries some. Any other reply means it will answer later, through `/answers`, with the operator token:

```sh
curl -s localhost:7878/answers -H "Authorization: Bearer $SUTURE_OPERATOR_TOKEN" \
  -d '{"adapter": "payments", "outcome": "charged", "key": "B1/Payment#1"}'
```

An adapter that `adapters.json` does not name can poll `GET /pending` for its requests, and answer the same way. If a post fails, it is tried again every 5 seconds while the request waits, and the effect's deadline decides what happens, exactly as in a scenario: after the deadline, the effect sends again, and after its last send it times out. A request's `key` stays the same across those posts and across restarts, so an adapter that remembers the keys it has done does each piece of work once.

Posts are made one at a time, and the service waits for each, for up to the effect's deadline and never more than 10 seconds. An adapter should therefore answer at once, or reply that it will answer later, rather than do slow work before it replies.

## Reading

| Request | Answers with |
|---|---|
| `GET /units/B1` | the explanation of `suture explain`, and the gate's decisions about B1 |
| `GET /pending` | the requests waiting for an answer |
| `GET /trace?from=40` | the trace from line 40, and the number to ask from next |
| `GET /health` | the module and its version |

Reads need no token. They show units and the requests' data, so, like inputs, they must only be reachable through the gateway.

## The log, and restarting

The state directory holds the compiled program and `log.jsonl`. Every input is a line of the log, and so is every move of the clock and every answer a model gave. An input is written, and synced to disk, after it is applied and before its caller is answered, so whatever a caller was told was accepted or refused is in the log. If the log cannot be written, for instance because the disk is full, the caller gets a 500 and the service takes no more inputs until it restarts, since memory would no longer match the log. A line cut short by a crash was never answered; the restart cuts it off.

On start, the service replays the log. It gives the same units, the same trace and the same waiting requests, and no model is asked again, because the log holds what each one answered. Then it posts the waiting requests again, with their keys. Time that passed while the service was down passes at the first tick, so a deadline that ran out meanwhile fires then.

A changed program under the same version is refused:

```text
examples/tickets/.suture: the state runs tickets v1, and this program differs under the same version; give it a new version to deploy it
```

A higher version is deployed over the live units, once the deploy check finds nothing they would break (chapter 18).

## The clock

The runtime clock counts whole seconds from the moment the log was started. Before an input applies, the clock moves to the current time, and once a second the service fires every timer that has fallen due. A scenario's `and 30 seconds pass` is, on a service, thirty seconds of waiting.
