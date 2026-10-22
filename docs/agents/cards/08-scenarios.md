# scenarios: tests in the same language, one file per topic beside the module

```text
scenario "⟨title⟩":
  given  a ⟨Kind⟩ ⟨Name⟩ [with ⟨attr⟩ ⟨value⟩, … and ⟨attr⟩ ⟨value⟩]
  when   ⟨role⟩ ⟨Caller⟩ ⟨verb⟩s a new ⟨Kind⟩ ⟨Name⟩ with ⟨attr⟩ ⟨value⟩ and …
  when   ⟨role⟩ ⟨Caller⟩ ⟨verb⟩s ⟨Name⟩
  when   ⟨role⟩ ⟨Caller⟩ changes ⟨Name⟩'s ⟨attr⟩ to ⟨value⟩
           is refused at the gate | is refused by rule ⟨name⟩      (on the next line)
  when   ⟨adapter⟩ answers ⟨outcome⟩ [with ⟨attr⟩ ⟨value⟩] for ⟨Name⟩
  when   model "⟨name⟩@⟨v⟩" picks ⟨Option⟩ [at 0.9] | declines | is late
  when   ⟨n⟩ days pass  |  when it is 2026-03-01
  and    …
  expect ⟨cond with names⟩        expect ⟨Name⟩'s ⟨Kind⟩ is [not] ⟨state⟩        expect [an|no] alert for ⟨Name⟩
```

- The first word of a step is `given`, `when`, `and` or `expect`; alignment is only for people. Steps run in order, and `when`/`expect` can alternate.
- `given` builds the world with no gate; `when` sends inputs through the gate, as the caller named (`staff Sam`). With `role member is a Member`, the caller's name is a Member's name: `member M borrows …`.
- Names (`B1`, `M`) are labels for the entities. Values: `"text"`, `3`, `9.99 EUR`, `yes`, `2026-03-01`, a name for a reference.
- An input that should be refused must say so on the next line, or the scenario fails. A failed `expect` does not stop the scenario.
- Cover each requirement with a scenario that passes and, for each limit, one that is refused.
- Every adapter is played by the scenario; an unanswered request just waits, and time passing re-sends and times it out.
- `suture check <dir>` runs every `.scenario` beside the module. `suture explain <dir> --scenario <file> <Name>` explains one entity at the end of each scenario: its states, parts, totals and why a step waits.
