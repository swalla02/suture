//! The compiled program: what `suture build` writes and the runtime reads.
//! Every name in it has been resolved and checked (spec §4, appendix B).

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Program {
    pub module: String,
    pub version: u32,
    pub currency: String,
    pub kinds: Vec<Kind>,
    pub needs: Vec<Need>,
    pub effects: Vec<Effect>,
    pub rules: Vec<Rule>,
    pub defines: Vec<Define>,
    pub goals: Vec<Goal>,
    pub reactions: Vec<Reaction>,
    pub slots: Vec<Slot>,
    pub lists: Vec<List>,
    pub bindings: Vec<Binding>,
    /// The units the host makes at the start, one for each rule `checked
    /// globally`, by kind: `all Refunds` (appendix F.2.13).
    pub globals: Vec<String>,
    /// Attempts of a part, and sends of a request (appendix E.2.8).
    pub attempts: u32,
    /// How many candidates a choice among shared entities gathers (appendix F.2.7).
    pub candidates: u32,
    /// What `suture build` warns about; the program still runs.
    pub warnings: Vec<String>,
}

/// `lines: list of Line` on an Order (appendix E.2.6).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct List {
    pub owner: String,
    pub name: String,
    pub element: String,
}

impl List {
    /// How `the …` and a scenario's `a … with` may name one element: the
    /// list's name without `s`, without `es`, with `ies` made `y`, or the
    /// element kind's own name (`lines` → `line`; `entries` → `entry`;
    /// `boxes` of Case → `box`, `case`).
    pub fn singulars(&self) -> Vec<String> {
        let n = &self.name;
        let mut out = vec![];
        if let Some(stem) = n.strip_suffix("ies") {
            out.push(format!("{stem}y"));
        }
        if let Some(stem) = n.strip_suffix("es") {
            out.push(stem.to_string());
        }
        if let Some(stem) = n.strip_suffix('s') {
            out.push(stem.to_string());
        }
        out.push(self.element.to_lowercase());
        out.dedup();
        out
    }

    /// The name the read-backs use for one element.
    pub fn singular(&self) -> String {
        let s = self.singulars();
        let kind = self.element.to_lowercase();
        if s.contains(&kind) { kind } else { s[0].clone() }
    }
}

/// `Shipment uses a Stock per line where …, taking …` (appendix F.2.5). Its
/// conditions are read at the subject, with [`Step::Cand`] for the
/// candidate and [`Step::Elem`] for the element of `per`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binding {
    /// How a part names it: the target (`Carrier`), or for a relation
    /// across units the target and the total (`Customer(amount of Refunds)`).
    pub id: String,
    pub subject: String,
    pub target: String,
    /// The list, of the subject or an owner of it, with one binding per element.
    pub per: Option<String>,
    /// The candidate's attribute the pool is indexed by; empty when the key
    /// names the candidate itself (appendix F.2.11).
    pub key_attr: String,
    /// Whether that attribute is a set that must contain the key.
    pub key_member: bool,
    /// The key, read at the subject; it never reads the candidate.
    pub key: Num,
    /// The other `and` parts of the condition.
    pub filter: Vec<Cond>,
    pub taking: Option<Taking>,
    pub prefer: Vec<Criterion>,
    pub source: String,
}

/// `taking the line's quantity`: the amount, and the name of the running
/// total it adds to (`quantity`, after its last attribute).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Taking {
    pub name: String,
    pub amount: Num,
    pub ty: Ty,
    /// `month`: one total for each period of the subject's `created`
    /// (appendix F.2.12), named `amount of Refunds per month@2026-01`.
    pub period: Option<String>,
}

/// The periods of `for each K and ⟨period⟩`, in UTC (appendix F.2.12).
pub const PERIODS: [&str; 4] = ["day", "week", "month", "year"];

/// How a binding ranks its candidates, most important first (appendix F.2.7).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Criterion {
    Lowest(Num),
    Highest(Num),
    Those(Cond),
}

/// A slot an owner fills with one of its alternatives (appendix D).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Slot {
    pub name: String,
    pub owner: String,
    /// In `prefer` order.
    pub alternatives: Vec<String>,
    /// For each alternative, how long to wait on it before moving on, if at all.
    pub patience: Vec<Option<u64>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Kind {
    pub name: String,
    /// `Refunds`, `People`: how relations name it (appendix E.2.2).
    pub plural: String,
    /// Value attributes, including the ones an effect's answer sets.
    pub attrs: Vec<Attr>,
    /// For a part: the attribute that names its owner, and the owner's kind.
    pub owner: Option<OwnerRef>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnerRef {
    pub attr: String,
    pub kind: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attr {
    pub name: String,
    pub ty: Ty,
    /// For a reference or a set: the kind it refers to (appendix F.2.2).
    pub of: Option<String>,
    pub default: Option<Lit>,
    /// Computed once at creation, after defaults (appendix E.2.5).
    pub formula: Option<Num>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Ty {
    Text,
    Bool,
    Money,
    Count,
    /// A reference to a shared entity; `Attr.of` says of which kind.
    Ref,
    /// References to several shared entities.
    Set,
    /// Seconds on the runtime clock (appendix F.2.14).
    Time,
    /// Seconds.
    Duration,
}

/// A literal value; money is in cents of the module's currency.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Lit {
    Text(String),
    Bool(bool),
    Money(i64),
    Count(i64),
    /// In seconds.
    Duration(i64),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Need {
    pub owner: String,
    pub part: String,
    pub cond: Option<Cond>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Effect {
    pub kind: String,
    pub verb: String,
    pub adapter: String,
    pub within_secs: u64,
    /// What the adapter receives: a name, and where its value comes from.
    pub sends: Vec<(String, Path)>,
    pub marks: Option<String>,
    pub success: String,
    pub data: Vec<(String, Ty)>,
    pub failures: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    pub name: String,
    pub subject: String,
    pub filter: Vec<Cond>,
    pub body: Body,
    pub source: String,
    pub formula: String,
    pub paraphrase: String,
    pub tier: String,
    /// For `for each K and month`: its totals are per period, checked only
    /// when something is taken in one.
    pub period: Option<String>,
    /// Whether its filter and `unless` read only facts that never change, so
    /// a slot it holds back moves on at once (appendix F.2.15).
    pub fixed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Body {
    Every(Cond),
    No { pred: Cond, unless: Option<Cond> },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Define {
    pub kind: String,
    pub state: String,
    pub cond: Cond,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Goal {
    pub kind: String,
    pub filter: Vec<Cond>,
    pub states: Vec<String>,
    pub within_secs: Option<u64>,
    pub source: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reaction {
    pub kind: String,
    pub outcome: String,
    pub actions: Vec<Action>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Action {
    pub target: Path,
    pub attr: String,
    pub value: bool,
    /// `… becomes ⟨num⟩`, evaluated at the reacting node; otherwise a Bool is set.
    pub becomes: Option<Num>,
}

/// A walk from a node: down to a part, up to the owner, and last to an attribute.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Path {
    pub steps: Vec<Step>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Step {
    Attr(String),
    Part(String),
    /// A slot: its current alternative's current part (appendix D.3.1).
    Slot(String),
    Owner,
    /// Through a reference attribute, to the shared entity's carried facts
    /// (appendix F.2.3).
    Ref(String),
    /// To the entity a binding to this kind was granted, through its carried facts.
    Bound(String),
    /// In a `uses` condition or its `prefer`: the candidate.
    Cand,
    /// In a `uses … per`: the element the binding is for.
    Elem,
}

/// A calculation (appendix E.2.3). Paths are relative to the node it is
/// evaluated at; a `Sum` or `Count` evaluates its `calc` and `filter` at
/// each entity of its relation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Num {
    Path(Path),
    Lit(Lit),
    Bin(Box<Num>, Arith, Box<Num>),
    /// Every part of `kind` under the node at `of`, added up.
    Sum { of: Path, kind: String, calc: Box<Num>, filter: Option<Box<Cond>>, ty: Ty },
    /// How many parts of `kind` there are under the node at `of`.
    Count { of: Path, kind: String, filter: Option<Box<Cond>> },
    /// The running total of the amounts taken as `name` from the unit at `of`
    /// (appendix F.2.8).
    Taken { of: Path, name: String, ty: Ty, period: Option<String> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Arith {
    Add,
    Sub,
    Mul,
    Div,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Op {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Cond {
    /// Compares a value with a literal or another value.
    Cmp { left: Num, op: Op, right: Num },
    /// A node is (or is not) in a state.
    State { node: Path, state: String, negated: bool },
    /// A node has (or has no) part of a kind, optionally in a state.
    Has { node: Path, part: String, state: Option<String>, negated: bool },
    /// A set contains (or does not contain) a reference (appendix F.2.2).
    Contains { set: Num, item: Num, negated: bool },
    And(Vec<Cond>),
    Or(Vec<Cond>),
}

/// The outcome of a request that got no answer in time (appendix C.3.3).
pub const TIMED_OUT: &str = "timed out";

/// The built-in attribute every entity has: when it was created (appendix F.2.14).
pub const CREATED: &str = "created";

impl Program {
    pub fn kind(&self, name: &str) -> Option<&Kind> {
        self.kinds.iter().find(|k| k.name == name)
    }

    pub fn slot(&self, name: &str) -> Option<&Slot> {
        self.slots.iter().find(|s| s.name == name)
    }

    /// Whether `owner` has parts of kind `part`: needed directly, or as an
    /// alternative of a slot it needs.
    pub fn has_part(&self, owner: &str, part: &str) -> bool {
        self.lists.iter().any(|l| l.owner == owner && l.element == part) || self.needs.iter().any(|n| {
            n.owner == owner && (n.part == part || self.slot(&n.part).is_some_and(|s| s.alternatives.iter().any(|a| a == part)))
        })
    }

    pub fn effect_of(&self, kind: &str) -> Option<&Effect> {
        self.effects.iter().find(|e| e.kind == kind)
    }

    /// The states a node of `kind` can be in (spec B.3): its Bool attributes,
    /// its defined states, its effect's marks and outcomes (`timed out` among
    /// them), `done` and `failed`.
    pub fn states(&self, kind: &str) -> Vec<String> {
        let mut out: Vec<String> = vec![];
        if self.slot(kind).is_some() {
            return vec!["done".into(), "failed".into()];
        }
        if let Some(k) = self.kind(kind) {
            out.extend(k.attrs.iter().filter(|a| a.ty == Ty::Bool).map(|a| a.name.clone()));
        }
        out.extend(self.defines.iter().filter(|d| d.kind == kind).map(|d| d.state.clone()));
        if let Some(e) = self.effect_of(kind) {
            out.extend(e.marks.iter().cloned());
            out.push(e.success.clone());
            out.extend(e.failures.iter().cloned());
            out.push(TIMED_OUT.into());
        }
        out.push("done".into());
        out.push("failed".into());
        out
    }
}

impl Kind {
    pub fn attr(&self, name: &str) -> Option<&Attr> {
        self.attrs.iter().find(|a| a.name == name)
    }
}
