# 4. Needs, parts and slots

A need says what each entity must have. The runtime creates what is needed, keeps it while it is needed, replaces it when it fails, and withdraws it when it is no longer needed. This chapter uses a visa office: each application needs a fee and a review, urgent applications need an interview, and the passport comes back by courier or is collected.

## Needs

```suture
module visas version 1, currency EUR

kind Applicant   { name: Text }
kind Application { applicant: Applicant, urgent: Bool = no, withdrawn: Bool = no }
kind Fee         { application: Application, amount: Money = 60 EUR }
kind Review      { application: Application }
kind Interview   { review: Review }

Application needs Fee, Review while it is not withdrawn
Review      needs Interview while the Application is urgent

effect pay Fee via payments within 1 hour
  sends    its amount
  reports  paid | fails with declined
effect hold Interview via calendar within 7 days
  sends    the review's application's applicant's name
  reports  held | fails with missed
```

`A needs B, C [while ⟨condition⟩]` gives **each** A its own B and its own C. These are A's **parts**. A part can have needs of its own, so structure can go as deep as the domain does: here a Review needs an Interview, but only for urgent applications.

- **Ownership.** The part kind must have exactly one attribute of the owner's kind (`application: Application`). The runtime sets it; you never give it.
- **One unit.** A root entity and all its parts, at every depth, form one unit (chapter 2). Every rule about them is checked in one step.
- **Conditional depth.** Structure appears only where its condition holds. An application that is not urgent never grows an Interview, and nothing in the program mentions it.

## A part's life

A need is **active** while its condition holds, or always if it has none.

1. **Created.** When a need becomes active, its part is created, and its own needs follow.
2. **Ready.** A part is ready when it exists, its own active needs are done, and its bindings are granted (chapter 9).
3. **Requested.** If the part's kind has an effect, the request goes out once the part is ready and the request step passes every rule. If a rule refuses it, the part waits (chapter 2).
4. **Done or failed.** The effect's success outcome makes the part `done`; a failure outcome makes it `failed`. A part whose kind has **no effect** is done as soon as all its active needs are done. A Review with no Interview to wait for is done at once.

Parts and slots have the built-in states `done` and `failed`, so a sentence can say `its Review is done`.

### Withdrawal

When a need's condition turns false, an **unfinished** part is withdrawn and gives back everything it took (chapter 9). A part that is already done stays done. If the application above is withdrawn before the fee is paid, the Fee and the Review disappear.

Withdrawal is how Suture does compensation. You never write "if cancelled, release the stock". The cancelled entity stops needing its parts, and the parts give back what they hold.

Only a part that has not reached the outside world can be withdrawn. Once a request has been sent, the part stays and counts, whatever happens next (chapter 2).

### Failure and replacement

A part that fails with an explicit failure outcome, such as `missed`, is **replaced**: a new attempt is created, and the failed one stays in the history. This happens up to the module's `attempts` setting (3 by default). After the last attempt the need fails, and its owner's goal explains why.

A part that ends **`timed out`** is never replaced (chapter 5): the request may still have happened.

```scenario
scenario "an urgent application gets an interview, and a missed one is held again":
  given  an Applicant Ada with name Ada
  when   a new Application A1 with applicant Ada and urgent yes
  and    payments answers paid
  and    calendar answers missed
  and    calendar answers held
  expect A1's Fee is paid
  expect A1's Review is done

scenario "a normal application needs no interview":
  given  an Applicant Ada with name Ada
  when   a new Application A1 with applicant Ada
  and    payments answers paid
  expect A1's Review is done
```

## Slots: one of several alternatives

Sometimes an entity needs one of several things: a delivery by shipment or by refund, storage as a bucket or a volume. A **slot** names that choice:

```suture
kind Courier    { application: Application }
kind Collection { application: Application }

Application needs Return while it is not withdrawn
Return      is    Courier or Collection

effect send Courier via couriers within 2 days
  sends    the application's applicant's name
  marks    sent
  reports  delivered | fails with lost
effect notify Collection via mailer within 1 hour
  sends    the application's applicant's name
  reports  notified

prefer Return: Courier, then Collection after 3 days
```

- `⟨Slot⟩ is ⟨Kind⟩ or ⟨Kind⟩[ or …]` declares the slot. Its name is capitalised and is not a kind.
- Each alternative is an ordinary part kind that names the slot's owner, and the owner does not also need it directly.
- A slot is needed like a part: `Application needs Return`.
- A slot is `done` when its current alternative is done, and `failed` when it has failed on its last alternative.
- In sentences, `its Return` reads the current alternative's state. You cannot go further through a slot (`its Return's …`); name the alternative instead (`its Courier's …`).

### Which alternative, and when to move on

`prefer ⟨Slot⟩: ⟨Kind⟩, then ⟨Kind⟩ [after ⟨duration⟩]…` sets the order. The slot starts with the first alternative and **moves on** to the next when the current one:

1. has failed on its last attempt, or timed out;
2. has waited longer than its **patience** (`then Collection after 3 days`: wait 3 days on the courier before giving up on it) without going out;
3. is refused by a rule on facts that can never change (chapter 7), in which case it moves on at once.

Patience never abandons a part that reached the outside world: a courier already sent stays, however long it takes. An untouched part is withdrawn when the slot moves on. If moving on would itself break a rule, the slot stays where it is, and the trace says `patience held: rule …`.

With no `prefer`, a slot uses declaration order, and `suture build` warns.

```scenario
scenario "three lost couriers end in a collection":
  given  an Applicant Ada with name Ada
  when   a new Application A1 with applicant Ada
  and    payments answers paid
  and    couriers answers lost
  and    couriers answers lost
  and    couriers answers lost
  and    mailer answers notified
  expect A1's Collection is notified
  expect the number of A1's Couriers is 3
```

The three lost couriers all still count (`the number of A1's Couriers is 3`): each one reached the outside world.

## Withdrawal in action

Two more statements: the passport must not leave before the fee is paid, and a declined fee withdraws the application.

```suture
rule prepaid: no Application has a Courier that is sent unless its Fee is paid

when a Fee is declined: its Application is withdrawn
```

Now the Courier is created at once but waits on rule `prepaid`. When the fee is declined, the reaction sets `withdrawn`, the need for a Return is no longer active, and the waiting Courier, which never went out, is withdrawn:

```scenario
scenario "a declined fee withdraws the application and its waiting courier":
  given  an Applicant Ada with name Ada
  when   a new Application A1 with applicant Ada
  expect A1 has a Courier
  when   payments answers declined
  expect A1 is withdrawn
  expect A1 has no Courier
```

## The `attempts` setting

`module visas version 1, currency EUR, attempts 5` makes every part try up to 5 times and every request send up to 5 times. It is one setting for the whole module in v0.
