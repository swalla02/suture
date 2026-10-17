-- Ticketing: fans book seats for a show; the system charges them, picks a
-- session with room, and mails the ticket. Nothing here says in which order.
module tickets version 1, currency EUR

role fan is a Fan
role box

kind Fan     { name: Text }
kind Show    { name: Text, price: Money }
kind Session { show: Show, day: Text, seats: Count }
kind Booking { fan: Fan, show: Show, quantity: Count,
               total: Money = show's price * quantity,
               cancelled: Bool = no }
kind Payment { booking: Booking, amount: Money = booking's total }
kind Ticket  { booking: Booking }

Booking needs Payment, Ticket while it is not cancelled
Ticket  uses  a Session where the Session's show is the Booking's show,
              taking the Booking's quantity

effect charge Payment via payments within 30 seconds
  sends    its amount
  reports  charged | fails with declined
effect issue Ticket via mailer within 10 minutes
  sends    the booking's fan's name
  marks    sent
  reports  issued

define: a Booking is paid when its Payment is charged
define: a Booking is issued when its Ticket is issued

rule seats: the total quantity taken from each Session is at most its seats
rule early: no Booking has a Ticket that is sent unless it is paid

desired within 1 day: every Booking that is not cancelled is paid and issued

when a Payment is declined: its Booking is cancelled
when a fan or box cancels a Booking: the Booking is cancelled

prefer Session: highest seats

admit book a new Booking by fan where the Booking's fan is the caller
admit cancel Booking by fan where the Booking's fan is the caller
admit cancel Booking by box
admit add a new Show or Session by box
admit change Session's seats by box
