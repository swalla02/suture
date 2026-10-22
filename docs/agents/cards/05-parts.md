# parts: needs, parts, slots, effects (work in the outside world) and pools

A need makes work. `A needs B` gives each A its own B, a **part**. The runtime creates it, keeps it while needed, retries it, and withdraws it when no longer needed. An effect is how a part reaches the outside world: a request to an adapter, with a deadline.

```suture
module visas version 1, currency EUR

role clerk

kind Applicant   { name: Text }
kind Application { applicant: Applicant, withdrawn: Bool = no }
kind Fee         { application: Application }
kind Courier     { application: Application }
kind Collection  { application: Application }

Application needs Fee, Return while it is not withdrawn
Return is Courier or Collection
prefer Return: Courier, then Collection after 2 days

effect charge Fee via payments within 1 hour
  sends    the application's applicant's name
  reports  paid | fails with declined
effect send Courier via couriers within 1 day
  sends    the application's applicant's name
  marks    dispatched
  reports  delivered

rule paid: no Application has a Courier that is dispatched unless its Fee is paid

when a clerk withdraws an Application: the Application is withdrawn

admit add a new Applicant by clerk
admit apply a new Application by clerk
admit withdraw Application by clerk
```

```scenario
scenario "the fee is charged, then the courier goes":
  given  a Applicant A with name "Ana"
  when   clerk Kim applies a new Application P1 with applicant A
  and    payments answers paid for P1
  and    couriers answers delivered for P1
  expect P1's Fee is paid
  expect P1's Courier is delivered
```

- `effect ⟨verb⟩ ⟨Kind⟩ via ⟨adapter⟩ within ⟨duration⟩` + `sends`, optional `marks ⟨state⟩` (true once sent), `reports ⟨outcome⟩ [with ⟨attr⟩: ⟨Type⟩] [| fails with ⟨outcome⟩]`. An outcome is a state of the part: `its Fee is paid`.
- Past the deadline, the request is sent again, up to `attempts` (module line, default 3); after the last it is `timed out`. React with `when a Fee is timed out: …`.
- A slot (`Return is Courier or Collection`) is one part of several kinds; `prefer` orders them, `after` gives each patience.
- Withdrawal is compensation: when the `while` turns false, an unfinished part is withdrawn and gives back what it took. A part whose request already went out stays and counts.
- A pool: `Hold uses a Title where the Title's name is the Loan's title, taking the Loan's copies` takes from a shared entity, with `rule stock: the total copies taken from each Title is at most its stock`.
- Scenarios play every adapter: `⟨adapter⟩ answers ⟨outcome⟩ [with …] for ⟨Name⟩`.
