# Claims: the requirements both versions implement

Motor insurance claims, with a decision model in the loop. `suture/` and `baseline/` implement the same list, and pass the same scenarios (`suture/claims.scenario`, `baseline/tests/scenarios.rs`).

1. Operations staff add holders, policies (holder, region, limit, deductible, IBAN) and garages (region, bays).
2. A holder files a claim on their own policy, with a photo of the damage, a description and an amount. Nobody else may file on it, and only the holder may withdraw it.
3. A model looks at the photo and picks the outcome: a repair or a write-off. Without an answer from the model, the outcome is a repair. Every pick, by the model or by default, is recorded.
4. A repair is booked at a garage in the policy's region with a free bay, the one with the most bays first. A garage never has more repairs than bays. If no booking has gone out within 2 days (no garage had room, or a check held it), or the booking fails on its last attempt or after its last send, the claim is written off instead. A booking that went out is not given up for lack of time alone.
5. A write-off pays the amount less the deductible to the policy's IBAN.
6. No write-off is paid beyond the policy's limit, approved or not.
7. A write-off over 5000 EUR is paid only once an adjuster approves the claim.
8. Nothing is booked or paid unless the claim's fraud score is under 70, or an adjuster approved it. The score comes from a bureau when a booking or payment needs it, is trusted for a day, and must be at most a day old. If the bureau does not answer within 10 seconds, it is asked again, up to 3 times; if it fails, the score is 100.
9. A booking not answered within 2 days, or a payment within 1 day, is sent again as it was, up to 3 times. Having gone out, it is not checked again.
10. A claim not settled within 14 days raises an alert, unless it was withdrawn. A withdrawn claim stops, asks the bureau nothing more, and gives back its bay. A garage that answers full, or a bank that refuses, is tried again as a new attempt (a repair picks its garage anew), up to 3 attempts. An outcome that fails on its last attempt, or whose last send went unanswered, gives way to the next one not yet tried, repair first; when none is left, the claim fails.
11. The IBAN goes to the bank only: never to the model, the bureau or a log.

## The changes made to both, one commit each

1. **A yearly cap.** The claims filed on one policy in a calendar year (UTC, by filing time) pay out at most 25000 EUR together. A claim that would pass it waits. A withdrawn claim still counts.
2. **Cash.** A third outcome: the payout paid to the holder, under the same limit, approval and fraud checks as a write-off. The model may pick it, and it comes last among the outcomes to try when one fails.
