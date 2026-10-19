-- Returns, after Solidus (github.com/solidusio/solidus at bafb57f): staff
-- authorize the return of shipped units, receive them, and pay them back to
-- the original payment, or in store credit once the refund window has passed.
-- An Order is added when it completes, and a Unit when it ships, so their
-- `created` is when that happened.
module returns version 1, currency EUR

role staff

kind Customer { name: Text }
kind Order    { customer: Customer, number: Text, complete: Bool = yes,
                shipped: Bool = yes }
kind Location { name: Text, restocks: Bool = yes }
kind Unit     { order: Order, number: Text, price: Money, discount: Money = 0 EUR,
                shipped: Bool = yes,
                resellable: Bool = yes, tracked: Bool = yes }
kind Rma      { order: Order, location: Location, cancelled: Bool = no }
kind Item     { rma: Rma, unit: Unit, preferred: Text = "none",
                amount: Money = unit's price - unit's discount,
                received: Bool = no, given: Bool = no, lost: Bool = no, wrong: Bool = no, short: Bool = no,
                expired: Bool = no, approved: Bool = no, refused: Bool = no, reimbursing: Bool = no }
kind Inspection { item: Item }
kind Refund   { item: Item, amount: Money = item's amount }
kind Credit   { item: Item, amount: Money = item's amount }
kind Restock  { item: Item }

define: an Item is back when it is received or it is given or it is lost or it is wrong or it is short
define: an Item is cancelled when its rma is cancelled and it is not back and it is not expired
define: an Item is awaiting when it is not back and it is not expired and it is not cancelled
define: an Item is recent when its Inspection's created - its unit's order's created is less than 365 days
define: an Item is timely when its unit's order is complete and it is recent
define: an Item is eligible when it is timely and its unit is shipped
define: an Item is passed when it is back and it is eligible and it is not refused
define: an Item is failing when it is back and it is not timely and it is not approved
define: an Item is undecided when it is back and it is timely and its unit is not shipped and it is not approved and it is not refused
define: an Item is rejected when it is refused or it is failing
define: an Item is accepted when it is approved or it is passed
define: an Item is reimbursed when its Payback is done

Item needs Inspection while it is back
Item needs Payback while it is accepted and it is reimbursing and it is not refused
Item needs Restock while it is received and its unit is resellable and its unit is tracked
Payback is Refund or Credit

effect refund Refund via payments within 1 day
  sends    its amount, the item's unit's order's number
  marks    sent
  reports  refunded | fails with declined
effect credit Credit via credits within 1 day
  sends    its amount, the item's unit's order's customer's name
  marks    sent
  reports  credited
effect restock Restock via stock within 1 day
  sends    the item's unit's number, the item's rma's location's name
  marks    sent
  reports  restocked

rule window:  no Item has a Refund that is sent unless its Refund's created - its unit's created is at most 90 days
rule wanted:  no Item has a Refund that is sent unless its preferred is not "credit"
rule kept:    no Item has a Credit that is sent unless its preferred is not "refund"
rule located: no Item has a Restock that is sent unless its rma's location is restocks

prefer Payback: Refund, then Credit after 1 minute

admit authorize a new Rma by staff where the Rma's order is shipped
admit authorize a new Item by staff where the Item's rma is not cancelled
admit receive Item by staff where the Item is not received and it is not expired and it is not cancelled
admit give Item by staff where the Item is awaiting
admit lose Item by staff where the Item is awaiting
admit mismatch Item by staff where the Item is awaiting
admit shortship Item by staff where the Item is awaiting
admit expire Item by staff where the Item is awaiting
admit accept Item by staff where the Item is not rejected
admit reject Item by staff where the Item is not rejected
admit reimburse Item by staff where the Item is accepted
admit change Rma's cancelled by staff where the Rma is not cancelled
admit add a new Customer, Order, Location or Unit by staff

when a staff receives an Item: the Item is received
when a staff gives an Item: the Item is given
when a staff loses an Item: the Item is lost
when a staff mismatches an Item: the Item is wrong
when a staff shortships an Item: the Item is short
when a staff expires an Item: the Item is expired
when a staff accepts an Item: the Item is approved and the Item is not refused
when a staff rejects an Item: the Item is refused and the Item is not approved
when a staff reimburses an Item: the Item is reimbursing
