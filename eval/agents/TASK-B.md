# Task: a conference's talk submissions, in Suture

Write a Suture module for a conference's call for talks, and scenarios that show it works.

Requirements:
1. Speakers submit talks. A talk has a title and a length in minutes (30 or 45 only).
2. Reviewers review talks with a score from 1 to 5. A reviewer cannot review their own talk (reviewers can also be speakers), and reviews a talk at most once.
3. The committee can accept a talk only once it has at least 2 reviews and their average score is at least 3.
4. At most 2 talks per speaker can be accepted.
5. The program has 600 minutes in total: the accepted talks' lengths may not add up to more.
6. A speaker can withdraw their own talk at any time; a withdrawn talk no longer counts toward requirements 4 and 5.
7. Every submitted talk should be decided (accepted or rejected) within 30 days; the committee is alerted about each one that is not.

Write at least one scenario per requirement, including the refusals, and make every scenario pass.
When you are done, report: the files you wrote, which requirements you could not express (if any) and why.
