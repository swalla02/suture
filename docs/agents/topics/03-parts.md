# parts: needs, slots, pools and effects

```text
⟨Kind⟩ needs ⟨Kind or Slot⟩[, …] [while ⟨cond⟩]                         each one gets its own part
⟨Slot⟩ is ⟨Kind⟩ or ⟨Kind⟩                                               one part, several kinds
prefer ⟨Slot⟩: ⟨Kind⟩, then ⟨Kind⟩ [after ⟨duration⟩]
⟨Kind⟩ uses a ⟨Kind⟩ where ⟨cond⟩[, taking ⟨num⟩]                       take from a shared pool
prefer ⟨Kind⟩: lowest|highest ⟨num⟩ [within ⟨margin⟩] | those ⟨cond⟩ | those that are ⟨defined⟩ | those whose … or whose …[, then …]     how a pool is chosen
effect ⟨verb⟩ ⟨Kind⟩ via ⟨adapter⟩ within ⟨duration⟩                      outside work, with a deadline
  sends    ⟨np⟩[, ⟨np⟩]
  [marks   ⟨state⟩]                                                      true once sent
  reports  ⟨outcome⟩ [with ⟨attr⟩: ⟨Type⟩] [| fails with ⟨outcome⟩[, …]]
```

- **Needs** create parts; an effect sends the part's request, re-sends after `within` up to `attempts`, then the part is `timed out`. When a need's `while` turns false, an unfinished part is withdrawn and gives back what it took; one whose request went out stays and counts.
- **Pools.** A `uses` takes from a shared entity, chosen by `prefer`; `the total ⟨attr⟩ taken from each ⟨Kind⟩` bounds it. A take a rule refuses waits for room.
- **Slots.** One part, several kinds, tried in `prefer` order; `after` is how long to wait on one before the next.
