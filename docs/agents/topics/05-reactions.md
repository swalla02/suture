# reactions: what happens when something happens

```text
when a ⟨role⟩ [or ⟨role⟩] ⟨verb⟩s a ⟨Kind⟩: ⟨action⟩[ and ⟨action⟩]
when a ⟨Kind⟩'s ⟨Time attr⟩ passes: ⟨action⟩[ and ⟨action⟩]          the clock reaches it
when a ⟨Kind⟩ is ⟨outcome | timed out⟩: ⟨action⟩[ and ⟨action⟩]           an effect's outcome
```

- **actions:** `⟨np⟩ is ⟨state or Bool⟩`, `⟨np⟩ is not ⟨Bool⟩`, `⟨np⟩'s ⟨attr⟩ becomes ⟨num or np⟩`. Only on the entity the reaction is about, its parts and its owner.
- An outcome reaction runs when the effect's answer arrives; a `timed out` one when the last send gets no answer. A reaction that would break a rule refuses the answer or the event; for `timed out` and a time passing, only the reaction is held back.
