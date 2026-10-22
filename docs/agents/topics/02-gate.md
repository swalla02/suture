# gate: roles and who may do what

```text
role ⟨role⟩ [is a ⟨Kind⟩]                                                callers; `is a` ties them to entities
admit ⟨verb⟩ a new ⟨Kind⟩[, ⟨Kind⟩ or ⟨Kind⟩] by ⟨role⟩[, ⟨role⟩] [where ⟨cond⟩]
admit ⟨verb⟩[, ⟨verb⟩ or ⟨verb⟩] ⟨Kind⟩ by ⟨role⟩ [where ⟨cond⟩]          events on an existing one, one per verb
admit change ⟨Kind⟩'s ⟨attr⟩ by ⟨role⟩ [where ⟨cond⟩]
```

- **Gate.** Every input (create, event, change) is refused unless an `admit` allows the caller's role, with its `where` true of the target and `the caller`.
- **The caller** is the entity of the caller's role (`role member is a Member`), read as `the caller` in an `admit … where` and in an event's reaction. A role with no kind has no caller entity.
- An event's `admit` and its reaction go together: `admit cancel Booking by member` lets `when a member cancels a Booking: …` happen.
