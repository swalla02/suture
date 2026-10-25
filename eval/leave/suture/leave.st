-- Leave applications, after frappe/hrms (github.com/frappe/hrms at 7fc314bdb):
-- employees apply for leave of a type, against an allocation of that type,
-- and their approver approves or rejects it. An approved leave takes its
-- days from the allocation's balance; a cancelled one gives them back.
module leave version 1, currency EUR

role employee is a Employee
role approver is a Approver
role hr

-- hrms takes days off from a holiday list. Weekends are in that list too,
-- and the tests give none, so the calendar works every day. A leave day is
-- its 8 hours, so a half day is 9:00 to 13:00 or 13:00 to 17:00.
calendar leave: every day, 9:00 to 17:00, except 2026-07-01, 2026-08-09 and 2026-08-10

kind Company  { name: Text, selfish: Bool = no, backdating: Bool = yes }
kind Approver { user: Text }
kind Employee { user: Text, company: Company, joined: Time, approver: Approver }
kind Type     { name: Text, negative: Bool = no, longest: Count = 0, after: Duration = 0 days }
kind Allocation { employee: Employee, type: Type, from: Time, to: Time, leaves: Duration }
kind Application { employee: Employee, type: Type, allocation: Allocation, approver: Approver,
                   from: Time, to: Time, days: Duration = the leave time from its from to its to,
                   status: open | approved | rejected | cancelled }

define: an Application is held when it is open or it is approved
define: an Application is counted when it is approved and its type is not negative
define: an Application is placed when its allocation's from is at most its from and its allocation's to is at least its to
define: an Application is fitting when it is placed or its type is negative
define: an Application is affordable when its days is at most its allocation's leaves - the total days of its allocation's Applications that are counted or its type is negative
define: an Application is entitled when its type's after is 0 days or its from is less than its employee's joined or its from - its employee's joined is at least its type's after
define: an Application is timely when its from is more than its created - 1 day or its employee's company is backdating
define: an Application is sound when it is fitting and it is affordable and it is entitled
define: an Application is valid when its to is at least its from and its days is more than 0 hours and it is sound
define: an Application is permitted when its employee's company is selfish

keep the total days of each Allocation's Applications that are open
keep the total days of each Allocation's Applications that are approved

rule apart: for each Employee and day from their from to their to, the total leave time of their Applications that are held is at most 8 hours
rule balance: the total days of each Allocation's Applications that are counted is at most its leaves
rule consecutive: for each Allocation and day from their from to their to, the days in a row of their Applications that are held is at most its type's longest or its type's longest is 0

admit apply a new Application by employee where the Application's employee is the caller and the Application is valid and it is timely and its allocation's employee is its employee and its allocation's type is its type
admit apply a new Application by hr where the Application is valid and its allocation's employee is its employee and its allocation's type is its type
admit approve Application by approver where the Application is open and its approver is the caller and the caller's user is not its employee's user
admit approve Application by approver where the Application is open and its approver is the caller and it is permitted
admit reject Application by approver where the Application is open and its approver is the caller
admit approve Application by hr where the Application is open
admit reject Application by hr where the Application is open
admit discard Application by employee where the Application is open and its employee is the caller
admit cancel Application by hr where the Application is approved
admit add a new Company, Approver, Employee, Type or Allocation by hr

when a approver or hr approves an Application: the Application is approved
when a approver or hr rejects an Application: the Application is rejected
when a employee discards an Application: the Application is cancelled
when a hr cancels an Application: the Application is cancelled
