# kinds: the module line, kinds, types, literals and formulas

```text
module ⟨name⟩ version ⟨n⟩, currency ⟨EUR|USD|…⟩[, attempts ⟨n⟩]       first line, once
kind ⟨Kind⟩ [(plural ⟨Plural⟩)] { ⟨attr⟩: ⟨Type⟩ [= ⟨default or formula⟩], … }
```

- **Types:** `Text`, `Bool`, `Count` (whole, ≥ 0), `Money`, `Time`, `Duration`, `Media`, a kind (a reference), `list of ⟨Kind⟩`, `set of ⟨Kind⟩`; a state attribute `status: open | paid | shipped = open`. Every entity has `created: Time`.
- **Literals:** `"text"`, `yes`/`no`, `3`, `9.99 EUR`, `-3 EUR`, `30 seconds|minutes|hours|days|weeks`. dates `2026-03-01` and `2026-03-01 14:30` (UTC).
- **Formulas** are computed when the entity is created: `class's price`, `price * quantity`, `created + 14 days`, `the days from its from to its to` (calendar dates, both ends: 4 for Jan 5–8), `the weekdays from … to …` (Monday to Friday), `the total of ⟨calc⟩ over its ⟨list⟩`.
- **Names:** kinds are Capitalised, attributes, states, verbs and rule names one lower-case word.
