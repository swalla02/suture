# gate: roles, who may do what (admit), and reactions to what they do

Every input passes a gate. Nothing is allowed unless an `admit` allows it.

```suture
module desk version 1, currency EUR

role member is a Member
role librarian

kind Member  { name: Text }
kind Request { member: Member, note: Text, closed: Bool = no }

admit open a new Request by member where the Request's member is the caller
admit close Request by librarian
admit add a new Member by librarian
admit change Request's note by member where the Request's member is the caller

when a librarian closes a Request: the Request is closed
```

| Form | Allows |
|---|---|
| `admit ⟨verb⟩ a new ⟨Kind⟩[ or ⟨Kind⟩] by ⟨role⟩[, ⟨role⟩] [where ⟨cond⟩]` | creating one |
| `admit ⟨verb⟩ ⟨Kind⟩ by ⟨role⟩ [where ⟨cond⟩]` | an event on an existing one |
| `admit change ⟨Kind⟩'s ⟨attr⟩ by ⟨role⟩ [where ⟨cond⟩]` | changing one attribute |

- `role member is a Member` ties callers to Member entities; then `the caller` is that Member in a `where`. "Only for themselves" is `where the ⟨Kind⟩'s member is the caller`; "not for themselves" is `where the ⟨Kind⟩'s member is not the caller`.
- The `where` reads the target (the new entity as it would be, or the existing one) and the caller, and totals they keep: `where the number of the Loan's member's Loans that are late is at most 0`.
- An event does nothing by itself: a reaction says what changes. `when a ⟨role⟩ [or ⟨role⟩] ⟨verb⟩s a ⟨Kind⟩: ⟨action⟩[ and ⟨action⟩]`. Actions: `the X is ⟨state or Bool⟩`, `the X is not ⟨Bool⟩`, `the X's ⟨attr⟩ becomes ⟨value⟩`.
- A reaction changes the entity it is about. It cannot set an attribute of another entity it only references; derive that one with a `define` or a total instead (`suture help recipes`).
- In scenarios, a gate refusal is `is refused at the gate`; a rule's is `is refused by rule ⟨name⟩`.
