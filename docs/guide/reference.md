# Quick reference

Every form in Suture v0 on one page. `⟨np⟩` is a noun phrase, `⟨cond⟩` a condition, `⟨num⟩` a number, `[…]` is optional, `…` repeats.

## Declarations

```text
module ⟨name⟩ version ⟨n⟩, currency ⟨code⟩[, attempts ⟨n⟩][, candidates ⟨n⟩]

role ⟨role⟩ [is a ⟨Kind⟩]

kind ⟨Kind⟩ [(plural ⟨Plural⟩)] { ⟨attr⟩: ⟨Type⟩ [= ⟨literal or formula⟩], … }

⟨Kind⟩ needs ⟨Kind or Slot⟩[, …] [while ⟨cond⟩]
⟨Slot⟩ is ⟨Kind⟩ or ⟨Kind⟩[ or …]
⟨Kind⟩ uses a ⟨Kind⟩ [per ⟨element⟩] where ⟨cond⟩[, taking ⟨num⟩]

effect ⟨verb⟩ ⟨Kind⟩ via ⟨adapter⟩ within ⟨duration⟩
  sends    ⟨np⟩[, ⟨np⟩…]
  [marks   ⟨state⟩]
  reports  ⟨outcome⟩ [with ⟨attr⟩: ⟨Type⟩, …] [| fails with ⟨outcome⟩[, …]]

source ⟨Kind⟩'s ⟨attr⟩ via ⟨adapter⟩ within ⟨duration⟩
  sends      ⟨np⟩[, ⟨np⟩…]
  reports    ⟨outcome⟩ [| fails with ⟨outcome⟩[, …]]
  [good for  ⟨duration⟩]
  [otherwise use ⟨literal⟩ | ask again after ⟨duration⟩ | ⟨action⟩[ and ⟨action⟩…]]

decide [⟨Kind⟩ | ⟨Slot⟩ | ⟨Kind⟩'s ⟨attr⟩] [for ⟨Kinds⟩ [whose ⟨cond⟩]]
       with model "⟨name⟩@⟨version⟩" [within ⟨n⟩ milliseconds | within ⟨duration⟩] | with prefer

prefer ⟨Slot⟩: ⟨Kind⟩[, then ⟨Kind⟩ [after ⟨duration⟩]]…
prefer ⟨Kind⟩ [for a ⟨element⟩]: ⟨criterion⟩[, then ⟨criterion⟩]…
    criteria: lowest ⟨num⟩ · highest ⟨num⟩ · those ⟨cond⟩ · those whose ⟨attr⟩ …

admit ⟨verb⟩ a new ⟨Kind⟩[, ⟨Kind⟩… or ⟨Kind⟩] by ⟨role⟩[, ⟨role⟩] [where ⟨cond⟩]
admit ⟨verb⟩ ⟨Kind⟩ by ⟨role⟩[, ⟨role⟩] [where ⟨cond⟩]
admit change ⟨Kind⟩'s ⟨attr⟩ by ⟨role⟩[, ⟨role⟩] [where ⟨cond⟩]
apply rule ⟨name⟩ to ⟨Kinds⟩ created from version ⟨n⟩
```

**Types:** `Text`, `Bool`, `Count`, `Money`, `Duration`, `Time`, `⟨Kind⟩`, `list of ⟨Kind⟩`, `set of ⟨Kind⟩`. Built-in: `created: Time`.

**Literals:** `"text"`, `yes`/`no`, `3`, `9.99 EUR`, `30 seconds` (also minutes, hours, days, weeks).

## Sentences

```text
rule ⟨name⟩: ⟨rule⟩[, with ⟨attr⟩ at most ⟨duration⟩ old[ and ⟨attr⟩ at most ⟨duration⟩ old…]]
define: a ⟨Kind⟩ is ⟨state⟩ when ⟨cond⟩
desired [within ⟨duration⟩]: every ⟨subject⟩ is ⟨state⟩[ and ⟨state⟩…]
when a ⟨Kind⟩ is ⟨outcome⟩: ⟨action⟩[ and ⟨action⟩…]
when a ⟨role⟩ [or ⟨role⟩] ⟨verb⟩s a ⟨Kind⟩: ⟨action⟩[ and ⟨action⟩…]
```

Rule names are one lower-case word.

**Subjects:** `[⟨state⟩] ⟨Kind⟩ [whose ⟨attr⟩ ⟨predicate⟩] [that ⟨cond⟩]`

**Rules**

```text
every ⟨subject⟩ is ⟨state⟩
every ⟨subject⟩ has a ⟨Kind⟩ [that ⟨cond⟩]
every ⟨subject⟩'s ⟨attr⟩ ⟨comparison⟩ ⟨num⟩
no ⟨subject⟩ has a ⟨Kind⟩ [that ⟨cond⟩] [unless ⟨cond⟩]
no ⟨subject⟩ is ⟨state⟩ [unless ⟨cond⟩]
the total ⟨attr⟩ of each ⟨Kind⟩'s ⟨Kinds⟩ [that ⟨cond⟩] ⟨comparison⟩ ⟨num⟩
the number of each ⟨Kind⟩'s ⟨Kinds⟩ [that ⟨cond⟩] ⟨comparison⟩ ⟨num⟩
the total ⟨attr⟩ taken from each ⟨Kind⟩ ⟨comparison⟩ ⟨num⟩
for each ⟨Kind⟩ and ⟨day|week|month|year⟩, the total ⟨attr⟩ of their ⟨Kinds⟩ ⟨comparison⟩ ⟨num⟩
⟨num over all ⟨Kinds⟩⟩ ⟨comparison⟩ ⟨num⟩, checked globally
```

**Comparisons:** `is at most`, `is at least`, `is more than`, `is less than`.

**Actions:** `⟨np⟩ is ⟨Bool attr⟩` · `⟨np⟩ is not ⟨Bool attr⟩` · `⟨np⟩'s ⟨attr⟩ becomes ⟨num or np⟩`

**Noun phrases:** `it` · `its ⟨attr⟩` · `their ⟨attr⟩` · `⟨np⟩'s ⟨attr⟩` · `⟨np⟩'s ⟨Kind⟩` (current part) · `⟨np⟩'s ⟨Kinds⟩` (relation) · `the ⟨Kind⟩` · `the ⟨element⟩` · `the caller` · `all ⟨Kinds⟩` · literals

**Conditions:** `⟨np⟩ is [not] ⟨np, literal or state⟩` · `⟨num⟩ ⟨comparison⟩ ⟨num⟩` · `⟨np⟩ contains ⟨np⟩` · `⟨np⟩ does not contain ⟨np⟩` · `⟨np⟩ has a ⟨Kind⟩ [that ⟨cond⟩]` · `⟨np⟩ has a ⟨Kind⟩ or a ⟨Kind⟩` · `⟨np⟩ has no ⟨Kind⟩ [that ⟨cond⟩]` · `⟨cond⟩ and ⟨cond⟩` · `⟨cond⟩ or ⟨cond⟩` (never both in one sentence)

**Numbers:** `⟨np⟩` · `the total ⟨attr⟩ of ⟨np⟩'s ⟨Kinds⟩ [that ⟨cond⟩]` · `the number of ⟨np⟩'s ⟨Kinds⟩ [that ⟨cond⟩]` · `the total ⟨attr⟩ taken from ⟨np⟩` · `the total of ⟨calc⟩ over ⟨list⟩` · `+ - * /` and parentheses

**States:** Bool attributes · `define`d states · `done`, `failed` (parts and slots) · an effect's mark, outcomes and `timed out`

## Scenarios

```text
scenario "⟨title⟩":
  given  a ⟨Kind⟩ ⟨Name⟩ [with ⟨attr⟩ ⟨value⟩, … and ⟨attr⟩ ⟨value⟩]
  when   a new ⟨Kind⟩ ⟨Name⟩ with …
  when   ⟨role⟩ ⟨Name⟩ ⟨verb⟩s a new ⟨Kind⟩ ⟨Name⟩ with …
           and a ⟨element⟩ with …
  when   ⟨role⟩ ⟨Name⟩ ⟨verb⟩s ⟨Name⟩
  when   ⟨role⟩ ⟨Name⟩ changes ⟨Name⟩'s ⟨attr⟩ to ⟨value⟩
           is refused at the gate | is refused by rule ⟨name⟩
  when   ⟨adapter⟩ answers ⟨outcome⟩ [with ⟨attr⟩ ⟨value⟩, …] [for ⟨Name⟩]
  when   model "⟨name⟩@⟨version⟩" picks ⟨Name⟩[, then ⟨Name⟩…] | declines | is late
  when   ⟨n⟩ ⟨seconds|minutes|hours|days|weeks⟩ pass
  and    …
  expect ⟨cond about a Name⟩
  expect ⟨Name⟩'s ⟨Kind⟩ is [not] ⟨state⟩
  expect [an|no] alert for ⟨Name⟩
```

## The command

```text
suture build   ⟨dir⟩                               compile, write ⟨module⟩-v⟨n⟩.sut, print read-backs
suture run     ⟨dir⟩ --scenario ⟨file⟩             run scenarios and print their traces
suture serve   ⟨dir⟩ [--state ⟨dir⟩] [--listen ⟨addr⟩] [--models]
                                                   serve the program over HTTP, with its log (chapter 17)
suture deploy  ⟨dir⟩ [--state ⟨dir⟩]               check a new version against the live state (chapter 18)
suture explain ⟨dir⟩ --scenario ⟨file⟩ ⟨name⟩      … and explain one entity at the end of each
```

Exit status: 0 success, 1 a scenario failed, 2 the program does not compile or bad arguments.
