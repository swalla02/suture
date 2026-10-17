-- The shop (spec §3.6): customers order products, which ship from stock in
-- a warehouse with a carrier that serves their country, or else are refunded.
module shop version 1, currency EUR

role customer is a Customer
role support
role staff

kind Country   { code: Text, restricted: Bool = no }
kind Customer  { name: Text, country: Country }
kind Product   { name: Text, price: Money }
kind Warehouse { code: Text, country: Country }
kind Carrier   { name: Text, countries: set of Country, fee: Money }
kind Stock     { warehouse: Warehouse, product: Product, units: Count }
kind Line      { order: Order, product: Product, quantity: Count, price: Money = product's price }
kind Order     { customer: Customer, lines: list of Line,
                 country: Country = customer's country,
                 total: Money = the total of the line's price * the line's quantity over its lines,
                 cancelled: Bool = no }
kind Payment   { order: Order, amount: Money = order's total }
kind Shipment  { order: Order }
kind Refund    { order: Order, customer: Customer = order's customer, amount: Money = order's total }

Order    needs Payment, Delivery while it is not cancelled
Delivery is    Shipment or Refund
Shipment uses  a Carrier where the Carrier's countries contain the Order's country
Shipment uses  a Stock per line where the Stock's product is the line's product,
               taking the line's quantity

effect capture Payment via payments within 30 seconds
  sends    its amount
  reports  captured | fails with declined
effect send Shipment via carriers within 7 days
  sends    the Order's country's code, its Carrier's name
  marks    sent
  reports  delivered | fails with lost
effect pay Refund via payments within 30 seconds
  sends    its amount
  reports  refunded

define: an Order is paid when its Payment is captured
define: an Order is settled when its Delivery is done

rule stock:   the total quantity taken from each Stock is at most its units
rule embargo: no Order whose country is restricted has a Shipment
rule paid:    no Order has a Shipment that is sent unless it is paid
rule refund:  no Order has a Refund unless it is paid
rule cap:     the total amount of each Order's Refunds is at most its total
rule late:    no cancelled Order has a Shipment that is sent
rule monthly: for each Customer and month, the total amount of their Refunds is at most 1000 EUR

desired within 3 days: every Order that is not cancelled is paid and settled

when a Payment is declined:                  its Order is cancelled
when a customer or support cancels an Order: the Order is cancelled

prefer Delivery:         Shipment, then Refund after 2 days
prefer Carrier:          lowest fee
prefer Stock for a line: those whose warehouse's country is the Order's country

admit place a new Order by customer where the Order's customer is the caller
admit cancel Order by customer where the Order's customer is the caller
admit cancel Order by support
admit add a new Country, Customer, Product, Warehouse, Carrier or Stock by staff
admit change Stock's units by staff
