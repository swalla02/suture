# Returns: what is ported from Solidus

The second comparison (after eval/claims). The baseline is real code that someone else wrote: the returns flow of [Solidus](https://github.com/solidusio/solidus), an open-source Rails shop, under `core/app/models/spree/`. The port is `suture/returns.st`, and its scenarios come from Solidus's specs (`core/spec/models/spree/return_item_spec.rb`, `reimbursement/reimbursement_type_engine_spec.rb`, `return_authorization_spec.rb`, `return_item/eligibility_validator/*_spec.rb`).

The port follows Solidus at `2741034ae^`, the parent of the first change replayed below. Between that commit and `bafb57f` (2026-10-19), the code it covers changes only by change 1 below, and by three deleted lines in the refund calculator. Change 2 is older and is replayed from 2015.

## What Solidus does, and the port does too

Each line names the Solidus code that does it.

1. **Authorizing a return.** Staff create a return authorization (RMA) for an order that has shipped units, at a stock location (`return_authorization.rb`, `must_have_shipped_units`). Its return items each name one inventory unit.
2. **The amount.** A return item's amount is its line's total before tax divided by the quantity, plus a share of the order's other adjustments, weighted by that amount (`calculator/returns/default_refund_amount.rb`). The port takes the unit's price and its `discount` as given (below).
3. **Reception.** A return item starts `awaiting` and may be `received`, `given_to_customer`, `lost_in_transit`, `shipped_wrong_item` or `short_shipped`, each only from `awaiting`. It may be `received` later from any of those, and it may `expire` (`state_machines/return_item/reception_status.rb`).
4. **Acceptance on reception.** When an item reaches one of those states, it is checked (`acceptance_status.rb`, `attempt_accept`, and `eligibility_validator/*`, in order, stopping at the first that fails):
   - the order must be complete, and fewer than 365 days must have passed since it completed, or the item is rejected;
   - the unit must have shipped, or the item waits for staff to decide (`manual_intervention_required`);
   - otherwise it is accepted.
5. **Staff decide.** Staff may accept or reject an item at any time, except one already rejected. An item accepted before it arrives stays accepted, and one waiting for staff is accepted or rejected by them.
6. **Restocking.** A received item is restocked when it is resellable, its variant tracks inventory, and the stock location restocks (`return_item.rb`, `should_restock?`). An item given to the customer or lost is not restocked.
7. **Reimbursing.** Staff reimburse accepted items only (`validate_acceptance_status_for_reimbursement`). The payment is chosen by `reimbursement_type_engine.rb` and `reimbursement_type_validator.rb`:
   - with no preference: the original payment is refunded, or, after 90 days since shipping, the item is paid in store credit;
   - with a preference: that type, if it is valid. A preferred refund is not valid after 90 days, and then the item is not paid at all.
8. **Cancelling an RMA** cancels its items that are still awaiting. They can then no longer be received (`return_authorization.rb`, `cancel_return_items`). A cancelled RMA stays cancelled.

## What the port leaves out

These are left out on both sides, and the counter (`count.py`) skips Solidus's lines and methods for them by name and pattern:

- exchanges;
- taxes;
- simulating a reimbursement;
- splitting a refund over several payments;
- the tolerance check, and the `errored` state it leads to;
- the override reimbursement type;
- display, generated numbers and admin search.

The filter is coarse: a few lines either way may be miscounted. `customer_return.rb` is counted whole, although the port folds it into receiving.

## Where the port differs, because of Suture v0

Each of these is a finding (RESULTS.md):

- **A second return item for the same unit.** Solidus cancels the other active return items for that unit (`cancel_others`). It also refuses a second completed one (`validate_no_other_completed_return_items`), and treats a unit with an earlier reimbursed item as ineligible (`no_reimbursements.rb`). All three read other return items of the unit. Suture cannot express any of them: each return item is its own unit, and v0 has no `that` filter on a relation across units (guide chapter 16).
- **The order has shipped units.** The port carries this as a given fact on the order (`shipped`), because an RMA cannot look at the order's units across units.
- **The promotion share.** Solidus computes it as order adjustments × the unit's price ÷ the order's item total. Suture cannot multiply Money by Money, so each unit carries its `discount`.
- **Times.** There is no Time literal in scenarios, so an Order is added when it completes and a Unit when it ships, and `created` stands for those moments.
- **CustomerReturn** is folded into receiving. Solidus groups received items into a customer return at a stock location; the port uses the RMA's location.
- **Cancelling an RMA.** Solidus refuses it when one of its items cannot be cancelled. The port cancels the items still awaiting and leaves the others, because the gate cannot read the RMA's items in other units.
- **A refund that fails** is tried `attempts` times, then paid in store credit unless a refund was preferred, by Suture's slot policy. In Solidus, the reimbursement becomes `errored` and staff retry it.

## The changes replayed

Two real changes from Solidus's history, made to both sides in the same order:

1. `2741034ae` (2026-10-04), *Validate return item unit is on the RMA's order*. A return item whose unit belongs to another order is refused.
2. `65c411211` (2015-03-24), *Add 'in-transit' intermediate return item status*. A new reception state, `in_transit`, which counts as back like the others. (`69a07e0d1`, 2015-08-20, re-added its transition after the change had been reverted and applied again.)

The port's first version has neither. Solidus's first version is `2741034ae^`, which already has `in_transit`, so its cost for change 2 is the 2015 commit's diff.
