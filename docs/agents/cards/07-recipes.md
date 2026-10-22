# recipes: from a requirement to the Suture form

Look the requirement up here first. Each line is a form that compiles; the card named after it has the details.

| Requirement | Suture |
|---|---|
| a status that follows from facts | `define: an Order is late when …` (rules) — never a stored status you must keep in sync |
| a status someone sets | a state attribute and a reaction: `when a clerk ships an Order: the Order is shipped` (gate) |
| only role R may do X | `admit X … by R` — anything not admitted is refused (gate) |
| only for their own | `admit … by member where the Loan's member is the caller` (gate) |
| not for themselves | `admit approve Expense by manager where the Expense's employee is not the caller` |
| at most N per owner | `rule ⟨n⟩: the number of each Member's Loans that are open is at most 3` (rules) |
| a sum within a budget | `rule ⟨n⟩: the total amount of each Project's Expenses that are approved is at most its budget` |
| at most one active per thing | `rule ⟨n⟩: the number of each Book's Loans that are open is at most 1` |
| X is blocked while Y | an `admit … where` reading a total: `where the number of the Loan's member's Loans that are late is at most 0` |
| one entity's state depends on another's | read it through the reference in a `define` or rule: `define: a Shipment is held when its order is unpaid` |
| a thing many share, with stock | a pool: `Hold uses a Title where …, taking …` and `rule stock: the total copies taken from each Title is at most its stock` (parts) |
| a call to an outside service | a part with an `effect` (parts) |
| undo work when cancelled | a need `while it is not cancelled`: unfinished parts are withdrawn (parts) |
| try A, else B after a wait | a slot: `Return is Courier or Collection` + `prefer Return: Courier, then Collection after 2 days` |
| a deadline that alerts | `desired within 2 days: every Order that is open is shipped` (time) |
| becomes overdue by itself | a part whose effect times out (time) |
| a value from a document or photo | `source … via model "…"` (models) |
| a choice a model makes | `decide … with model "…"` behind `prefer` (models) |
| a limit per day or month | `rule ⟨n⟩: for each Member and day, the number of their Bookings is at most 2` (time) |
| a value from a fixed set of numbers (30 or 45) | `define: a Talk is standard when its length is 30 or its length is 45` and `admit submit a new Talk by speaker where the Talk is standard` |
| an average at least N | compare totals: `rule good: no Talk is accepted unless the total score of its Reviews is at least the number of its Reviews * 3` |
| a total shown but not limited | `keep the total amount of each Project's Expenses` (rules) |

Before writing, check `suture help limits`: some requirements cannot be written yet, and the card says what to do instead.

The forms above, in one module that compiles:

```suture
module recipes version 1, currency EUR

role employee is a Employee
role manager is a Employee

kind Employee { name: Text }
kind Project  { name: Text, budget: Money }
kind Expense  { employee: Employee, project: Project, amount: Money,
                status: draft | approved | rejected = draft }
kind Book     { title: Text }
kind Loan     { book: Book, employee: Employee, returned: Bool = no }

define: a Loan is open when it is not returned
define: a Book is lent when the number of its Loans that are open is at least 1
define: an Expense is big when its amount is at least 1000 EUR

rule budget: the total amount of each Project's Expenses that are approved is at most its budget
rule single: the number of each Book's Loans that are open is at most 1
rule three:  the number of each Employee's Loans that are open is at most 3
rule daily:  for each Employee and day, the number of their Expenses is at most 5

keep the total amount of each Employee's Expenses

when a manager approves an Expense: the Expense is approved
when a manager rejects an Expense: the Expense is rejected
when a employee returns a Loan: the Loan is returned

admit submit a new Expense by employee where the Expense's employee is the caller
admit approve Expense by manager where the Expense's employee is not the caller
admit reject Expense by manager
admit borrow a new Loan by employee where the Loan's employee is the caller
admit return Loan by employee where the Loan's employee is the caller
admit add a new Employee, Project or Book by manager

kind Talk   { title: Text, length: Count, accepted: Bool = no }
kind Review { talk: Talk, score: Count }

define: a Talk is standard when its length is 30 or its length is 45

rule good: no Talk is accepted unless the total score of its Reviews is at least the number of its Reviews * 3

when a manager accepts a Talk: the Talk is accepted

admit propose a new Talk by employee where the Talk is standard
admit score a new Review by manager
admit accept Talk by manager
```

```scenario
scenario "a manager cannot approve their own expense, and a book is lent while a loan is open":
  given  a Employee Ann with name "Ann"
  and    a Employee Bo with name "Bo"
  and    a Project P with name "Web" and budget 500 EUR
  and    a Book K with title "Dune"
  when   employee Ann submits a new Expense E1 with employee Ann, project P and amount 300 EUR
  and    manager Ann approves E1
         is refused at the gate
  and    manager Bo approves E1
  and    employee Ann submits a new Expense E2 with employee Ann, project P and amount 300 EUR
  and    manager Bo approves E2
         is refused by rule budget
  and    employee Bo borrows a new Loan L1 with book K and employee Bo
  expect K is lent
  when   employee Ann borrows a new Loan L2 with book K and employee Ann
         is refused by rule single
  when   employee Bo returns L1
  expect K is not lent
  expect the total amount of Ann's Expenses is 600 EUR

scenario "a talk of 40 minutes is refused, and a low average cannot be accepted":
  given  a Employee Ann with name "Ann"
  and    a Employee Bo with name "Bo"
  when   employee Ann proposes a new Talk T1 with title "Rust" and length 40
         is refused at the gate
  when   employee Ann proposes a new Talk T2 with title "Rust" and length 45
  and    manager Bo scores a new Review R1 with talk T2 and score 2
  and    manager Bo scores a new Review R2 with talk T2 and score 3
  and    manager Bo accepts T2
         is refused by rule good
  when   manager Bo scores a new Review R3 with talk T2 and score 5
  and    manager Bo accepts T2
  expect T2 is accepted
```
