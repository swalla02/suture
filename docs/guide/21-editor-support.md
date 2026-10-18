# 21. Editor support

`suture lsp` is a language server. Any editor with a Language Server Protocol client can use it to show errors as you type, the read-back of a statement when you hover it, and the words that can come next.

## Setting it up

The server runs over stdin and stdout, and needs no arguments. A module is the `.st` files of one directory, as for `suture build`.

**Neovim** (0.11 or later):

```lua
vim.filetype.add({ extension = { st = "suture" } })
vim.lsp.config("suture", { cmd = { "suture", "lsp" }, filetypes = { "suture" }, root_markers = { ".git" } })
vim.lsp.enable("suture")
```

**Helix**, in `languages.toml`:

```toml
[language-server.suture]
command = "suture"
args = ["lsp"]

[[language]]
name = "suture"
scope = "source.suture"
file-types = ["st"]
language-servers = ["suture"]
```

**VS Code** has no built-in client for an arbitrary server. Use a generic LSP client extension, pointed at `suture lsp` for `*.st` files. A packaged extension comes later.

## Errors as you type

Each change compiles the whole module, with your unsaved text, and puts each error on its line, as `suture build` would report it:

```text
tickets.st:33: unknown kind Bookng; did you mean Booking?
```

Warnings, such as a slot with no `prefer`, name no line, so they appear on the `module` line.

## Read-backs on hover

Hover a rule to see what the compiler understood:

```text
rule seats

understood as: for each Session s: taken_quantity(s) ≤ s.seats
in other words: each session on its own: the total quantity taken from it is at most its seats
cost: per-group
```

Hover a kind's name, anywhere, to see its attributes with their types and marks, its parts, its effect and the rules about it. Hover an effect to see what its adapter receives and may answer, from the manifest (chapter 19).

While the text has errors, hover and completion use the last version of the module that compiled.

## Completion

Completion offers only what the grammar accepts at the cursor:

| After | Offered |
|---|---|
| an empty line | statement templates (`kind`, `effect`, `rule` …) and the program's kinds |
| `rule cap: every ` | the kinds, and the states that `define` names |
| `every Booking's ` | Booking's attributes, with their types |
| `its quantity is ` | `at most`, `at least`, `more than`, `less than`, `not` … |
| `via ` | the program's adapters |
| `card: Text ` | `secret`, `personal` |

A word is offered when the statement, with that word added, either parses or stops only at its end, wanting more. The words tried are the parser's own, so the list cannot fall behind the language. Nothing is guessed, and no model ranks anything.
