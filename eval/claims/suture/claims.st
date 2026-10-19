-- Motor claims: a policyholder files a claim with a photo of the damage. A
-- model looks at the photo and picks a repair at a garage or a write-off,
-- and every payout stays within the policy and past the fraud check.
module claims version 1, currency EUR

role holder is a Holder
role adjuster
role ops

kind Holder   { name: Text }
kind Policy   { holder: Holder, region: Text, limit: Money, deductible: Money, iban: Text secret }
kind Garage   { name: Text, region: Text, bays: Count }
kind Claim    { policy: Policy, photo: Media, damage: Text, amount: Money, score: Count,
                approved: Bool = no, withdrawn: Bool = no,
                payout: Money = amount - policy's deductible }
kind Repair   { claim: Claim, bays: Count = 1 }
kind Writeoff { claim: Claim, amount: Money = claim's payout }
kind Cash     { claim: Claim, amount: Money = claim's payout }

Claim   needs Outcome while it is not withdrawn
Outcome is    Repair or Writeoff or Cash
Repair  uses  a Garage where the Garage's region is the Claim's policy's region,
              taking its bays

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

rule bays:    the total bays taken from each Garage is at most its bays
rule limit:   no Claim has a Writeoff that is sent unless its payout is at most its policy's limit
rule big:     no Claim has a Writeoff that is sent unless its payout is at most 5000 EUR or it is approved
rule fraud:   no Claim has a Writeoff that is sent unless its score is less than 70 or it is approved,
              with score at most 1 day old
rule suspect: no Claim has a Repair that is sent unless its score is less than 70 or it is approved,
              with score at most 1 day old
rule cashlimit: no Claim has a Cash that is sent unless its payout is at most its policy's limit
rule cashbig:   no Claim has a Cash that is sent unless its payout is at most 5000 EUR or it is approved
rule cashfraud: no Claim has a Cash that is sent unless its score is less than 70 or it is approved,
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
