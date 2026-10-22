# scenarios: tests in the same language

```text
scenario "⟨title⟩":
  given  a ⟨Kind⟩ ⟨Name⟩ [with ⟨attr⟩ ⟨value⟩, … and ⟨attr⟩ ⟨value⟩]     no gate
  given  14 ⟨Kinds⟩ T1 to T14 [with …]                                 n alike; also `⟨verb⟩s 3 new ⟨Kinds⟩ B1 to B3`
  when   ⟨role⟩ ⟨Caller⟩ ⟨verb⟩s a new ⟨Kind⟩ ⟨Name⟩ with …              through the gate
  when   ⟨role⟩ ⟨Caller⟩ ⟨verb⟩s ⟨Name⟩
  when   ⟨role⟩ ⟨Caller⟩ changes ⟨Name⟩'s ⟨attr⟩ to ⟨value⟩
         is refused at the gate | is refused by rule ⟨name⟩            required for a refusal
  when   ⟨adapter⟩ answers ⟨outcome⟩ [with ⟨attr⟩ ⟨value⟩] for ⟨Name⟩
  when   ⟨n⟩ days pass | when it is 2026-03-01 [14:30]
  when   model "⟨m⟩@1" picks ⟨Option⟩ [at 0.9] | reads ⟨value⟩ [at 0.9] | declines | is late
  and    …                                                            the same as the line above
  expect ⟨Name⟩ is [not] ⟨state⟩ | ⟨cond, with names as subjects⟩
  expect ⟨Name⟩'s ⟨Part⟩ is [not] ⟨state⟩                              a part it needs
  expect [an|no] alert for ⟨Name⟩
```

The caller of a role with `is a Member` is named by a Member's name. Values: `"text"`, `3`, `9.99 EUR`, `yes`, `2026-03-01`, a name for a reference. The clock starts at 2026-01-01 00:00 UTC; time passing re-sends and times out requests. A failed `expect` does not stop the scenario. `suture explain <dir> --scenario <file> <Name>` shows one entity's states, parts, totals and why a step waits.
