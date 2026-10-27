-- RuleArena's airline bag fees (eval/rules/airline/data/reference_rules.txt):
-- the fee of each checked bag, and which bags ride on a free allowance.
--
-- The harness enters, in this order:
--   1. the checked-bag fee table as rows, Rates (data/fee_tables), and the
--      regions;
--   2. a case's Trip and its checked Bags, in the order the case lists them
--      (the first item is the carry-on, which is never charged); each bag
--      gets its row of the checked-bag fees;
--   3. three Allowances, nth 1 to 3, since "you won't have to pay the fees
--      for up to 3 bags": the free ones go to the bags they save most on,
--      and one with no bag to save on waits.
-- The answer is the ticket plus each bag's fee, read with Host::value.
module airline version 2, currency USD, candidates 16

role clerk

-- The overweight and oversize fees of the policy text that depend on where
-- the trip goes.
kind Region {
  name: Text,
  -- "Over 65 in. … up to 115 in.": $150 for these, $200 elsewhere.
  big: Money = 150 USD if its name is "Panama" or its name is "South America" or its name is "Peru" or its name is "Colombia"
    or its name is "Ecuador" or its name is "Europe" or its name is "Israel" or its name is "Qatar", otherwise 200 USD,
  -- "Over 53 lbs … up to 70 lbs": $200 to Cuba, $100 elsewhere.
  heavy: Money = 200 USD if its name is "Cuba", otherwise 100 USD,
  -- "Over 70 lbs … up to 100 lbs": $450 to these, $200 elsewhere.
  over: Money = 450 USD if its name is "India" or its name is "China" or its name is "Japan" or its name is "South Korea"
    or its name is "Hong Kong", otherwise 200 USD,
  far: Bool = yes if its name is "Australia" or its name is "New Zealand", otherwise no
}

kind Trip {
  region: Region,
  class: Text,
  way: Text,
  ticket: Money,
  -- How many free bags may weigh 70 lbs: "For First / Business, weight is
  -- 70 lbs / 32 kgs for complimentary bags", and to or from Australia or
  -- New Zealand in every class. These are the bags whose row of the fee
  -- table is $0: the 1st and 2nd in Business and First, the 3rd in First
  -- outside the U.S., Puerto Rico and Canada, and to or from Australia or
  -- New Zealand the 1st in Main Cabin and the 1st and 2nd in Main Plus and
  -- Premium Economy. Elsewhere a free bag keeps the 50 lbs allowance.
  free: Count = 3 if its class is "First" and its region's name is not "U.S." and its region's name is not "Puerto Rico" and its region's name is not "Canada",
    otherwise 2 if its class is "Business" or its class is "First",
    otherwise 1 if its region is far and its class is "Main Cabin",
    otherwise 2 if its region is far and (its class is "Main Plus" or its class is "Premium Economy"),
    otherwise 0
}

kind Bag {
  trip: Trip,
  nth: Count,
  length: Count,
  width: Count,
  height: Count,
  weight: Count,
  -- "adding the total outside dimensions of each bag, length + width + height"
  size: Count = length + width + height,
  oversize: Money = 0 USD if its size is at most 62, otherwise 30 USD if its size is at most 65, otherwise its trip's region's big,
  overweight: Money = 0 USD if its weight is at most 50, otherwise 30 USD if its weight is at most 53,
    otherwise its trip's region's heavy if its weight is at most 70, otherwise its trip's region's over,
  -- The same on a free allowance of 70 lbs.
  allowed: Money = 0 USD if its weight is at most 70, otherwise its trip's region's over,
  -- "The higher fee between oversize and overweight applies, in addition to
  -- the checked bag fee", paid and on the allowance, and what it saves.
  charge: Money = the larger of its oversize and its overweight,
  grace: Money = the larger of its oversize and its allowed,
  saving: Money = its charge - its grace
}
-- The checked-bag fee of the nth bag; nth 4 stands for the 4th and after.
kind Rate { region: Region, class: Text, way: Text, nth: Count, amount: Money }

kind Base      { bag: Bag }
kind Allowance { trip: Trip, nth: Count }
kind Waiver    { allowance: Allowance }

Bag needs Base
Allowance needs Waiver

-- The nth checked bag pays the fee table's row for the nth bag. Which bag
-- is nth does not change the total, since the rows depend on nth alone.
Base uses a Rate where the Rate's region is the Bag's trip's region
  and the Rate's class is the Bag's trip's class and the Rate's way is the Bag's trip's way
  and (the Rate's nth is the Bag's nth or (the Rate's nth is 4 and the Bag's nth is more than 4))
prefer Rate: lowest nth

-- The free allowance goes where it saves the most. A bag it saves nothing
-- on needs none, and an Allowance past the trip's free ones waits.
Waiver uses a Bag where the Bag's trip is the Allowance's trip
  and the Allowance's nth is at most the Allowance's trip's free and the Bag's saving is more than 0 USD,
  taking 1
prefer Bag: highest saving
rule once: the total waivers taken from each Bag is at most 1

define: a Bag is waived when the total waivers taken from it is at least 1
define: a Bag's fee is its Base's Rate's amount + (its grace if it is waived, otherwise its charge)

admit add a new Region, Trip, Bag, Rate or Allowance by clerk
