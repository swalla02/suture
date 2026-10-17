# 8. Definitions and goals

A **definition** names a state, so the rest of the program can use one word for a condition. A **goal** says what must eventually be true of each entity, and by when. Together they replace status columns and the jobs that look for stuck work.

The examples use onboarding a new hire:

```suture
module onboarding version 1, currency EUR

kind Hire    { name: Text, remote: Bool = no, left: Bool = no }
kind Laptop  { hire: Hire }
kind Badge   { hire: Hire }
kind Account { hire: Hire }

Hire needs Laptop, Account while it is not left
Hire needs Badge while it is not remote and it is not left

effect ship Laptop via itdesk within 2 days
  sends    the hire's name
  reports  delivered | fails with lost
effect print Badge via security within 1 day
  sends    the hire's name
  reports  printed
effect open Account via identity within 1 hour
  sends    the hire's name
  reports  opened

define: a Hire is equipped when its Laptop is delivered and its Account is opened
define: a Hire is admitted when it is remote or its Badge is printed

desired within 3 days: every Hire that is not left is equipped and admitted
```

## Definitions

```text
define: a ⟨Kind⟩ is ⟨state⟩ when ⟨condition⟩
```

- The state holds exactly when the condition holds. It is never stored, and it cannot be set: it is always computed from the facts.
- A defined state is used anywhere a state is: in rules (`no equipped Hire …`), goals, `while` conditions, other definitions, and scenarios (`expect H1 is equipped`).
- A definition is how to name a mixed condition, since one sentence cannot mix `and` and `or` (chapter 6). `admitted` above is an `or`, used inside the goal's `and`.
- Each state name has one meaning per kind: a definition cannot reuse the name of an attribute, a mark or an outcome of the same kind.

A definition is the honest version of a status column. A column says what someone last wrote; a definition says what is true now.

## Goals

```text
desired [within ⟨duration⟩]: every ⟨subject⟩ is ⟨state⟩[ and ⟨state⟩…]
```

- For every entity that matches the subject, the states must **eventually** all hold.
- The deadline runs from the moment the entity **first matched** the subject. Here that is when the hire was created, since a new hire is not `left`.
- A goal is about a root kind. In v0 a part cannot be a goal's subject; state the goal on its owner (`every Hire is equipped`, with `define: a Hire is equipped when its Laptop is delivered`).
- A goal without `within` has no deadline: it is still checked, and `explain` reports whether it is met.

A goal is not an instruction. It does not make anything happen: needs do that. It states what success looks like, so the runtime can tell you when it has not happened in time.

## Alerts

When a goal's deadline passes and the goal is not met, the entity raises an **alert**. The alert carries an explanation: the goal's read-back and, for each state that does not hold, the part that is missing, waiting (with the rule that holds it), failed, or timed out.

- A goal alerts at most once per entity.
- If it is met later, the trace notes it as met late.
- In v0, alerts go to the trace and to `suture explain`.

```scenario
scenario "a remote hire needs no badge":
  given  a Hire Ana with name Ana and remote yes
  when   itdesk answers delivered
  and    identity answers opened
  expect Ana is equipped
  expect Ana is admitted
  expect Ana has no Badge
  expect no alert for Ana

scenario "a lost laptop is replaced, and the goal is met late":
  given  a Hire Bo with name Bo
  when   security answers printed
  and    identity answers opened
  and    itdesk answers lost
  and    4 days pass
  expect an alert for Bo
  expect Bo is not equipped
  when   itdesk answers delivered
  expect Bo is equipped
```

Note that the first scenario makes the hire with `given`. A `given` entity is part of the scenario's world, but its needs and goals still run like those of any other entity.

## Asking why

`suture explain` shows each goal of an entity and why it is not met yet. For Bo, at the end of the first part of that scenario:

```text
  Hire Bo (left no, name "Bo", remote no)
    goal late, alert raised: every Hire that is not left is equipped and admitted (not yet equipped)
    Laptop: failed (lost)
    Account: opened
    Badge: printed
    Laptop (attempt 2): requested from it, send 3 of 3, answer due in 2 days
```

And the trace shows the alert with its reason, at the moment the deadline passed:

```text
  Bo  alert: not met within 3 days: every Hire that is not left is equipped and admitted
  Bo    because Laptop has no answer yet
```

Every line comes from the state the runtime uses to decide what to do next, so the explanation cannot drift from the behaviour.
