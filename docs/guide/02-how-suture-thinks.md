# 2. How Suture thinks

Most languages describe a computation: do this, then that, and if this, do that. Suture describes a situation: what exists, what it needs, what must hold, and what should eventually be true. The runtime works out the steps. This chapter explains how, so that what the runtime does is never a surprise.

## Declare, never direct

A Suture program has only statements. Every line is one of these:

| Statement | Says | Example |
|---|---|---|
| a fact | what exists | `kind Booking { fan: Fan, quantity: Count }` |
| a need | what each thing must have | `Booking needs Payment, Ticket while it is not cancelled` |
| a rule | what must always hold | `rule seats: the total quantity taken from each Session is at most its seats` |
| a goal | what must eventually hold, and by when | `desired within 1 day: every Booking that is not cancelled is paid and issued` |
| an effect | what the outside world can do | `effect charge Payment via payments within 30 seconds …` |
| a reaction | what an event changes | `when a Payment is declined: its Booking is cancelled` |
| a preference | how to choose | `prefer Session: highest seats` |
| a permission | who may do what | `admit cancel Booking by box` |

No statement says what to do next. There is no `if`, no loop, no call, no status field that you move along by hand. That is the whole design: if a program cannot say "do X", it cannot do X in the wrong order, forget to do X on one path, or do X twice.

## What replaces the code you would write

| You would normally write | In Suture you write | Chapter |
|---|---|---|
| a status column and a state machine | `define: a Booking is paid when its Payment is charged` | 8 |
| "create the payment, then the shipment" | `Booking needs Payment, Ticket` | 4 |
| a check before every write | one `rule` | 7 |
| a job queue with retries and back-off | `effect … within 30 seconds` (re-sent with the same key) | 5 |
| row locks or a reservation table | `uses a Session …, taking …` and a rule on the total taken | 9 |
| compensation ("release the stock if the payment fails") | nothing: a withdrawn part gives back what it took | 4 |
| a cron job that looks for stuck orders | `desired within 3 days: …` | 8 |
| fallbacks ("if no stock in 2 days, refund") | a slot: `Delivery is Shipment or Refund` with patience | 4 |
| authorization checks in every handler | `admit … by … where …` | 10 |
| tests with mocks | scenarios that answer for each service | 11 |
| logs to find out why an order is stuck | `suture explain` | 12 |

## Units

At run time, entities are grouped into **units**. A unit is a root entity together with everything it owns through `needs`: a Booking, its Payment and its Ticket form one unit. Entities that many units share, such as a Session, a Show or a Fan, are units of their own.

A unit has one mailbox and changes in **steps**. Every event for a unit is exactly one step:

- its creation;
- an answer from an outside service;
- a timer firing (a deadline or a patience);
- an admitted event or change from a caller;
- a message from another unit (a seat granted, a release).

A step is checked against every rule before it is kept. If the new state would break a rule, the step is refused and the unit stays as it was.

## Refusal and waiting

Rules are not checked after the fact, and they are not checked by code you remember to call. They are enforced by **refusal**: a step that would break a rule never happens.

What happens to the thing that wanted that step depends on where the step came from:

- **Something the runtime wants to do** (send a request, create a part, fill a slot) **waits.** It watches the facts the rule read, and it is tried again when one of them changes. Nothing polls. In chapter 1 the Ticket waited on rule `early` and went out the moment the payment was charged.
- **An answer or a caller's input** that would break a rule is refused, and the sender is told which rule refused it. A refused answer leaves its request open, so a corrected answer can still arrive.
- **A time-out** is a fact and is always recorded. Only reactions to it can be held back.

## Facts never come undone

The outside world is never assumed undone. Once a request has been sent, its entity keeps counting toward every rule and total, whatever happens next: a parcel that was lost still took its stock, and a refund that was requested still counts toward the refund cap. A request that times out is re-sent with the same idempotency key, and when it finally times out it is never replaced by a new one automatically, because the first one might still have happened.

This is why the runtime can be trusted with money and stock. It never "rolls back" something that might have reached the world.

## Choices

Some statements leave a choice open: which alternative fills a slot, which session a ticket uses. Every open choice is resolved through one function, which receives the allowed options and returns one of them. In v0 that function is your `prefer`, and ties are broken by creation order, so choices are deterministic. The choice is journaled.

Because every pick is still checked against the rules, the choice function cannot break a rule however it picks. This is where a decision model plugs in (chapter 15): it can only pick among options that are already allowed.

## The journal

Every input, answer, timer firing and choice is written to a journal. The clock is an input like any other: units never read a clock, the host tells them the time. So replaying a journal gives exactly the same state, step for step. Two live runs that receive the same inputs in a different order may make different choices, and both are valid.

## Cost

Every mechanism has a work bound that does not grow with the size of the system. Checking a rule reads one unit, or one running total. A choice looks at a bounded number of candidates from one pool. Moving the clock does work only for the timers that are due. The compiler tells you the cost of each rule (chapter 7), and refuses rules that would have to look at everything, unless you mark them `checked globally`.

## The toolchain is deterministic

The compiler never guesses. Every sentence matches exactly one pattern with exactly one meaning, and the read-back shows you that meaning. When a sentence matches no pattern, the error names the closest pattern and, where it can, a corrected sentence that it has compiled to make sure it works (chapter 12). There is no model behind any of this: the same program always gets the same errors.
