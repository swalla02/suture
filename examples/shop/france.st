-- France only: a customs form, and an excise section when the goods are alcohol.
kind CustomsForm { order: Order }
kind Declaration { form: CustomsForm }
kind Excise      { form: CustomsForm }

define: an Order is dutiable when its country's code is "FR"

Order       needs CustomsForm while it is dutiable
CustomsForm needs Declaration
CustomsForm needs Excise while its Declaration's contents is alcohol

effect ask Declaration via forms within 3 days
  sends    the form's order's total
  reports  filled with value: Money, contents: Text
effect ask Excise via forms within 3 days
  sends    the form's order's country's code
  reports  filled with licence: Text

rule declared: every Declaration's value is at most its form's order's total
rule licence:  every Excise's licence is not ""
rule hold:     no dutiable Order has a Shipment that is sent unless its CustomsForm is done
