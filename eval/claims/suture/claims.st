-- Motor claims: a policyholder files a claim with a photo of the damage. A
-- model looks at the photo and picks a repair at a garage or a write-off,
-- another reads the repair cost off it, and every payout stays within the
-- policy, the reading and the fraud check.
module claims version 1, currency EUR

role holder is a Holder
role adjuster
role ops

kind Holder   { name: Text }
kind Policy   { holder: Holder, region: Text, limit: Money, deductible: Money, iban: Text secret }
kind Garage   { name: Text, region: Text, bays: Count }
kind Claim    { policy: Policy, photo: Media, damage: Text, amount: Money, score: Count, estimate: Money,
                approved: Bool = no, withdrawn: Bool = no,
                payout: Money = amount - policy's deductible }
kind Repair   { claim: Claim }
kind Writeoff { claim: Claim, amount: Money = claim's payout }
kind Cash     { claim: Claim, amount: Money = claim's payout }

Claim   needs Outcome while it is not withdrawn
Outcome is    Repair or Writeoff or Cash
Repair  uses  a Garage where the Garage's region is the Claim's policy's region,
              taking 1

effect book Repair via garages within 2 days
  sends    the claim's damage
  marks    sent
  reports  booked | fails with full
effect pay Writeoff via bank within 1 day
  sends    its amount, the claim's policy's iban
  marks    sent
  reports  paid | fails with refused
effect pay Cash via bank within 1 day
  sends    its amount, the claim's policy's iban
  marks    sent
  reports  paid | fails with refused

source Claim's score via fraud within 10 seconds
  sends      the policy's holder's name
  reports    scored | fails with unknown
  good for   1 day
  otherwise  use 100
source Claim's estimate via model "reader@1" within 10 seconds
  sends      its photo
  asking     "the repair cost the photo shows"
  at least 0.8 sure
  otherwise  ask again after 1 hour

rule bays:    the total repairs taken from each Garage is at most its bays
rule limit:   no Claim has a Writeoff or a Cash that is sent unless its payout is at most its policy's limit
rule big:     no Claim has a Writeoff or a Cash that is sent unless its payout is at most 5000 EUR or it is approved
rule photo:   no Claim has a Writeoff or a Cash that is sent unless its amount is at most its estimate or it is approved
rule fraud:   no Claim has an Outcome that is sent unless its score is less than 70 or it is approved,
              with score at most 1 day old
rule yearly:  for each Policy and year, the total payout of their Claims is at most 25000 EUR

define: a Claim is settled when its Outcome is done
desired within 14 days: every Claim that is not withdrawn is settled

when a holder withdraws a Claim: the Claim is withdrawn

prefer Outcome: Repair, then Writeoff after 2 days, then Cash
prefer Garage:  highest bays
decide Outcome with model "assessor@1"

admit file a new Claim by holder where the Claim's policy's holder is the caller
admit withdraw Claim by holder where the Claim's policy's holder is the caller
admit change Claim's approved by adjuster
admit add a new Holder, Policy or Garage by ops
allow bank to receive Policy's iban
