-- RuleArena's airline bag fees (eval/rules/airline/data/reference_rules.txt):
-- the fee of each checked bag, and which bags ride on a free allowance.
--
-- The harness enters, in this order:
--   1. the fee tables as rows: Rates (data/fee_tables, the checked-bag
--      fees) and Charges and Graces (charges.csv, the overweight and
--      oversize tables of the policy text, one row per region of a group:
--      a granted row is carried into the bag with what it refers to);
--   2. a case's Trip and its checked Bags, in the order the case lists them
--      (the first item is the carry-on, which is never charged); each bag
--      gets its row of the checked-bag fees;
--   3. three Allowances, nth 1 to 3, since "you won't have to pay the fees
--      for up to 3 bags": the free ones go to the bags they save most on,
--      and one left with no bag waits;
--   4. a weigh of each Bag, which then gets its overweight or oversize row.
-- The answer is the ticket plus the amounts of the rows each part was
-- granted; a part that waits granted nothing, and the case has no answer.
module airline version 1, currency USD, candidates 16

role clerk

kind Region { name: Text }
kind Trip   { region: Region, class: Text, way: Text, ticket: Money }
kind Bag {
  trip: Trip,
  nth: Count,
  length: Count,
  width: Count,
  height: Count,
  weight: Count,
  -- "adding the total outside dimensions of each bag, length + width + height"
  size: Count = length + width + height,
  weighed: Bool = no
}
-- The checked-bag fee of the nth bag; nth 4 stands for the 4th and after.
kind Rate { region: Region, class: Text, way: Text, nth: Count, amount: Money }
-- An overweight or oversize fee for a bag between the bounds (over the
-- low one, up to the high one): Charges for bags that are paid for,
-- Graces for bags on a free allowance.
kind Charge { region: Region, lowsize: Count, highsize: Count, lowweight: Count, highweight: Count, amount: Money }
kind Grace  { region: Region, lowsize: Count, highsize: Count, lowweight: Count, highweight: Count, amount: Money }

kind Base      { bag: Bag }
-- A free place, one unit each: three in one would not fit.
kind Allowance { trip: Trip, nth: Count }
kind Waiver    { allowance: Allowance }
kind Paid    { bag: Bag }
kind Free    { bag: Bag }

Bag needs Base
Allowance needs Waiver

-- The nth checked bag pays the fee table's row for the nth bag. Which bag
-- is nth does not change the total, since the rows depend on nth alone.
Base uses a Rate where the Rate's region is the Bag's trip's region
  and the Rate's class is the Bag's trip's class and the Rate's way is the Bag's trip's way
  and (the Rate's nth is the Bag's nth or (the Rate's nth is 4 and the Bag's nth is more than 4))

-- "For First / Business, weight is 70 lbs / 32 kgs for complimentary bags",
-- and to or from Australia or New Zealand in every class. The nth
-- Allowance frees a bag where the fee table's row for the nth bag is $0
-- and the 70 lbs allowance applies: the 1st and 2nd in Business and First,
-- the 3rd in First outside the U.S., Puerto Rico and Canada, and to or from
-- Australia or New Zealand the 1st in Main Cabin and the 1st and 2nd in
-- Main Plus and Premium Economy. Elsewhere a free bag
-- keeps the 50 lbs allowance, so no Waiver is needed and that Allowance waits.
Waiver uses a Bag where the Bag's trip is the Allowance's trip
  and ((the Bag's weight is more than 53 and the Bag's weight is at most 70 and the Bag's size is at most 65)
    or (the Bag's weight is more than 50 and the Bag's weight is at most 53 and the Bag's size is at most 62))
  and ((the Allowance's nth is at most 2 and (the Allowance's trip's class is "Business" or the Allowance's trip's class is "First"))
    or (the Allowance's nth is 3 and the Allowance's trip's class is "First" and the Allowance's trip's region's name is not "U.S." and the Allowance's trip's region's name is not "Puerto Rico" and the Allowance's trip's region's name is not "Canada")
    or ((the Allowance's trip's region's name is "Australia" or the Allowance's trip's region's name is "New Zealand")
      and ((the Allowance's nth is 1 and the Allowance's trip's class is "Main Cabin")
        or (the Allowance's nth is at most 2 and (the Allowance's trip's class is "Main Plus" or the Allowance's trip's class is "Premium Economy"))))),
  taking 1
rule once: the total waivers taken from each Bag is at most 1

define: a Bag is heavy when its weight is more than 53 and its weight is at most 70
define: a Bag is plump when its weight is more than 50 and its weight is at most 53
define: a Bag is snug when its size is at most 62
define: a Bag is close when its size is more than 62 and its size is at most 65
define: a Bag is waived when the total waivers taken from it is at least 1

-- The free allowance goes where it saves the most: the higher of the
-- overweight and oversize fees, paid, less the same on the allowance.
-- Heavy and snug saves 100 (200 to Cuba), heavy and close 70 (170), plump
-- and snug 30; every other bag saves nothing, and the Waiver's condition
-- leaves it out: an Allowance choosing among ten bags would not fit its unit.
prefer Bag: those that are heavy and snug, then those that are heavy and close, then those that are plump and snug

-- "The higher fee between oversize and overweight applies, in addition to
-- the checked bag fee": a bag matches one size row and one weight row, and
-- takes the higher.
Bag needs Paid while it is weighed and it is not waived
Bag needs Free while it is weighed and it is waived
Paid uses a Charge where the Charge's region is the Bag's trip's region
  and the Bag's size is more than the Charge's lowsize and the Bag's size is at most the Charge's highsize
  and the Bag's weight is more than the Charge's lowweight and the Bag's weight is at most the Charge's highweight
Free uses a Grace where the Grace's region is the Bag's trip's region
  and the Bag's size is more than the Grace's lowsize and the Bag's size is at most the Grace's highsize
  and the Bag's weight is more than the Grace's lowweight and the Bag's weight is at most the Grace's highweight
prefer Charge: highest amount
prefer Grace: highest amount
prefer Rate: lowest nth

when a clerk weighs a Bag: the Bag is weighed

admit add a new Region, Trip, Bag, Rate, Charge, Grace or Allowance by clerk
admit weigh Bag by clerk
