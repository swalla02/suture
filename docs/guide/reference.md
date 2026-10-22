# Quick reference

Every form in Suture v0 on one page. `⟨np⟩` is a noun phrase, `⟨cond⟩` a condition, `⟨num⟩` a number, `[…]` is optional, `…` repeats.

## Declarations

```text
module ⟨name⟩ version ⟨n⟩, currency ⟨code⟩[, attempts ⟨n⟩][, candidates ⟨n⟩]

role ⟨role⟩ [is a ⟨Kind⟩]

kind ⟨Kind⟩ [(plural ⟨Plural⟩)] { ⟨attr⟩: ⟨Type⟩ [= ⟨literal or formula⟩], … }
                                   (a formula may read a value a source reads once, with no `good for`)

⟨Kind⟩ needs ⟨Kind or Slot⟩[, …] [while ⟨cond⟩]
⟨Slot⟩ is ⟨Kind⟩ or ⟨Kind⟩[ or …]
⟨Kind⟩ uses a ⟨Kind⟩ [per ⟨element⟩] where ⟨cond⟩[, taking ⟨num⟩ | taking ⟨count⟩]

effect ⟨verb⟩ ⟨Kind⟩ via ⟨adapter⟩ within ⟨duration⟩
  sends    ⟨np⟩[, ⟨np⟩…]
  [marks   ⟨state⟩]
  reports  ⟨outcome⟩ [with ⟨attr⟩: ⟨Type⟩, …] [| fails with ⟨outcome⟩[, …]]

source ⟨Kind⟩'s ⟨attr⟩ via ⟨adapter⟩ within ⟨duration⟩
  sends      ⟨np⟩[, ⟨np⟩…]
  reports    ⟨outcome⟩ [| fails with ⟨outcome⟩[, …]]
  [good for  ⟨duration⟩]
  [otherwise use ⟨literal⟩ | ask again after ⟨duration⟩ | ⟨action⟩[ and ⟨action⟩…]]

source ⟨Kind⟩'s ⟨attr⟩ via model "⟨name⟩@⟨version⟩" within ⟨duration⟩
  sends      ⟨np⟩[, ⟨np⟩…]                       (media allowed)
  [asking    "⟨instructions⟩"]
  [at least ⟨0…1⟩ sure]
  [good for  ⟨duration⟩]
  [otherwise …]

decide [⟨Kind⟩ | ⟨Slot⟩ | ⟨Kind⟩'s ⟨attr⟩] [for ⟨Kinds⟩ [whose ⟨cond⟩]]
       with model "⟨name⟩@⟨version⟩" [within ⟨n⟩ milliseconds | within ⟨duration⟩] [at least ⟨0…1⟩ sure] [otherwise ⟨Option⟩] | with prefer
decide … with model "⟨a⟩@⟨v⟩" or model "⟨b⟩@⟨v⟩"[ or prefer…] [within …] [at least … sure] [otherwise ⟨Option⟩],
       picked by model "⟨router⟩@⟨v⟩" [within …] [at least … sure] [otherwise model "⟨a⟩@⟨v⟩"]   (a model picks the model)

prefer ⟨Slot⟩: ⟨Kind⟩[, then ⟨Kind⟩ [after ⟨duration⟩]]…
prefer ⟨Kind⟩ [for a ⟨element⟩]: ⟨criterion⟩[, then ⟨criterion⟩]…
    criteria: lowest ⟨num⟩ · highest ⟨num⟩ · those ⟨cond⟩ · those whose ⟨attr⟩ …

admit ⟨verb⟩ a new ⟨Kind⟩[, ⟨Kind⟩… or ⟨Kind⟩] by ⟨role⟩[, ⟨role⟩] [where ⟨cond⟩]
admit ⟨verb⟩ ⟨Kind⟩ by ⟨role⟩[, ⟨role⟩] [with ⟨name⟩: ⟨Type⟩[, …]] [where ⟨cond⟩]
admit change ⟨Kind⟩'s ⟨attr⟩ by ⟨role⟩[, ⟨role⟩] [where ⟨cond⟩]
apply rule ⟨name⟩ to ⟨Kinds⟩ created from version ⟨n⟩
allow ⟨adapter⟩ to receive ⟨Kind⟩'s ⟨attr⟩[, ⟨Kind⟩'s ⟨attr⟩…]
keep the total ⟨attr⟩ of each ⟨Kind⟩'s ⟨Kinds⟩ [that ⟨cond⟩] | keep the number of each ⟨Kind⟩'s ⟨Kinds⟩ [that ⟨cond⟩]
```

**Types:** `Text`, `Bool`, `Count`, `Money`, `Duration`, `Time`, `Media`, `⟨Kind⟩`, `list of ⟨Kind⟩`, `set of ⟨Kind⟩`. Built-in: `created: Time`. A state attribute: `⟨attr⟩: ⟨state⟩ | ⟨state⟩… [= ⟨state⟩]`.

**Marks:** after the type, `secret` (Text only, no default) or `personal`: `card: Text secret` (chapter 19).

**Literals:** `"text"`, `yes`/`no`, `3`, `9.99 EUR`, `30 seconds` (also minutes, hours, days, weeks). A duration is at most 100 years. In scenarios, `file "dent.png"` gives media, read beside the scenario file.

**Checked once:** `attempts` and `candidates` are at least 1; no two kinds share a plural, and no plural is another kind's name; a choice has one `prefer` (one per `for a ⟨element⟩`).

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
for each ⟨Kind⟩ and ⟨day|week|month|year⟩ [from their ⟨attr⟩ to their ⟨attr⟩], the total ⟨attr⟩ of their ⟨Kinds⟩ ⟨comparison⟩ ⟨num⟩
⟨num over all ⟨Kinds⟩⟩ ⟨comparison⟩ ⟨num⟩, checked globally
no two ⟨Kinds⟩ [that ⟨cond⟩] have the same ⟨attr⟩[, ⟨attr⟩…] [and ⟨attr⟩][, checked globally]
```

**Comparisons:** `is at most`, `is at least`, `is more than`, `is less than`.

**Actions:** `⟨np⟩ is ⟨Bool attr⟩` · `⟨np⟩ is not ⟨Bool attr⟩` · `⟨np⟩ is ⟨state⟩` · `⟨np⟩'s ⟨attr⟩ becomes ⟨num or np⟩`

**Noun phrases:** `it` · `its ⟨attr⟩` · `their ⟨attr⟩` · `⟨np⟩'s ⟨attr⟩` · `⟨np⟩'s ⟨Kind⟩` (current part) · `⟨np⟩'s ⟨Kinds⟩` (relation) · `the ⟨Kind⟩` · `the ⟨element⟩` · `the caller` · `all ⟨Kinds⟩` · literals

**Conditions:** `⟨np⟩ is [not] ⟨np, literal or state⟩` · `⟨num⟩ ⟨comparison⟩ ⟨num⟩` · `⟨np⟩ contains ⟨np⟩` · `⟨np⟩ does not contain ⟨np⟩` · `⟨np⟩ has a ⟨Kind⟩ [that ⟨cond⟩]` · `⟨np⟩ has a ⟨Kind⟩ or a ⟨Kind⟩` · `⟨np⟩ has no ⟨Kind⟩ [that ⟨cond⟩]` · `⟨cond⟩ and ⟨cond⟩` · `⟨cond⟩ or ⟨cond⟩` (never both in one sentence)

**Numbers:** `⟨np⟩` · `the total ⟨attr⟩ of ⟨np⟩'s ⟨Kinds⟩ [that ⟨cond⟩]` · `the number of ⟨np⟩'s ⟨Kinds⟩ [that ⟨cond⟩]` (also across units, and through a reference: `its unit's Items`) · `the days in a row of their ⟨Kinds⟩ [that ⟨cond⟩]` (in `for each ⟨Kind⟩ and day from … to …`) · `the days from ⟨Time⟩ to ⟨Time⟩`, a Count of calendar days with both ends counted · `the total ⟨attr⟩ taken from ⟨np⟩` · `the total of ⟨calc⟩ over ⟨list⟩` · `+ - * /` and parentheses · `⟨Money⟩ * ⟨Money⟩ / ⟨Money⟩`, a share · `-3 EUR`

**States:** Bool attributes · the states of state attributes · `define`d states · `done`, `failed` (parts and slots) · an effect's mark, outcomes and `timed out`

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
  when   model "⟨name⟩@⟨version⟩" picks ⟨Name⟩ | "⟨model⟩@⟨v⟩" | prefer [at ⟨0…1⟩][, then …] | declines | is late
  when   model "⟨name⟩@⟨version⟩" reads ⟨value⟩ [at ⟨0…1⟩] | nothing[, then …]
  when   ⟨n⟩ ⟨seconds|minutes|hours|days|weeks⟩ pass
  when   it is ⟨YYYY-MM-DD⟩ [⟨H:MM⟩]
  and    …
  expect ⟨cond about a Name⟩
  expect ⟨Name⟩'s ⟨Kind⟩ is [not] ⟨state⟩
  expect [an|no] alert for ⟨Name⟩
```

## The command

```text
suture check   ⟨dir⟩ [--json]                      compile and run every scenario beside it; traces only for failures
suture build   ⟨dir⟩                               compile, write ⟨module⟩-v⟨n⟩.sut and .manifest, print read-backs
suture run     ⟨dir⟩ --scenario ⟨file⟩             run scenarios and print their traces
suture serve   ⟨dir⟩ [--state ⟨dir⟩] [--listen ⟨addr⟩] [--pod ⟨name⟩] [--models]
                                                   serve the program over HTTP, with its log (chapters 17, 20)
suture deploy  ⟨dir⟩ [--state ⟨dir⟩]               check a new version against the live state (chapter 18)
suture explain ⟨dir⟩ --scenario ⟨file⟩ ⟨name⟩      … and explain one entity at the end of each
suture lsp                                         the language server, over stdin and stdout (chapter 21)
suture init    ⟨dir⟩                               AGENTS.md, CLAUDE.md and a starter module, for a coding agent
suture help    [⟨section⟩ | all]                   the agent book: the language in about 4k tokens (chapter 12)
```

Exit status: 0 success, 1 a scenario failed (for `check`, anything failed), 2 the program does not compile or bad arguments.
