//! Typed payloads of the dependency-network program and their `Value`
//! encodings. Rules decode a payload, compute, and encode the result; the
//! kernel only ever sees immutable values (IR spec M1).

use crate::tup;
use crate::value::{Value, ValueError, ValueType};

pub const INACTIVE: i64 = 0;
/// Activated, waiting for the recipe decision.
pub const CHOOSING: i64 = 1;
pub const ACTIVE: i64 = 2;
pub const DEAD: i64 = 3;
/// A root that exhausted its attempts.
pub const GAVE_UP: i64 = 4;

fn ints(v: &Value) -> Result<Vec<i64>, ValueError> {
    v.items()
        .ok_or_else(|| ValueError { value: v.to_string(), reason: "not a list".into() })?
        .iter()
        .map(|x| x.as_int().ok_or_else(|| ValueError { value: x.to_string(), reason: "not an int".into() }))
        .collect()
}

fn list_of_ints(xs: &[i64]) -> Value {
    Value::list(xs.iter().map(|&x| Value::Int(x)))
}

fn int_list_type() -> ValueType {
    ValueType::List(Box::new(ValueType::Int))
}

/// One dependency of an active provider (or one need of a root).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dep {
    pub ty: i64,
    /// Provider currently bound, or -1.
    pub bound: i64,
    /// Nonce of the current need for this dependency (part of the binding key).
    pub nonce: i64,
    pub attempts: i64,
    /// Providers this dependency lost or failed with, newest last.
    pub history: Vec<i64>,
}

/// The state of a provider cell (roots are provider cells with `root`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prov {
    pub id: i64,
    pub ty: i64,
    pub region: i64,
    pub capacity: i64,
    /// Binding keys `(owner, dep, nonce)` of current consumers, newest first;
    /// mirrors the provider's structural consumer list.
    pub consumers: Vec<[i64; 3]>,
    pub phase: i64,
    pub recipes: Vec<Vec<i64>>,
    pub recipe: i64,
    pub deps: Vec<Dep>,
    /// Recipes tried and abandoned during the current activation.
    pub tried: Vec<i64>,
    /// Local failure history: dependency failures observed at this provider.
    pub fails: i64,
    /// Regions this provider's dependencies may use.
    pub regions: Vec<i64>,
    pub root: bool,
    /// Next need nonce.
    pub nonce: i64,
    /// Bumped on deactivation and recipe switch; stale decisions carry an older one.
    pub epoch: i64,
}

impl Prov {
    pub fn schema() -> ValueType {
        use ValueType::*;
        let dep = Tuple(vec![Int, Int, Int, Int, int_list_type()]);
        Tuple(vec![
            Int,
            Int,
            Int,
            Int,
            List(Box::new(Tuple(vec![Int, Int, Int]))),
            Int,
            List(Box::new(int_list_type())),
            Int,
            List(Box::new(dep)),
            int_list_type(),
            Int,
            int_list_type(),
            Bool,
            Int,
            Int,
        ])
    }

    pub fn to_value(&self) -> Value {
        let deps = self.deps.iter().map(|d| tup![d.ty, d.bound, d.nonce, d.attempts, list_of_ints(&d.history)]);
        tup![
            self.id,
            self.ty,
            self.region,
            self.capacity,
            Value::list(self.consumers.iter().map(|k| tup![k[0], k[1], k[2]])),
            self.phase,
            Value::list(self.recipes.iter().map(|r| list_of_ints(r))),
            self.recipe,
            Value::list(deps),
            list_of_ints(&self.tried),
            self.fails,
            list_of_ints(&self.regions),
            self.root,
            self.nonce,
            self.epoch
        ]
    }

    pub fn from_value(v: &Value) -> Result<Prov, ValueError> {
        let deps = v
            .list_at(8)?
            .iter()
            .map(|d| {
                Ok(Dep { ty: d.int(0)?, bound: d.int(1)?, nonce: d.int(2)?, attempts: d.int(3)?, history: ints(d.get(4)?)? })
            })
            .collect::<Result<_, ValueError>>()?;
        Ok(Prov {
            id: v.int(0)?,
            ty: v.int(1)?,
            region: v.int(2)?,
            capacity: v.int(3)?,
            consumers: v
                .list_at(4)?
                .iter()
                .map(|k| Ok([k.int(0)?, k.int(1)?, k.int(2)?]))
                .collect::<Result<_, ValueError>>()?,
            phase: v.int(5)?,
            recipes: v.list_at(6)?.iter().map(ints).collect::<Result<_, _>>()?,
            recipe: v.int(7)?,
            deps,
            tried: ints(v.get(9)?)?,
            fails: v.int(10)?,
            regions: ints(v.get(11)?)?,
            root: v.get(12)?.as_bool().unwrap_or(false),
            nonce: v.int(13)?,
            epoch: v.int(14)?,
        })
    }

    pub fn used(&self) -> i64 {
        self.consumers.len() as i64
    }

    pub fn alive(&self) -> bool {
        self.phase != DEAD && self.phase != GAVE_UP
    }

    /// Whether this provider may take a consumer that accepts `regions`.
    pub fn eligible_for(&self, regions: &[i64]) -> bool {
        !self.root && self.alive() && self.used() < self.capacity && regions.contains(&self.region)
    }

    /// Activation cost seen by a binder: dependencies it would have to build.
    pub fn activation_cost(&self) -> i64 {
        if self.phase == ACTIVE || self.phase == CHOOSING {
            0
        } else {
            self.recipes.iter().map(|r| r.len() as i64).min().unwrap_or(0)
        }
    }
}

/// A requirement travelling to a market: dependency `dep` of `owner`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Need {
    pub owner_ty: i64,
    pub owner: i64,
    pub dep: i64,
    pub nonce: i64,
    pub ty: i64,
    pub regions: Vec<i64>,
    pub history: Vec<i64>,
    pub attempt: i64,
    /// Stale decisions so far (the chosen provider changed before enactment).
    pub conflicts: i64,
}

impl Need {
    pub fn schema() -> ValueType {
        use ValueType::*;
        Tuple(vec![Int, Int, Int, Int, Int, int_list_type(), int_list_type(), Int, Int])
    }
    pub fn to_value(&self) -> Value {
        tup![
            self.owner_ty,
            self.owner,
            self.dep,
            self.nonce,
            self.ty,
            list_of_ints(&self.regions),
            list_of_ints(&self.history),
            self.attempt,
            self.conflicts
        ]
    }
    pub fn from_value(v: &Value) -> Result<Need, ValueError> {
        Ok(Need {
            owner_ty: v.int(0)?,
            owner: v.int(1)?,
            dep: v.int(2)?,
            nonce: v.int(3)?,
            ty: v.int(4)?,
            regions: ints(v.get(5)?)?,
            history: ints(v.get(6)?)?,
            attempt: v.int(7)?,
            conflicts: v.int(8)?,
        })
    }
    /// The binding key: unique per (owner, dependency, nonce).
    pub fn key(&self) -> Value {
        tup![self.owner, self.dep, self.nonce]
    }
}

/// What a gathering walk records about an admissible provider. Only data the
/// walk saw in the provider cell itself, plus the need's own history.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cand {
    pub id: i64,
    pub used: i64,
    pub capacity: i64,
    pub active: i64,
    pub cost: i64,
    pub fails: i64,
    pub in_history: i64,
    pub recipes: i64,
}

impl Cand {
    pub fn of(p: &Prov, need: &Need) -> Cand {
        Cand {
            id: p.id,
            used: p.used(),
            capacity: p.capacity,
            active: (p.phase == ACTIVE || p.phase == CHOOSING) as i64,
            cost: p.activation_cost(),
            fails: p.fails,
            in_history: need.history.contains(&p.id) as i64,
            recipes: p.recipes.len() as i64,
        }
    }
    pub fn to_value(&self) -> Value {
        tup![self.id, self.used, self.capacity, self.active, self.cost, self.fails, self.in_history, self.recipes]
    }
    pub fn from_value(v: &Value) -> Result<Cand, ValueError> {
        Ok(Cand {
            id: v.int(0)?,
            used: v.int(1)?,
            capacity: v.int(2)?,
            active: v.int(3)?,
            cost: v.int(4)?,
            fails: v.int(5)?,
            in_history: v.int(6)?,
            recipes: v.int(7)?,
        })
    }
    pub fn schema() -> ValueType {
        ValueType::Tuple(vec![ValueType::Int; 8])
    }
}

/// Control messages processed by a market's walkers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ctl {
    /// A bind decision: give `provider` to the parked need (owner, dep, nonce).
    Choice { owner: i64, dep: i64, nonce: i64, provider: i64 },
    /// A consumer drops its binding to `provider`.
    Release { provider: i64, owner: i64, dep: i64, nonce: i64 },
    /// The environment removes `provider`.
    Kill { provider: i64 },
    DepBound { owner: i64, dep: i64, nonce: i64, provider: i64, provider_ty: i64 },
    DepFailed { owner: i64, dep: i64, nonce: i64 },
    DepLost { owner: i64, dep: i64, nonce: i64, provider: i64 },
    /// A recipe decision for `provider`, valid for activation `epoch`.
    Recipe { provider: i64, recipe: i64, epoch: i64 },
    /// The environment changes `provider`'s capacity.
    SetCap { provider: i64, capacity: i64 },
    /// Activates a root.
    Start { provider: i64 },
}

impl Ctl {
    pub fn schema() -> ValueType {
        ValueType::Tuple(vec![ValueType::Int; 6])
    }

    pub fn to_value(&self) -> Value {
        let (tag, f) = match *self {
            Ctl::Choice { owner, dep, nonce, provider } => (0, [owner, dep, nonce, provider, 0]),
            Ctl::Release { provider, owner, dep, nonce } => (1, [provider, owner, dep, nonce, 0]),
            Ctl::Kill { provider } => (2, [provider, 0, 0, 0, 0]),
            Ctl::DepBound { owner, dep, nonce, provider, provider_ty } => (3, [owner, dep, nonce, provider, provider_ty]),
            Ctl::DepFailed { owner, dep, nonce } => (4, [owner, dep, nonce, 0, 0]),
            Ctl::DepLost { owner, dep, nonce, provider } => (5, [owner, dep, nonce, provider, 0]),
            Ctl::Recipe { provider, recipe, epoch } => (6, [provider, recipe, epoch, 0, 0]),
            Ctl::SetCap { provider, capacity } => (7, [provider, capacity, 0, 0, 0]),
            Ctl::Start { provider } => (8, [provider, 0, 0, 0, 0]),
        };
        tup![tag, f[0], f[1], f[2], f[3], f[4]]
    }

    pub fn from_value(v: &Value) -> Result<Ctl, ValueError> {
        let f = |i| v.int(i);
        Ok(match f(0)? {
            0 => Ctl::Choice { owner: f(1)?, dep: f(2)?, nonce: f(3)?, provider: f(4)? },
            1 => Ctl::Release { provider: f(1)?, owner: f(2)?, dep: f(3)?, nonce: f(4)? },
            2 => Ctl::Kill { provider: f(1)? },
            3 => Ctl::DepBound { owner: f(1)?, dep: f(2)?, nonce: f(3)?, provider: f(4)?, provider_ty: f(5)? },
            4 => Ctl::DepFailed { owner: f(1)?, dep: f(2)?, nonce: f(3)? },
            5 => Ctl::DepLost { owner: f(1)?, dep: f(2)?, nonce: f(3)?, provider: f(4)? },
            6 => Ctl::Recipe { provider: f(1)?, recipe: f(2)?, epoch: f(3)? },
            7 => Ctl::SetCap { provider: f(1)?, capacity: f(2)? },
            8 => Ctl::Start { provider: f(1)? },
            t => return Err(ValueError { value: v.to_string(), reason: format!("unknown control tag {t}") }),
        })
    }

    /// The cell a visiting walk looks for (`None` for bind choices, which
    /// first unpark their need).
    pub fn target(&self) -> Option<i64> {
        match *self {
            Ctl::Choice { .. } => None,
            Ctl::Release { provider, .. }
            | Ctl::Kill { provider }
            | Ctl::Recipe { provider, .. }
            | Ctl::SetCap { provider, .. }
            | Ctl::Start { provider } => Some(provider),
            Ctl::DepBound { owner, .. } | Ctl::DepFailed { owner, .. } | Ctl::DepLost { owner, .. } => Some(owner),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payloads_roundtrip_and_match_their_schemas() {
        let p = Prov {
            id: 101,
            ty: 2,
            region: 1,
            capacity: 3,
            consumers: vec![[9, 0, 4]],
            phase: ACTIVE,
            recipes: vec![vec![3], vec![3, 4]],
            recipe: 1,
            deps: vec![Dep { ty: 3, bound: 140, nonce: 7, attempts: 1, history: vec![130] }],
            tried: vec![0],
            fails: 2,
            regions: vec![1],
            root: false,
            nonce: 8,
            epoch: 1,
        };
        assert!(Prov::schema().admits(&p.to_value()));
        assert_eq!(Prov::from_value(&p.to_value()).unwrap(), p);
        let n = Need { owner_ty: 1, owner: 9, dep: 0, nonce: 3, ty: 2, regions: vec![0, 1], history: vec![], attempt: 1, conflicts: 0 };
        assert!(Need::schema().admits(&n.to_value()));
        assert_eq!(Need::from_value(&n.to_value()).unwrap(), n);
        for c in [
            Ctl::Choice { owner: 1, dep: 2, nonce: 3, provider: 4 },
            Ctl::Release { provider: 4, owner: 1, dep: 2, nonce: 3 },
            Ctl::Kill { provider: 5 },
            Ctl::DepBound { owner: 1, dep: 0, nonce: 2, provider: 3, provider_ty: 4 },
            Ctl::DepFailed { owner: 1, dep: 0, nonce: 2 },
            Ctl::DepLost { owner: 1, dep: 0, nonce: 2, provider: 3 },
            Ctl::Recipe { provider: 3, recipe: 1, epoch: 2 },
            Ctl::SetCap { provider: 3, capacity: 0 },
            Ctl::Start { provider: 1 },
        ] {
            assert!(Ctl::schema().admits(&c.to_value()));
            assert_eq!(Ctl::from_value(&c.to_value()).unwrap(), c);
        }
    }
}
