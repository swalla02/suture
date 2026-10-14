-- The shop: an order is paid, then delivered: shipped, or else refunded.
module shop version 1, currency EUR

kind Order    { country: Text, total: Money, cancelled: Bool = no }
kind Payment  { order: Order }
kind Shipment { order: Order }
kind Refund   { order: Order }

Order    needs Payment, Delivery while it is not cancelled
Delivery is    Shipment or Refund

effect capture Payment via payments within 30 seconds
  sends    the order's total
  reports  captured | fails with declined
effect send Shipment via carriers within 7 days
  sends    the order's country
  marks    sent
  reports  delivered | fails with lost
effect pay Refund via payments within 30 seconds
  sends    the order's total
  reports  refunded

rule paid:   no Order has a Shipment that is sent unless its Payment is captured
rule refund: no Order has a Refund unless its Payment is captured
rule late:   no cancelled Order has a Shipment that is sent

when a Payment is declined: its Order is cancelled

define: an Order is paid when its Payment is captured
define: an Order is settled when its Delivery is done

desired within 3 days: every Order that is not cancelled is paid and settled

prefer Delivery: Shipment, then Refund after 2 days
