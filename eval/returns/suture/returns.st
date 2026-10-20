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
                seq: Count = the number of its unit's Items,
                reception: awaiting | received | given | lost | wrong | short | transit | expired,
                approved: Bool = no, refused: Bool = no, reimbursing: Bool = no }
kind Inspection { item: Item }
kind Refund   { item: Item, amount: Money = item's amount }
kind Credit   { item: Item, amount: Money = item's amount }
kind Restock  { item: Item }

define: an Item is back when it is not awaiting and it is not expired
define: an Item is superseded when the number of its unit's Items is more than its seq + 1
define: an Item is dropped when its rma is cancelled or it is superseded
define: an Item is cancelled when it is dropped and it is awaiting
define: an Item is open when it is awaiting and it is not cancelled
define: an Item is recent when its Inspection's created - its unit's order's created is less than 365 days
define: an Item is timely when its unit's order is complete and it is recent
define: an Item is reimbursed when its Payback is done
define: an Item is spent when it is reimbursed and it is not cancelled and it is not expired
define: an Item is clean when the number of its unit's Items that are spent is 0 or it is reimbursed
define: an Item is eligible when it is timely and its unit is shipped and it is clean
define: an Item is passed when it is back and it is eligible and it is not refused
define: an Item is failing when it is back and it is not timely and it is not approved
define: an Item is undecided when it is back and it is timely and it is not eligible and it is not approved and it is not refused
define: an Item is rejected when it is refused or it is failing
define: an Item is accepted when it is approved or it is passed

Item needs Inspection while it is back
Item needs Payback while it is accepted and it is reimbursing and it is not refused
Item needs Restock while it is received and its unit is resellable and its unit is tracked and its rma's location is restocks
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
rule once:    the number of each Unit's Items that are back is at most 1

prefer Payback: Refund, then Credit

admit authorize a new Rma by staff where the Rma's order is shipped
admit authorize a new Item by staff where the Item's rma is not cancelled and the Item's unit's order is the Item's rma's order and the number of the Item's unit's Items that are back is 0
admit receive Item by staff where the Item is not received and it is not expired and it is not cancelled
admit give Item by staff where the Item is open
admit lose Item by staff where the Item is open
admit mismatch Item by staff where the Item is open
admit shortship Item by staff where the Item is open
admit forward Item by staff where the Item is open
admit expire Item by staff where the Item is open
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
when a staff forwards an Item: the Item is transit
when a staff expires an Item: the Item is expired
when a staff accepts an Item: the Item is approved and the Item is not refused
when a staff rejects an Item: the Item is refused and the Item is not approved
when a staff reimburses an Item: the Item is reimbursing
