# serve: HTTP and JSON

`suture serve <dir>` serves the module with a durable log. Every `POST /inputs` needs the header `Suture-Caller: ⟨role⟩ ⟨name⟩` (`member Ann`), else 401. Its body is one of:

- `{"new": {"verb": "books", "kind": "Booking", "label": "B1", "attrs": {…}, "items": {"lines": [{…}]}}}`
- `{"event": {"verb": "cancels", "target": "B1", "data": {…}, "part": "Ticket"}}`: `data` only if the `admit` declares some, `part` for an event to a part
- `{"change": {"target": "Fri", "attr": "seats", "value": 120}}`

It answers 200 with the trace, 409 with `refused` and the trace, or 400 for a malformed body. `GET /units/⟨label⟩` explains one entity. Adapters with no URL in `adapters.json` poll `GET /pending` and answer with `POST /answers`, `{"adapter": "payments", "outcome": "paid", "key": "B1/Payment#1", "data": {…}}`, under `Authorization: Bearer $SUTURE_OPERATOR_TOKEN`. JSON values: `Count` an integer, `Money` `"12.50 EUR"`, `Time` RFC 3339 (`"2026-03-01T14:30:00Z"`), `Duration` ISO 8601 (`"PT2H"`, `"P3D"`), a reference its label, a set a list of labels.
