# api: serving a module over HTTP, and the JSON forms of values

`suture serve <dir> [--state <dir>] [--listen 127.0.0.1:7878]` serves the module with a durable log. Every input passes the gate; the caller is the `Suture-Caller` header (`member Ann`).

| Request | Does |
|---|---|
| `POST /inputs` with `{"create": {"kind": "Loan", "label": "L1", "verb": "lend", "data": {…}}}`, `{"event": {"target": "L1", "verb": "return"}}`, `{"change": {"target": "L1", "attr": "note", "value": "…"}}` | sends an input: 200 with the trace, 409 with `refused`, 400 when malformed |
| `GET /units/⟨label⟩` | explains one entity |
| `GET /pending`, `POST /answers` | requests to adapters with no URL, and their answers |
| `POST /media` | stores media by SHA-256 |
| `GET /trace?from=⟨n⟩`, `GET /health` | the trace, and the module and pods |

Values in JSON follow the attribute's type: text a string, `Bool` a boolean, `Count` an integer, `Money` a string with the currency (`"12.50 EUR"`), `Time` RFC 3339 (`"2026-03-01T14:30:00Z"`, an offset, or a date), `Duration` ISO 8601 (`"PT2H30M"`, `"P3D"`), a reference the entity's label, a set a list of labels.

`adapters.json` beside the module gives each adapter a URL; an adapter not named there polls `GET /pending`. `suture deploy <dir>` checks a new version against the live state before it is served.
