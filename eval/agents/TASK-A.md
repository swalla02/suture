# Task: a library's loans, in Suture

Write a Suture module for a small library, and scenarios that show it works.

Requirements:
1. Members and books. A book has a title and is either on the shelf or lent.
2. A librarian lends a book to a member: that makes a loan. Members cannot lend books to themselves.
3. A member has at most 3 loans open at once. A fourth is refused.
4. A book can be in at most one open loan.
5. A loan is due 14 days after it starts. A librarian can mark a loan returned; it is then closed.
6. If a loan is not returned by its due date, it becomes overdue on its own (no one has to do anything).
7. A member with an overdue loan cannot borrow more until it is returned.

Write at least one scenario per requirement, including the refusals, and make every scenario pass.
When you are done, report: the files you wrote, which requirements you could not express (if any) and why.
