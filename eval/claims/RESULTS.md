# Claims: Suture against the usual way

The first measured comparison (plan M7). One workflow, motor insurance claims with a model that looks at a photo (`REQUIREMENTS.md`), written twice:

- `suture/claims.st`, run by the Suture runtime;
- `baseline/src/lib.rs`, plain Rust: state structs, command handlers and a `progress` function, the way a careful engineer writes a service core.

Both pass the same thirteen scenarios (`suture/claims.scenario`, `baseline/tests/scenarios.rs`). Then the same two changes were made to both, one commit each. `python3 count.py ⟨commit⟩` does the counting at any commit, and the numbers below come from it and from `git show --numstat`.

## Numbers

| | Suture | Baseline |
|---|---|---|
| First version: lines of logic | 47 | 378 |
| First version: control-flow statements (`if`, `else`, `match`, `for`, `while`, `loop`, `return`) | none: the language has none | 77 |
| First version: condition and fallback clauses (`unless`, `where`, `whose`, `while`, `otherwise`, `after`, `fails with`) | 13 | (inside the 77) |
| Change 1, a yearly payout cap per policy: lines added / removed | **+1 −0** | +43 −0 |
| Change 2, cash as a third outcome: lines added / removed | +11 −2 | **+5 −4** |
| After both: lines of logic | 57 | 415 |
| Scenarios or tests, after both | 148 lines | 192 lines |

The baseline does less than the Suture version. The following come with the Suture runtime and are not in its 415 lines:

- a durable log, with deterministic replay after a restart, model answers included;
- an HTTP service, the posting of requests to adapters with retries, and pods;
- `suture deploy`, which checks a new version against the live claims (except totals such as the yearly cap: chapter 16);
- read-backs of every rule, explanations of every claim, and the list of open choices;
- the IBAN shown as `(secret)` in every trace and explanation, and a compile-time check that only the bank receives it. The baseline keeps it out of the model and the journal by hand, and its `Request` derives `Debug` with the IBAN in it;
- a deadline on the model (`decide … within`). The baseline's model call is synchronous, with no timeout;
- the yearly cap and the garage bays kept as running totals. The baseline sums every claim of the policy at each step.

On a real stack, most of this would come from a database, Temporal and more code.

## What we learned about Suture

**Where it wins.** A rule is one sentence, so a new rule costs one more sentence. Change 1 cost one line in Suture. The baseline needed a new field, a counting function, a check in `progress`, and a calendar function (`year_of`) to find a UTC year. There is no control flow to read or get wrong, and the compiler prints how it understood each rule.

**Where it loses.** Change 2 cost Suture 11 lines and the baseline 5. In Rust, the payout checks were written once for any payout, so `Kind::Writeoff | Kind::Cash` and one more entry in `PREFER` covered the new outcome. Suture's rules name one kind, so the limit, approval and fraud rules had to be copied for `Cash`. If one of those copies is forgotten, cash is paid past the limit with no error. A rule should be able to name the slot instead: `no Claim has an Outcome that is sent unless …`.

**A bug the evaluation found in Suture.** A withdrawn claim keeps asking the fraud bureau for its score: three sends, then the fallback. Requirement 10 says it asks nothing more. The baseline does stop, and its test checks that. The Suture scenario only checks that the bay is given back. A source should not be asked for a unit whose needs are gone.

**The edge cases were decided by Suture, not by the requirements.** The first draft of the requirements left open several cases:
- what happens to a booking that went out but was never answered;
- whether a resend is checked against the rules again;
- what a refused payment leads to;
- which outcome follows a failed one.

Suture already had an answer for each, from its general rules:
- a part that went out is never given up for patience;
- a resend is the same request;
- a failure is a new attempt, up to `attempts`;
- then the next alternative in `prefer` order that has not been tried.

The first baseline answered all of them differently, and nothing caught that until review. Aligning the baseline took an attempt counter, the list of outcomes tried, a `send` function, and about 35 more lines. That is a strength of the language: one consistent policy everywhere, written nowhere. It is also a risk: the behaviour is implicit, and the author of the requirements had not chosen it.

**What tripped up the author** while writing the scenarios. The author knows the language well, which makes these findings more telling:

1. A model's pick has to be scripted *before* the input that asks the model. Otherwise `prefer` has already chosen.
2. A source is asked only when a step needs it (appendix I). Three scenarios expected a fraud request that never came, because another rule held the step first, or because no garage was free yet.
3. A stale score is asked for again on its own when it expires. A scenario that let 25 hours pass saw that request time out, and the fallback score of 100 was used.
4. `taking` needs an attribute, so `Repair` carries a made-up `bays: Count = 1`.
5. A withdrawn claim drops its Outcome, so `expect C1's Writeoff is not sent` is an error rather than true.

**What the baseline got wrong.** Its first draft reset the send counter at every timeout, so a booking that nobody answered would have been sent forever. It also kept asking the bureau after a withdrawal. Neither was caught by a scenario; both were found by reading the code.

## After slice 22

Slice 22 (spec appendix W) made the fixes this evaluation asked for, and `claims.st` was rewritten to use them. With the same thirteen scenarios, it is now 51 lines instead of 57:

- `taking 1` replaces the made-up `bays: Count = 1` on `Repair`;
- `rule fraud` names the slot, `no Claim has an Outcome that is sent unless …`, so it covers Repair, Writeoff and Cash at once;
- `rule limit` and `rule big` name `a Writeoff or a Cash`, which replaces the three copied `cash…` rules.

Change 2 made against this version would cost **+9 −4**, against the baseline's +5 −4:

- `kind Cash` (+1);
- `effect pay Cash`, a new adapter call (+4);
- `Outcome is … or Cash` and `prefer …, then Cash` (+2 −2);
- `or a Cash` in `limit` and `big` (+2 −2).

The fraud rule needs no change. That is better than +11 −2, but still more than plain Rust. It was estimated at about 5 lines before the rewrite, and the measured number is higher. The remaining cost is the effect, which is real work in both versions: the baseline already had a payment call to reuse. Forgetting `or a Cash` in `limit` is still possible, and it still pays past the limit with no error. A rule written over the slot from the start (`… unless … or it has a Repair`) avoids that, but it reads worse.

The bug is fixed: a withdrawn claim's fetch stops at its next timeout (`score: no step waits on it any more: stopped asking fraud`). The read-back now says when the bureau is asked, `asked only when a Repair, a Writeoff or a Cash would be sent`. It also prints the policy each slot moves on by: attempts, patience, the next alternative not yet tried, and that a part that went out is never given up for time alone.

## How far to trust this

- One author wrote both versions, and that author designed Suture. The baseline is careful but small: one file, no storage, no HTTP.
- Lines and branches are a stand-in for effort and risk, not a measure of them.
- The scenarios were written in Suture first, and then ported. The two versions behave the same as far as these thirteen scenarios reach, and no further.
- Neither side tests:
  - a bureau that times out three times;
  - a resent payment;
  - a garage that answers `full`;
  - a bay given back on a switch to a write-off;
  - a late answer;
  - that the IBAN reaches only the bank.
- This is one workflow and two changes. A third party porting a workflow they already run would be much stronger evidence.

## What it suggests next

1. ~~Stop sourcing for units whose needs are gone (the bug above).~~ Slice 22.
2. ~~Rules over a slot, so that an alternative added to a slot is under the same rules.~~ Slice 22. This was the one change where Suture lost, and it still loses, by less.
3. ~~`taking` a constant, such as `taking 1`.~~ Slice 22, for counts.
4. ~~Say when a source is asked: in the read-back, and in the explanation of a step that waits for it.~~ Slices 22 and 23.
5. ~~Print the policies the runtime applies to every slot in the read-back.~~ Slice 22.
6. Then the same comparison on a workflow from a design partner, written by someone who did not design the language.
