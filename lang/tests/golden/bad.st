rule g1: every Order must be settled
rule g2: no Order has a Refund if it isn't paid
rule g3: the sum of quantity taken from each Stock may not exceed its units
rule g4: the count of each Order's Shipments must be at most 2
rule g5: refunds of an Order can't be more than its total
rule g6: for each Customer and month, the sum of amount of their Refunds may not exceed 1000 EUR
rule g7: no Order whose country is restricted doesn't have a Shipment
rule g8: every Order's total is greater than 0 EUR
rule g9: Orders are frobbed
rule g10: no Order has a Shipment that isn't sent
define: an Order is closed when it is paid and it isn't cancelled
desired within 3 days: every Order that isn't cancelled must be paid
