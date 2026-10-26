# Source

RuleArena (Zhou et al., ACL 2025, arXiv 2412.08972), https://github.com/skyriver-2000/RuleArena,
commit 59f60f6b4eb93a9b6d187ec781349d4f15618903, MIT licence (`LICENSE`).

Copied unchanged from its `airline/` folder: `reference_rules.txt` (the policy, as the paper's
prompts give it), `fee_tables/` (the checked-bag fees the answer script reads) and
`synthesized_problems/` as `cases/` (the 300 cases, levels 0–2). `SHA256SUMS` lists every file.

The cases carry no answers: RuleArena computes each one with its `compute_answer.py`. That script
was read, not run; `harness/src/reference.rs` is its rewrite in Rust.
