# Suture, for agents: the core

Suture is a declarative backend language in controlled English: you state facts, rules, who may do what, and what work the system owes the world; the runtime does the rest (ordering, retries, compensation, limits, audit). Every sentence pattern has one meaning. You have not seen it in training: do not guess forms. Every form that exists is listed below with the topic that explains it. `suture help ⟨topic⟩` prints one topic, `suture help --for ⟨dir or file⟩` the topics a module already uses, and `suture help all` everything.

## loop: how to work

1. A module is the `.st` files of one directory; scenarios (tests) are `.scenario` files beside them. Before you edit an existing module, run `suture help --for <dir>`.
2. After every edit run `suture check <dir>`: it compiles, runs every scenario, and prints errors (`file:line: message (suture help ⟨topic⟩)`) and the trace around each failing expect. Exit 0 = all good; `--json` for tools.
3. Fix the first error first; read the topic it names when the message is not enough.
4. Cover each requirement with a scenario, and each limit or permission with a refused input.
5. When it passes, `suture build <dir>` prints every rule in plain English with its cost. Give that to the person: they review the read-back and the scenarios, not the syntax.

## forms: every form, and its topic

```text
module ⟨name⟩ version ⟨n⟩, currency ⟨EUR|USD|…⟩[, attempts ⟨n⟩]                      kinds
kind ⟨Kind⟩ [(plural ⟨Plural⟩)] { ⟨attr⟩: ⟨Type⟩ [= ⟨default or formula⟩], … }        kinds
role ⟨role⟩ [is a ⟨Kind⟩]                                                         gate
admit ⟨verb⟩ a new ⟨Kind⟩[, ⟨Kind⟩ or ⟨Kind⟩] by ⟨role⟩[, ⟨role⟩] [where ⟨cond⟩]     gate
admit ⟨verb⟩[, ⟨verb⟩ or ⟨verb⟩] ⟨Kind⟩ by ⟨role⟩ [where ⟨cond⟩]                   gate
admit change ⟨Kind⟩'s ⟨attr⟩ by ⟨role⟩ [where ⟨cond⟩]                             gate
⟨Kind⟩ needs ⟨Kind or Slot⟩[, …] [while ⟨cond⟩]                                  parts
⟨Slot⟩ is ⟨Kind⟩ or ⟨Kind⟩ · prefer ⟨Slot⟩: ⟨Kind⟩, then ⟨Kind⟩ [after ⟨duration⟩]    parts
⟨Kind⟩ uses a ⟨Kind⟩ where ⟨cond⟩[, taking ⟨num⟩] · prefer ⟨Kind⟩: lowest ⟨num⟩ …     parts
effect ⟨verb⟩ ⟨Kind⟩ via ⟨adapter⟩ within ⟨duration⟩, with sends/marks/reports lines   parts
rule ⟨name⟩: every ⟨subject⟩ is ⟨state⟩ | every ⟨subject⟩'s ⟨attr⟩ ⟨cmp⟩ ⟨num⟩         rules
rule ⟨name⟩: no ⟨subject⟩ is ⟨state⟩ | has a ⟨Kind⟩ [that ⟨cond⟩] [unless ⟨cond⟩]    rules
rule ⟨name⟩: the number | the total ⟨attr⟩ of each ⟨Kind⟩'s ⟨Kinds⟩ [that …] ⟨cmp⟩ ⟨num⟩  rules
rule ⟨name⟩: no two ⟨Kinds⟩ [that ⟨cond⟩] have the same ⟨attr⟩[, …] [and ⟨attr⟩]     rules
rule ⟨name⟩: the total ⟨attr⟩ taken from each ⟨Kind⟩ ⟨cmp⟩ ⟨num⟩                    parts
rule ⟨name⟩: for each ⟨Kind⟩ and day|week|month|year [from … to …], ⟨number rule⟩   time
define: a ⟨Kind⟩ is ⟨state⟩ when ⟨cond⟩                                          rules
keep the total ⟨attr⟩ | the number of each ⟨Kind⟩'s ⟨Kinds⟩ [that ⟨cond⟩]           rules
desired [within ⟨duration⟩]: every ⟨subject⟩ is ⟨state⟩                           time
when a ⟨role⟩ [or ⟨role⟩] ⟨verb⟩s a ⟨Kind⟩: ⟨action⟩[ and ⟨action⟩]                  reactions
when a ⟨Kind⟩ is ⟨outcome | timed out⟩: ⟨action⟩[ and ⟨action⟩]                      reactions
when a ⟨Kind⟩'s ⟨Time attr⟩ passes: ⟨action⟩[ and ⟨action⟩]                         time
decide ⟨Slot⟩ with model "⟨m⟩@1" … · source ⟨Kind⟩'s ⟨attr⟩ via model "⟨m⟩@1" …     models
scenario "⟨title⟩": given / when / expect lines                                  scenarios
suture serve ⟨dir⟩: HTTP inputs and JSON values                                   serve
```

Conditions (`⟨cond⟩`), noun phrases, numbers and filters are in `rules`; actions in `reactions`; types, literals and formulas in `kinds`. Two whole modules that use most forms are in `examples`.

## recipes: requirement → form

| Requirement | Write |
|---|---|
| status that follows from facts | `define: a Booking is confirmed when its Payment is paid` |
| status someone sets | state attribute + `when a staff ships an Order: the Order is shipped` + `admit ship Order by staff` |
| only their own | `admit … by member where the Booking's member is the caller` |
| not their own | `admit approve Expense by manager where the Expense's employee is not the caller` |
| at most once per pair, a unique value | `rule once: no two Reviews have the same talk and reviewer`; with no reference among them, end with `, checked globally` |
| at most N per owner / at most one active | `rule three: the number of each Member's Bookings that are live is at most 3` |
| sum within a budget | `rule budget: the total amount of each Project's Expenses that are approved is at most its budget` |
| blocked while something holds | `admit borrow a new Loan by member where the number of the Loan's member's Loans that are late is at most 0` |
| one of a set of numbers | `define: a Talk is standard when its length is 30 or its length is 45` + `admit … where the Talk is standard` |
| an average at least N | compare totals: `… unless the total score of its Reviews is at least the number of its Reviews * 3` |
| derived from what references it | `define: a Book is lent when the number of its Loans that are open is at least 1` |
| call an outside service | a part with an `effect`; scenarios answer for it |
| undo work on cancel | `needs … while it is not cancelled` |
| try A, else B after a wait | `Return is Courier or Collection` + `prefer Return: Courier, then Collection after 2 days` |
| shared stock | `Hold uses a Title where the Title's name is the Loan's title, taking the Loan's copies` + `rule stock: the total copies taken from each Title is at most its stock` |
| deadline that alerts | `desired within 30 days: every Talk is decided` |
| becomes overdue by itself | `due: Time = created + 14 days` + `when a Loan's due passes: it is overdue`; guard with `define: a Loan is late when it is overdue and it is not returned` |
| limit per day or month | `rule daily: for each Member and day, the number of their Bookings is at most 2` |
| number of days between dates | `days: Count = the days from its from to its to`; Monday to Friday: `the weekdays from …` |

## limits: not writable yet, and what to do

| Cannot | Instead |
|---|---|
| `now`, "past its due date" in a condition | `when a ⟨Kind⟩'s ⟨time⟩ passes` sets a fact the condition reads |
| `no two` on a value an answer or a source sets | compare a value callers set, or say duplicates are not prevented |
| a reaction setting another entity it only references | a `define` on that entity, from a total of what references it |
| a negative `Count` (`score - 3` is not −1) | compare totals, or use `Money` |
| holidays and half days in a day count | take working days as an input, and say so |
| taking back a request already sent | guard the state it sets with the fact that ends it |
| loops, functions, generated text or code | none, by design |

Say in your report when a requirement needs one of these.
