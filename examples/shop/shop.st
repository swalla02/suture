-- The shop, prototype slice 1: an order is paid, then shipped.
module shop version 1, currency EUR

kind Order    { country: Text, total: Money, cancelled: Bool = no }
kind Payment  { order: Order }
kind Shipment { order: Order }

Order needs Payment, Shipment while it is not cancelled

effect capture Payment via payments within 30 seconds
  sends    the order's total
  reports  captured | fails with declined
effect send Shipment via carriers within 7 days
  sends    the order's country
  marks    sent
  reports  delivered | fails with lost

rule paid: no Order has a Shipment that is sent unless its Payment is captured
rule late: no cancelled Order has a Shipment that is sent

when a Payment is declined: its Order is cancelled

define: an Order is paid when its Payment is captured
define: an Order is settled when its Shipment is delivered

desired within 3 days: every Order that is not cancelled is paid and settled
