# 1. Getting started

In this chapter you build, test and question a small ticketing system. Fans book seats for a show, the system charges them, finds a session with room, and mails the ticket. If the card is declined, the booking is cancelled and its seats go back. If a show is sold out, the booking waits until the box office adds seats.

## Install

Suture is a Rust workspace. With Rust 1.85 or later:

```sh
git clone https://github.com/swalla02/suture
cd suture
cargo build --release -p suture-cli
```

The command is then `target/release/suture`. In this guide it is written `suture`; from the repository you can also run it as `cargo run --release -p suture-cli -- …`.

## The program

A program is one or more `.st` files in a directory. Here is the whole ticketing system, `examples/tickets/tickets.st`:

```suture
-- Ticketing: fans book seats for a show; the system charges them, picks a
-- session with room, and mails the ticket. Nothing here says in which order.
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

Read it from the top. Each line states one thing:

- **`role`**: two kinds of caller. A `fan` is a Fan entity; `box` (the box office) is just a name.
- **`kind`**: the things that exist, and their attributes. `total` is a formula, computed once when the booking is made. `cancelled` has a default.
- **`needs`**: while a booking is not cancelled, it has its own Payment and its own Ticket. These are its *parts*.
- **`uses`**: a Ticket is bound to an existing Session of the booking's show, and takes the booking's quantity of seats from it.
- **`effect`**: how the system talks to the outside world. A Payment is charged through the `payments` service, which must answer within 30 seconds with `charged` or `declined`. A Ticket is issued through `mailer`, and is `sent` from the moment it is requested.
- **`define`**: two named states, so the rest of the program can say "paid" and "issued".
- **`rule`**: two things that must always hold. No session gives out more seats than it has, and no ticket goes out before the booking is paid.
- **`desired`**: the goal. Every booking that is not cancelled is paid and issued within a day.
- **`when`**: reactions to events. A declined payment cancels the booking, and so does a cancellation by its fan or the box office.
- **`prefer`**: when several sessions have room, take the one with the most seats.
- **`admit`**: who may do what. Fans book for themselves and cancel their own bookings; the box office cancels any booking, adds shows and sessions, and changes a session's seats.

Notice what is missing. Nothing says "charge first, then issue". The rule `early` says a ticket must not be sent before the booking is paid, so the runtime holds the ticket back until the payment is charged. Nothing says "release the seats when cancelled". A cancelled booking no longer needs a Ticket, so its unsent Ticket is withdrawn and gives back what it took. Nothing says "retry the mailer". The effect's deadline does that.

## Build it

```sh
suture build examples/tickets
```

The build checks the program, writes the compiled program file (`tickets-v1.sut`), and prints what it understood:

```text
tickets v1 → examples/tickets/tickets-v1.sut
  kinds 6 · effects 2 · rules 2 · goals 1

rule seats: the total quantity taken from each Session is at most its seats
  understood as:   for each Session s: taken_quantity(s) ≤ s.seats
  in other words:  each session on its own: the total quantity taken from it is at most its seats
  cost:            per-group

rule early: no Booking has a Ticket that is sent unless it is paid
  understood as:   for each Booking b: exists(s in b.Ticket where s is sent) implies b is paid
  in other words:  no booking ever has a ticket that is sent unless it is paid
  cost:            local

choices:
  Ticket uses a Session: pool key Session's show is its booking's show, up to 8 candidates, ranked by `prefer Session` (1 criterion)

roles: fan (a Fan), box
admitted:
  book a new Booking by fan where the Booking's fan is the caller
  …
watches:
  Session's seats, changed by box: carried by Tickets bound to a Session; each change is checked there first, then refreshed
```

Each rule gets a *read-back*: the formula the compiler built from your sentence, a paraphrase generated back from that formula, and the cost of checking it. If the paraphrase is not what you meant, the sentence does not say what you meant. Chapter 7 explains the read-back and the cost tiers.

## Test it

A program is tested with *scenarios*, written in the same controlled English. A scenario sets up a world (`given`), sends inputs (`when`), plays the outside services (`answers`), moves the clock (`pass`), and checks the result (`expect`). Here is the happy path, from `examples/tickets/tickets.scenario`:

```scenario
scenario "a fan books two seats":
  given  a Fan Ada with name Ada
  and    a Show Hamlet with name Hamlet and price 40 EUR
  and    a Session Fri with show Hamlet, day Friday and seats 100
  when   fan Ada books a new Booking B1 with fan Ada, show Hamlet and quantity 2
  and    payments answers charged
  and    mailer answers issued
  expect B1 is paid
  expect B1 is issued
  expect B1's total is 80 EUR
  expect the total quantity taken from Fri is 2
```

Run every scenario in the file:

```sh
suture run examples/tickets --scenario examples/tickets/tickets.scenario
```

Each scenario prints a trace of every step the runtime took:

```text
scenario "a fan books two seats"
  Ada placed: name "Ada"
  Hamlet placed: name "Hamlet", price 40 EUR
  Fri placed: day "Friday", seats 100, show Hamlet
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
  ✓ expect B1 is issued
  ✓ expect B1's total is 80 EUR
  ✓ expect the total quantity taken from Fri is 2
  passed
```

Read the trace once carefully, because it shows the whole idea:

1. The booking passes the gate: the `admit` for fans holds, since B1's fan is the caller.
2. B1 is created with its formula: 2 × 40 EUR = 80 EUR.
3. It needs a Payment and a Ticket, so both are created. The Payment can go out at once, so it is requested from `payments`.
4. The Ticket must be bound to a Session first. Fri is the only candidate; it takes 2 seats and grants them.
5. The Ticket could now go out, but sending it would break rule `early`, because B1 is not paid. So it **waits**, and the trace names the rule.
6. `payments` answers `charged`. B1 is now paid, so the waiting Ticket is tried again, and this time it goes out.
7. `mailer` answers `issued`, and the goal is met.

Nobody wrote steps 3 to 6. They follow from the needs, the binding, the rule and the effects.

## When things go wrong

The other scenarios in the file show what the same eight declarations do when the world misbehaves:

```scenario
scenario "a declined card cancels the booking and frees the seats":
  given  a Fan Ada with name Ada
  and    a Show Hamlet with name Hamlet and price 40 EUR
  and    a Session Fri with show Hamlet, day Friday and seats 100
  when   fan Ada books a new Booking B1 with fan Ada, show Hamlet and quantity 2
  and    payments answers declined
  expect B1 is cancelled
  expect B1 has no Ticket
  expect the total quantity taken from Fri is 0

scenario "a sold-out show waits for seats":
  given  a Fan Ada with name Ada
  and    a Show Hamlet with name Hamlet and price 40 EUR
  and    a Session Fri with show Hamlet, day Friday and seats 1
  when   fan Ada books a new Booking B1 with fan Ada, show Hamlet and quantity 2
  and    payments answers charged
  expect B1 is not issued
  when   box Bo changes Fri's seats to 10
  and    mailer answers issued
  expect B1 is issued

scenario "a fan cannot book for someone else":
  given  a Fan Ada with name Ada
  and    a Fan Eve with name Eve
  and    a Show Hamlet with name Hamlet and price 40 EUR
  when   fan Eve books a new Booking B1 with fan Ada, show Hamlet and quantity 1
         is refused at the gate

scenario "a mailer that never answers raises an alert":
  given  a Fan Ada with name Ada
  and    a Show Hamlet with name Hamlet and price 40 EUR
  and    a Session Fri with show Hamlet, day Friday and seats 100
  when   fan Ada books a new Booking B1 with fan Ada, show Hamlet and quantity 1
  and    payments answers charged
  and    1 day pass
  expect B1's Ticket is timed out
  expect an alert for B1
```

- **Declined card.** The reaction cancels the booking. The booking no longer needs a Ticket, so its unsent Ticket is withdrawn, and the 2 seats go back to Fri.
- **Sold out.** Fri has 1 seat, so taking 2 would break rule `seats`. Fri refuses, and the Ticket waits on the pool of Hamlet's sessions. When the box office raises the seats to 10, the waiting Ticket is woken, takes its seats and goes out.
- **Wrong caller.** Eve tries to book for Ada. The admit's condition fails, and the input changes nothing.
- **Silent mailer.** The request is sent three times, 10 minutes apart, with the same idempotency key. Then the Ticket is `timed out`. After a day the goal is late, and an alert is raised that says why.

## Ask why

`suture explain` runs the scenarios and then explains one entity at the end of each scenario that has it:

```sh
suture explain examples/tickets --scenario examples/tickets/tickets.scenario B1
```

For the silent mailer:

```text
  Booking B1 (cancelled no, fan Ada, quantity 1, show Hamlet, total 40 EUR)
    goal late, alert raised: every Booking that is not cancelled is paid and issued (not yet issued)
    Payment: charged — amount 40 EUR
    Ticket: failed (timed out)
  gate: admitted: fan Ada books a new Booking B1
```

The explanation comes from the same state the runtime uses. It is not a log someone remembered to write.

## What next

- Chapter 2 explains the model behind what you just saw: units, steps, refusal and the journal.
- To change the program, edit `tickets.st` and run `suture build` again. Try writing `rule early: no Booking may have a Ticket that is sent unless it is paid` and see what the compiler suggests (chapter 12).
