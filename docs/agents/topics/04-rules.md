# rules: rules, definitions, totals, and the parts of a sentence

```text
rule ⟨name⟩: every ⟨subject⟩ is ⟨state⟩
rule ⟨name⟩: every ⟨subject⟩'s ⟨attr⟩ ⟨cmp⟩ ⟨num⟩
rule ⟨name⟩: no ⟨subject⟩ is ⟨state⟩ [unless ⟨cond⟩]
rule ⟨name⟩: no ⟨subject⟩ has a ⟨Kind⟩ [that ⟨cond⟩] [unless ⟨cond⟩]
rule ⟨name⟩: the number of each ⟨Kind⟩'s ⟨Kinds⟩ [that ⟨cond⟩] ⟨cmp⟩ ⟨num⟩
rule ⟨name⟩: the total ⟨attr⟩ of each ⟨Kind⟩'s ⟨Kinds⟩ [that ⟨cond⟩] ⟨cmp⟩ ⟨num⟩
rule ⟨name⟩: the total ⟨attr⟩ taken from each ⟨Kind⟩ ⟨cmp⟩ ⟨num⟩
rule ⟨name⟩: no two ⟨Kinds⟩ [that ⟨cond⟩] have the same ⟨attr⟩[, ⟨attr⟩] [and ⟨attr⟩]   values never set after creation
rule ⟨name⟩: for each ⟨Kind⟩ and day|week|month|year [from their ⟨attr⟩ to their ⟨attr⟩], ⟨number rule about their ⟨Kinds⟩⟩
define: a ⟨Kind⟩ is ⟨state⟩ when ⟨cond⟩
keep the total ⟨attr⟩ | the number of each ⟨Kind⟩'s ⟨Kinds⟩ [that ⟨cond⟩]  a total for reads and scenarios
```

- **cmp:** `is at most`, `is at least`, `is more than`, `is less than`.
- **subject:** `[⟨state⟩] ⟨Kind⟩ [whose ⟨attr⟩ ⟨predicate⟩] [that ⟨cond⟩]`.
- **np:** `it`, `its ⟨attr⟩`, `their ⟨attr⟩`, `the ⟨Kind⟩`, `⟨np⟩'s ⟨attr⟩`, `⟨np⟩'s ⟨Kind⟩` (its part), `⟨np⟩'s ⟨Kinds⟩` (those referencing it), `the caller`.
- **cond:** `⟨np⟩ is [not] ⟨state, np or literal⟩`, `⟨num⟩ ⟨cmp⟩ ⟨num⟩`, `⟨np⟩ has [a|no] ⟨Kind⟩ [that ⟨cond⟩]`, `⟨np⟩ contains ⟨np⟩`, joined by `and` or by `or`; to mix them, group one side in parentheses: `it is not cancelled and (it is paid or it is free)`. After `and`, a bare state shares the subject: `it is cancelled and paid`.
- **inside a filter** (`that …` or `whose …` after `⟨np⟩'s ⟨Kinds⟩`), `it`, `its` and `the ⟨Kind⟩` all mean the entity being counted. A filter cannot compare it with the outer entity or the caller; for "at most once per pair", use `no two`.
- **num:** an np, a literal, `the number of ⟨np⟩'s ⟨Kinds⟩ [that ⟨cond⟩]`, `the total ⟨attr⟩ of ⟨np⟩'s ⟨Kinds⟩ […]`, `+ - * /`, parentheses.
- **Rules** are checked before every change. An input that would break one is refused, naming the rule. The system's own work (a part, a take from a pool) waits until the rule allows it. A rule also refuses the steps that would repair an old wrong state, so put conditions that must stay fixable in an `admit … where`.
- **Definitions** are always current; a stored status you would keep in sync is a `define` instead.
- **Totals** across references (`each Class's Bookings`) are kept incrementally; `suture build` shows each rule's cost (local, per group, global).
- **`no two`** counts per value of its attributes, kept by the first one that refers to another kind, so the rule is per group; with no reference among them, end it with `, checked globally`. The attributes must never change after creation. An input that would make two alike is refused.
