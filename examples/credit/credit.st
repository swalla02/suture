-- A credit check (spec appendix I.4): an order's payment goes out only when
-- the customer's credit, fetched from a bureau, covers its total. The
-- credit is fetched when a step needs it, trusted for a day, and must be at
-- most an hour old when the payment goes out.
module credit version 1, currency EUR

role staff

kind Customer { name: Text }
kind Product  { name: Text }
kind Stock    { product: Product, units: Count }
kind Order    { customer: Customer, product: Product, quantity: Count, total: Money,
                credit: Money, approved: Bool = no, cancelled: Bool = no }
kind Payment  { order: Order, amount: Money = order's total }
kind Shipment { order: Order }

Order    needs Payment, Shipment while it is not cancelled
Shipment uses  a Stock where the Stock's product is the Order's product,
               taking the Order's quantity

effect capture Payment via payments within 30 seconds
  sends    its amount
  marks    sent
  reports  captured | fails with declined
effect send Shipment via carriers within 7 days
  sends    the Order's quantity
  marks    sent
  reports  delivered

source Order's credit via bureau within 5 seconds
  sends      the customer's name
  reports    found | fails with unknown
  good for   1 day
  otherwise  use 0 EUR

rule credit:   no Order has a Payment that is sent unless its total is at most its credit,
               with credit at most 1 hour old
rule approved: no Order has a Payment that is sent unless it is approved
rule stock:    the total quantity taken from each Stock is at most its units
rule paid:     no Order has a Shipment that is sent unless its Payment is captured

define: an Order is delivered when its Shipment is delivered
desired within 3 days: every Order that is not cancelled is delivered

when a Payment is declined:  its Order is cancelled
when a staff cancels an Order: the Order is cancelled

prefer Stock: highest units

admit place a new Order by staff
admit change Order's approved by staff
admit cancel Order by staff
admit add a new Customer, Product or Stock by staff
