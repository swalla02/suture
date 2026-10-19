//! The claims workflow of eval/claims/REQUIREMENTS.md, written the usual way:
//! state structs, command handlers, and a `progress` function that works out
//! what can happen next. Storage, HTTP and retries of the process itself are
//! left to the platform it would run on (a database, Temporal, …), so they are
//! not here; see eval/claims/RESULTS.md for what that leaves out.
//!
//! Money is in cents: `5000_00` is 5000.00 EUR.
#![allow(clippy::inconsistent_digit_grouping)]

use std::collections::BTreeMap;

pub const DAY: u64 = 86_400;
const FRAUD_WITHIN: u64 = 10;
const FRAUD_GOOD_FOR: u64 = DAY;
const FRAUD_FALLBACK: u32 = 100;
const SUSPECT: u32 = 70;
const BIG: i64 = 5000_00;
const BOOK_WITHIN: u64 = 2 * DAY;
const PAY_WITHIN: u64 = DAY;
const REPAIR_PATIENCE: u64 = 2 * DAY;
const SETTLE_WITHIN: u64 = 14 * DAY;
const SENDS: u32 = 3;
/// The outcomes, best first: one that fails gives way to the next not yet tried.
const PREFER: [Kind; 3] = [Kind::Repair, Kind::Writeoff, Kind::Cash];
const YEARLY: i64 = 25000_00;

#[derive(Clone, Debug, PartialEq)]
pub enum Role {
    Holder,
    Adjuster,
    Ops,
}

#[derive(Clone, Debug)]
pub struct Caller {
    pub role: Role,
    pub name: String,
}

pub struct Policy {
    pub holder: String,
    pub region: String,
    pub limit: i64,
    pub deductible: i64,
    pub iban: String,
}

pub struct Garage {
    pub name: String,
    pub region: String,
    pub bays: u32,
    pub taken: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Repair,
    Writeoff,
    Cash,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    Waiting,
    Sent { at: u64 },
    Done,
    Failed,
}

pub struct Outcome {
    pub kind: Kind,
    pub chosen_at: u64,
    pub garage: Option<String>,
    pub effect: Effect,
    pub sends: u32,
    pub attempt: u32,
}

pub struct Score {
    pub value: u32,
    pub at: u64,
}

pub struct Fetch {
    pub at: u64,
    pub sends: u32,
}

pub struct Claim {
    pub policy: String,
    pub photo: Vec<u8>,
    pub damage: String,
    pub amount: i64,
    pub approved: bool,
    pub withdrawn: bool,
    pub created: u64,
    pub score: Option<Score>,
    pub fetching: Option<Fetch>,
    pub outcome: Option<Outcome>,
    pub alerted: bool,
    /// The outcomes that failed, so as not to try them again.
    pub tried: Vec<Kind>,
    /// Whether its payout counts toward its policy's yearly total.
    pub counted: bool,
}

impl Claim {
    pub fn settled(&self) -> bool {
        matches!(&self.outcome, Some(o) if o.effect == Effect::Done)
    }
}

/// A request to an outside service.
#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    Fraud { claim: String, name: String },
    Book { claim: String, garage: String, damage: String },
    Pay { claim: String, amount: i64, iban: String },
}

/// What the model sees: the photo and the claim, never the IBAN.
pub struct ModelInput<'a> {
    pub photo: &'a [u8],
    pub damage: &'a str,
    pub amount: i64,
}

pub type Model = Box<dyn FnMut(&ModelInput) -> Option<Kind>>;

#[derive(Default)]
pub struct System {
    pub now: u64,
    pub holders: BTreeMap<String, String>,
    pub policies: BTreeMap<String, Policy>,
    pub garages: BTreeMap<String, Garage>,
    pub claims: BTreeMap<String, Claim>,
    pub outbox: Vec<Request>,
    pub alerts: Vec<String>,
    /// Every decision, for audit: who or what picked, and why.
    pub journal: Vec<String>,
    pub model: Option<Model>,
}

impl System {
    fn need(caller: &Caller, role: Role) -> Result<(), String> {
        if caller.role == role { Ok(()) } else { Err(format!("{} may not do that", caller.name)) }
    }

    pub fn add_holder(&mut self, caller: &Caller, id: &str, name: &str) -> Result<(), String> {
        Self::need(caller, Role::Ops)?;
        self.holders.insert(id.into(), name.into());
        Ok(())
    }

    pub fn add_policy(&mut self, caller: &Caller, id: &str, p: Policy) -> Result<(), String> {
        Self::need(caller, Role::Ops)?;
        if !self.holders.contains_key(&p.holder) {
            return Err(format!("no holder {}", p.holder));
        }
        self.policies.insert(id.into(), p);
        Ok(())
    }

    pub fn add_garage(&mut self, caller: &Caller, id: &str, name: &str, region: &str, bays: u32) -> Result<(), String> {
        Self::need(caller, Role::Ops)?;
        self.garages.insert(id.into(), Garage { name: name.into(), region: region.into(), bays, taken: 0 });
        self.progress_all();
        Ok(())
    }

    pub fn file_claim(&mut self, caller: &Caller, id: &str, policy: &str, photo: Vec<u8>, damage: &str, amount: i64) -> Result<(), String> {
        Self::need(caller, Role::Holder)?;
        let p = self.policies.get(policy).ok_or(format!("no policy {policy}"))?;
        if p.holder != caller.name {
            return Err(format!("{} does not hold {policy}", caller.name));
        }
        if self.claims.contains_key(id) {
            return Err(format!("{id} exists"));
        }
        let picked = match self.model.as_mut() {
            Some(m) => m(&ModelInput { photo: &photo, damage, amount }),
            None => None,
        };
        let kind = match picked {
            Some(k) => {
                self.journal.push(format!("{id}: {k:?} by the model"));
                k
            }
            None => {
                self.journal.push(format!("{id}: Repair by default (the model gave no answer)"));
                Kind::Repair
            }
        };
        let outcome = Outcome { kind, chosen_at: self.now, garage: None, effect: Effect::Waiting, sends: 0, attempt: 1 };
        let claim = Claim {
            policy: policy.into(),
            photo,
            damage: damage.into(),
            amount,
            approved: false,
            withdrawn: false,
            created: self.now,
            score: None,
            fetching: None,
            outcome: Some(outcome),
            alerted: false,
            tried: vec![],
            counted: false,
        };
        self.claims.insert(id.into(), claim);
        self.progress(id);
        Ok(())
    }

    pub fn withdraw(&mut self, caller: &Caller, id: &str) -> Result<(), String> {
        Self::need(caller, Role::Holder)?;
        let c = self.claims.get(id).ok_or(format!("no claim {id}"))?;
        if self.policies[&c.policy].holder != caller.name {
            return Err(format!("{} does not hold {id}", caller.name));
        }
        let c = self.claims.get_mut(id).unwrap();
        c.withdrawn = true;
        c.fetching = None;
        if let Some(o) = c.outcome.take()
            && let Some(g) = o.garage
        {
            self.garages.get_mut(&g).unwrap().taken -= 1;
        }
        self.progress_all();
        Ok(())
    }

    pub fn set_approved(&mut self, caller: &Caller, id: &str, approved: bool) -> Result<(), String> {
        Self::need(caller, Role::Adjuster)?;
        self.claims.get_mut(id).ok_or(format!("no claim {id}"))?.approved = approved;
        self.progress(id);
        Ok(())
    }

    pub fn fraud_answers(&mut self, id: &str, score: Option<u32>) {
        let Some(c) = self.claims.get_mut(id) else { return };
        if c.fetching.take().is_none() {
            return; // late or unasked: ignored
        }
        c.score = Some(Score { value: score.unwrap_or(FRAUD_FALLBACK), at: self.now });
        self.progress(id);
    }

    pub fn garage_answers(&mut self, id: &str, booked: bool) {
        let Some(o) = self.claims.get_mut(id).and_then(|c| c.outcome.as_mut()) else { return };
        if o.kind != Kind::Repair || !matches!(o.effect, Effect::Sent { .. }) {
            return;
        }
        if booked {
            o.effect = Effect::Done;
        } else {
            self.try_again(id);
        }
        self.progress_all();
    }

    pub fn bank_answers(&mut self, id: &str, paid: bool) {
        let Some(o) = self.claims.get_mut(id).and_then(|c| c.outcome.as_mut()) else { return };
        if matches!(o.kind, Kind::Writeoff | Kind::Cash) && matches!(o.effect, Effect::Sent { .. }) {
            if paid {
                o.effect = Effect::Done;
            } else {
                self.try_again(id);
            }
        }
        self.progress_all();
    }

    /// Moves the clock, firing every deadline that falls due.
    pub fn tick(&mut self, now: u64) {
        self.now = now;
        let ids: Vec<String> = self.claims.keys().cloned().collect();
        for id in &ids {
            let c = self.claims.get_mut(id).unwrap();
            if let Some(f) = &mut c.fetching
                && now >= f.at + FRAUD_WITHIN
            {
                if f.sends < SENDS {
                    f.sends += 1;
                    f.at = now;
                    let name = self.holders[&self.policies[&c.policy].holder].clone();
                    self.outbox.push(Request::Fraud { claim: id.clone(), name });
                } else {
                    c.fetching = None;
                    c.score = Some(Score { value: FRAUD_FALLBACK, at: now });
                }
            }
            let c = self.claims.get_mut(id).unwrap();
            if let Some(o) = &mut c.outcome {
                let within = if o.kind == Kind::Repair { BOOK_WITHIN } else { PAY_WITHIN };
                if let Effect::Sent { at } = o.effect
                    && now >= at + within
                {
                    // Sent again as it was: it went out, so no rule is checked again.
                    if o.sends < SENDS {
                        self.send(id);
                    } else {
                        self.move_on(id);
                    }
                } else if o.kind == Kind::Repair && o.effect == Effect::Waiting && now >= o.chosen_at + REPAIR_PATIENCE {
                    // A booking that went out is never given up on for patience, only when it fails.
                    self.move_on(id);
                }
            }
            let c = &self.claims[id];
            if !c.withdrawn && !c.settled() && !c.alerted && now >= c.created + SETTLE_WITHIN {
                self.alerts.push(id.clone());
                self.claims.get_mut(id).unwrap().alerted = true;
            }
        }
        self.progress_all();
    }

    /// After a failure answer: the same outcome again, as a new attempt (a
    /// repair picks its garage anew), until the last attempt has failed.
    fn try_again(&mut self, id: &str) {
        let o = self.claims.get_mut(id).unwrap().outcome.as_mut().unwrap();
        if o.attempt >= SENDS {
            return self.move_on(id);
        }
        if let Some(g) = o.garage.take() {
            self.garages.get_mut(&g).unwrap().taken -= 1;
        }
        o.attempt += 1;
        o.sends = 0;
        o.effect = Effect::Waiting;
        self.journal.push(format!("{id}: {:?}, attempt {}", o.kind, o.attempt));
    }

    /// Gives up on the outcome, and tries the next one if there is one.
    fn move_on(&mut self, id: &str) {
        let c = self.claims.get_mut(id).unwrap();
        let Some(o) = c.outcome.as_mut() else { return };
        if let Some(g) = o.garage.take() {
            self.garages.get_mut(&g).unwrap().taken -= 1;
        }
        c.tried.push(o.kind);
        let Some(next) = PREFER.into_iter().find(|k| !c.tried.contains(k)) else {
            o.effect = Effect::Failed;
            return;
        };
        self.journal.push(format!("{id}: {next:?}, as the {:?} did not happen", o.kind));
        *o = Outcome { kind: next, chosen_at: self.now, garage: None, effect: Effect::Waiting, sends: 0, attempt: 1 };
    }

    fn progress_all(&mut self) {
        let ids: Vec<String> = self.claims.keys().cloned().collect();
        for id in ids {
            self.progress(&id);
        }
    }

    /// Whether the fraud check lets a payout or booking go: a fresh score
    /// under the limit, or an adjuster's approval. Asks the bureau when the
    /// score is missing or stale.
    fn fraud_clear(&mut self, id: &str) -> bool {
        let now = self.now;
        let c = self.claims.get_mut(id).unwrap();
        if c.approved {
            return true;
        }
        match &c.score {
            Some(s) if now < s.at + FRAUD_GOOD_FOR => s.value < SUSPECT,
            _ => {
                if c.fetching.is_none() {
                    c.fetching = Some(Fetch { at: now, sends: 1 });
                    let name = self.holders[&self.policies[&c.policy].holder].clone();
                    self.outbox.push(Request::Fraud { claim: id.into(), name });
                }
                false
            }
        }
    }

    /// Counts the claim toward its policy's total for the year it was filed
    /// in, if there is room; a claim without room waits.
    fn count_yearly(&mut self, id: &str) -> bool {
        let c = &self.claims[id];
        if c.counted {
            return true;
        }
        let deductible = self.policies[&c.policy].deductible;
        let year = year_of(c.created);
        let total: i64 = self
            .claims
            .values()
            .filter(|o| o.counted && o.policy == c.policy && year_of(o.created) == year)
            .map(|o| o.amount - self.policies[&o.policy].deductible)
            .sum();
        if total + c.amount - deductible > YEARLY {
            return false;
        }
        self.claims.get_mut(id).unwrap().counted = true;
        true
    }

    fn progress(&mut self, id: &str) {
        let Some(c) = self.claims.get(id) else { return };
        if c.withdrawn {
            return;
        }
        if !self.count_yearly(id) {
            return;
        }
        let c = &self.claims[id];
        let Some(o) = &c.outcome else { return };
        if o.effect != Effect::Waiting {
            return;
        }
        let policy = &self.policies[&c.policy];
        match o.kind {
            Kind::Repair => {
                if o.garage.is_none() {
                    let region = policy.region.clone();
                    let best = self
                        .garages
                        .iter()
                        .filter(|(_, g)| g.region == region && g.taken < g.bays)
                        .max_by_key(|(gid, g)| (g.bays, std::cmp::Reverse((*gid).clone())))
                        .map(|(gid, _)| gid.clone());
                    let Some(g) = best else { return };
                    self.garages.get_mut(&g).unwrap().taken += 1;
                    self.journal.push(format!("{id}: garage {g}, the one with the most bays"));
                    self.claims.get_mut(id).unwrap().outcome.as_mut().unwrap().garage = Some(g);
                }
                if !self.fraud_clear(id) {
                    return;
                }
                self.send(id);
            }
            Kind::Writeoff | Kind::Cash => {
                let payout = c.amount - policy.deductible;
                if payout > policy.limit {
                    return;
                }
                if payout > BIG && !c.approved {
                    return;
                }
                if !self.fraud_clear(id) {
                    return;
                }
                self.send(id);
            }
        }
    }

    /// Sends the outcome's request, the first time or again.
    fn send(&mut self, id: &str) {
        let c = self.claims.get_mut(id).unwrap();
        let policy = &self.policies[&c.policy];
        let o = c.outcome.as_mut().unwrap();
        o.sends += 1;
        o.effect = Effect::Sent { at: self.now };
        let request = match o.kind {
            Kind::Repair => Request::Book { claim: id.into(), garage: o.garage.clone().unwrap(), damage: c.damage.clone() },
            Kind::Writeoff | Kind::Cash => Request::Pay { claim: id.into(), amount: c.amount - policy.deductible, iban: policy.iban.clone() },
        };
        self.outbox.push(request);
    }
}

/// The calendar year, in UTC, of a time in seconds since 1970.
fn year_of(secs: u64) -> i64 {
    // Days to a civil date (Howard Hinnant's algorithm).
    let z = (secs / DAY) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    yoe + era * 400 + i64::from(month <= 2)
}
