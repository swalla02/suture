# examples: two whole modules, tested

```suture
module studio version 1, currency EUR, attempts 3

role member is a Member
role staff

kind Member  { name: Text, banned: Bool = no }
kind Class   { name: Text, seats: Count, price: Money }
kind Booking { member: Member, class: Class, cancelled: Bool = no,
               price: Money = class's price,
               until: Time = created + 7 days }
kind Payment { booking: Booking }

Booking needs Payment while it is not cancelled

effect charge Payment via payments within 1 hour
  sends    the booking's price
  reports  paid with reference: Text | fails with declined

define: a Booking is live when it is not cancelled
define: a Booking is confirmed when its Payment is paid

rule seats:  the number of each Class's Bookings that are live is at most its seats
rule three:  the number of each Member's Bookings that are live is at most 3
rule banned: no Booking whose member is banned is live

desired within 1 day: every Booking that is live is confirmed

when a member or staff cancels a Booking: the Booking is cancelled
when a Payment is declined: its Booking is cancelled

admit book a new Booking by member where the Booking's member is the caller
admit cancel Booking by member where the Booking's member is the caller
admit cancel Booking by staff
admit add a new Member or Class by staff
admit change Member's banned by staff
```

```scenario
scenario "a member books, pays, and a full class refuses":
  given  a Member Ann with name "Ann"
  and    a Member Bo with name "Bo"
  and    a Class Yoga with name "Yoga", seats 1 and price 12 EUR
  when   member Ann books a new Booking B1 with member Ann and class Yoga
  and    payments answers paid with reference "p-1" for B1
  expect B1 is confirmed
  when   member Bo books a new Booking B2 with member Bo and class Yoga
         is refused by rule seats
  when   member Bo cancels B1
         is refused at the gate
  when   member Ann cancels B1
  and    member Bo books a new Booking B3 with member Bo and class Yoga
  expect the number of Yoga's Bookings that are live is 1

scenario "an unpaid booking raises an alert":
  given  a Member Ann with name "Ann"
  and    a Class Yoga with name "Yoga", seats 5 and price 12 EUR
  when   member Ann books a new Booking B1 with member Ann and class Yoga
  and    2 days pass
  expect an alert for B1
```

```suture
module recipes version 1, currency EUR

role employee is a Employee
role manager is a Employee

kind Employee { name: Text }
kind Project  { name: Text, budget: Money }
kind Expense  { employee: Employee, project: Project, amount: Money,
                status: draft | approved = draft }
kind Book     { title: Text }
kind Loan     { book: Book, employee: Employee, returned: Bool = no,
                due: Time = created + 14 days, overdue: Bool = no }
kind Talk     { title: Text, length: Count, accepted: Bool = no }
kind Review   { talk: Talk, reviewer: Employee, score: Count }

define: a Loan is open when it is not returned
define: a Book is lent when the number of its Loans that are open is at least 1
define: a Talk is standard when its length is 30 or its length is 45

rule budget: the total amount of each Project's Expenses that are approved is at most its budget
rule single: the number of each Book's Loans that are open is at most 1
rule once:   no two Reviews have the same talk and reviewer
rule good:   no Talk is accepted unless the total score of its Reviews is at least the number of its Reviews * 3

when a manager approves an Expense: the Expense is approved
when a employee returns a Loan: the Loan is returned
when a Loan's due passes: it is overdue
when a manager accepts a Talk: the Talk is accepted

admit submit a new Expense by employee where the Expense's employee is the caller
admit approve Expense by manager where the Expense's employee is not the caller
admit borrow a new Loan by employee where the Loan's employee is the caller
admit return Loan by employee where the Loan's employee is the caller
admit propose a new Talk by employee where the Talk is standard
admit score a new Review by manager
admit accept Talk by manager
admit add a new Employee, Project or Book by manager
```

```scenario
scenario "the recipes hold":
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
  when   employee Bo returns L1
  expect K is not lent
  when   employee Ann borrows a new Loan L2 with book K and employee Ann
  and    15 days pass
  expect L2 is overdue
  when   employee Ann proposes a new Talk T1 with title "Rust" and length 40
         is refused at the gate
  when   employee Ann proposes a new Talk T2 with title "Rust" and length 45
  and    manager Bo scores a new Review R1 with talk T2, reviewer Bo and score 2
  and    manager Bo accepts T2
         is refused by rule good
  when   manager Bo scores a new Review R2 with talk T2, reviewer Bo and score 5
         is refused by rule once
  when   manager Ann scores a new Review R3 with talk T2, reviewer Ann and score 5
  and    manager Bo accepts T2
  expect T2 is accepted
```
