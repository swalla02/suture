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
    /// Attributes whose values come from outside (appendix I).
    pub sources: Vec<Source>,
    /// Which model handles which choices (appendix J).
    pub decisions: Vec<Decision>,
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
    /// Who calls in, and what each may do (appendix G.2).
    pub roles: Vec<Role>,
    pub admits: Vec<Admit>,
    /// Attempts of a part, and sends of a request (appendix E.2.8).
    pub attempts: u32,
    /// How many candidates a choice among shared entities gathers (appendix F.2.7).
    pub candidates: u32,
    /// What `suture build` warns about; the program still runs.
    pub warnings: Vec<String>,
    /// The marked fields each adapter may receive (appendix N.2.3).
    #[serde(default)]
    pub grants: Vec<Grant>,
}

/// `allow payments to receive Order's card`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grant {
    pub adapter: String,
    pub kind: String,
    pub attr: String,
}

/// An attribute's mark (appendix N.2.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mark {
    Secret,
    Personal,
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

/// A choice a model may make (appendix K.2.1): `Carrier`, `Delivery` or
/// `Order's credit`, and the kind that makes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChoicePoint {
    pub name: String,
    pub chooser: String,
}

/// `decide [Carrier] [for Orders [whose …]] with model "…"` (appendix J).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Decision {
    /// The choice point, a `uses` target; `None`: every choice in the scope.
    pub point: Option<String>,
    /// The kind whose choices it covers, its own and its parts'; `None`: the module.
    pub scope: Option<String>,
    /// `whose …`, read at the scope's node.
    pub cond: Option<Cond>,
    /// `None`: `with prefer`.
    pub model: Option<String>,
    /// `Orders whose total is at least 1000 EUR`, as written.
    pub scope_text: Option<String>,
    /// How long its model may take to answer (appendix K.2.5).
    pub within_ms: u64,
    /// With a router, `model` is the first candidate (appendix AJ).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route: Option<Route>,
    /// `at least 0.8 sure`, in thousandths: below it a pick is not used, in
    /// every deployment (appendix AK).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sure: Option<u16>,
    /// `otherwise Writeoff`: the option its model's choice falls back to,
    /// when it is still an option, instead of `prefer`'s (appendix AL).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub otherwise: Option<String>,
    pub source: String,
}

/// `with model "fast@1" or model "careful@1" or prefer, picked by model
/// "router@1"`: a model picks which candidate makes the choice (appendix AJ.2).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Route {
    pub router: String,
    /// How long the router may take to answer.
    pub within_ms: u64,
    /// Every candidate, first first; `None` is `prefer`.
    pub among: Vec<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sure: Option<u16>,
    /// The candidate a router's choice falls back to, by its name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub otherwise: Option<String>,
}

/// A router's name for a candidate: the model's, or `prefer`.
pub fn candidate(m: &Option<String>) -> String {
    m.clone().unwrap_or_else(|| "prefer".to_string())
}

impl Decision {
    /// How it reads in `suture build`: `for Orders whose …: model "careful@1"`.
    pub fn handler(&self) -> String {
        let one = |m: &Option<String>| match m {
            Some(m) => format!("model \"{m}\""),
            None => "`prefer`".to_string(),
        };
        let within = |ms: u64, sure: Option<u16>, otherwise: &Option<String>| {
            let w = if ms != 1000 { format!(" within {ms} ms") } else { String::new() };
            let s = sure.map_or_else(String::new, |t| format!(" at least {} sure", f64::from(t) / 1000.0));
            let o = otherwise.as_ref().map_or_else(String::new, |o| format!(" otherwise {o}"));
            format!("{w}{s}{o}")
        };
        match (&self.route, &self.model) {
            (Some(r), _) => format!(
                "{}{}, picked by model \"{}\"{}",
                r.among.iter().map(one).collect::<Vec<_>>().join(" or "),
                within(self.within_ms, self.sure, &self.otherwise),
                r.router,
                within(r.within_ms, r.sure, &r.otherwise)
            ),
            (None, Some(_)) => format!("{}{}", one(&self.model), within(self.within_ms, self.sure, &self.otherwise)),
            (None, None) => one(&None),
        }
    }

    /// The model a choice it handles is put to first: its router, or its
    /// model; `None` for `prefer`.
    pub fn asked(&self) -> Option<&str> {
        self.route.as_ref().map(|r| r.router.as_str()).or(self.model.as_deref())
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
    /// For a relation across units with `that`: the subject adds to the
    /// total only while this holds, read at the subject (appendix AA.2.1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub held: Option<Cond>,
    /// For a relation across units: whether the units that refer to the
    /// target read the total, so they carry it (appendix AA.2.3).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub shared: bool,
    /// A `no two … have the same …` count: an admitted input that would
    /// break it is refused, even with no filter (appendix AP.2.3).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unique: bool,
    /// The sourced attributes its amount reads through formulas, as (kind,
    /// attribute): fetched for it while it cannot be asked (appendix AG.2.2).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<(String, String)>,
}

impl Binding {
    /// For a relation across units: the subject's attribute that names the target.
    pub fn key_words(&self) -> String {
        match &self.key {
            Num::Path(Path { steps }) => match steps.as_slice() {
                [Step::Attr(a)] => a.clone(),
                _ => self.target.to_lowercase(),
            },
            _ => self.target.to_lowercase(),
        }
    }
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
    /// With `period`, the times the entity spans, read at it: it counts
    /// toward every period from the first to the last (appendix AD.2.1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span: Option<(Num, Num)>,
    /// `no two Reviews have the same talk and reviewer`: the values, read at
    /// the entity, that pick its group; one total for each group, named
    /// `number of Reviews with the same reviewer#Bo` (appendix AP.2.2).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub group: Vec<Num>,
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

/// `role customer is a Customer`: a caller's name is then a Customer unit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Role {
    pub name: String,
    pub kind: Option<String>,
}

/// One `admit` (appendix G.2.2). Its condition is read at the target, with
/// [`Step::Caller`] for the caller's unit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Admit {
    pub what: Admitted,
    pub roles: Vec<String>,
    /// An event's data, read as `the reason` (appendix Q.2.1).
    #[serde(default)]
    pub data: Vec<(String, Ty)>,
    pub cond: Option<Cond>,
    pub source: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Admitted {
    New { verb: String, kind: String },
    Event { verb: String, kind: String },
    Change { kind: String, attr: String },
}

impl Admitted {
    pub fn kind(&self) -> &str {
        match self {
            Admitted::New { kind, .. } | Admitted::Event { kind, .. } | Admitted::Change { kind, .. } => kind,
        }
    }
}

/// `cancel` → `cancels`, `place` → `places`, `fix` → `fixes`, `apply` →
/// `applies`, `pay` → `pays` (appendix G.2.4).
pub fn third_person(verb: &str) -> String {
    if let Some(stem) = verb.strip_suffix('y').filter(|s| s.ends_with(|c: char| c.is_alphabetic() && !"aeiou".contains(c))) {
        return format!("{stem}ies");
    }
    if ["s", "sh", "ch", "x", "z"].iter().any(|e| verb.ends_with(e)) { format!("{verb}es") } else { format!("{verb}s") }
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
    #[serde(default)]
    pub mark: Option<Mark>,
    /// `reception: awaiting | received | lost`: a Text that holds one of these
    /// states, the first by default (appendix Z.2.1). Empty for any other attribute.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub states: Vec<String>,
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
    /// Media, held by the hash of its content (appendix V.2.1).
    Media,
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
    /// In seconds on the runtime clock, whose 0 is 2026-01-01 00:00 UTC.
    Time(i64),
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

/// `source Order's credit via bureau within 2 seconds …` (appendix I).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    pub kind: String,
    pub attr: String,
    pub ty: Ty,
    /// The adapter, or the model's `name@version` when `model` (appendix AF).
    pub adapter: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub model: bool,
    /// What the model is asked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asking: Option<String>,
    /// The confidence, in thousandths, that a value the model reads needs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sure: Option<u16>,
    /// `scored from 1 to 5`: a count the model reads outside it is not used (appendix AZ).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<(i64, i64)>,
    pub within_secs: u64,
    /// What the adapter receives, as for an effect.
    pub sends: Vec<(String, Path)>,
    pub success: String,
    pub failures: Vec<String>,
    /// How long a value is trusted; `None`: for ever.
    pub good_for: Option<u64>,
    pub otherwise: Otherwise,
}

/// A model source's outcomes: it read the value, or did not (appendix AF.2.1).
pub const READ: &str = "read";
pub const UNREAD: &str = "unread";

impl Source {
    /// How requests name it: `Order~credit@bureau`.
    pub fn request_kind(&self) -> String {
        format!("{}~{}@{}", self.kind, self.attr, self.adapter)
    }
}

/// What a source does when its fetch fails (appendix I.2.6).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Otherwise {
    /// The value stays unknown, and every step its rules guard waits.
    Hold,
    /// The value is this, as if fetched now.
    Use(Lit),
    /// Fetch again after this many seconds, if a step still needs it.
    Again(u64),
    /// Apply these actions at the sourced node.
    Act(Vec<Action>),
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
    /// The sourced attributes it reads, directly or through a definition, as
    /// (kind, attribute): if any, the rule is strict (appendix I.2.4).
    pub sources: Vec<(String, String)>,
    /// `with credit at most 1 hour old`: the oldest value it accepts, in seconds.
    pub fresh: Vec<(String, u64)>,
    /// `apply rule … to Ks created from version n`: the rule holds only on
    /// units of root kind K created under version n or later (appendix M.2.3).
    #[serde(default)]
    pub applies: Option<Applies>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Applies {
    pub kind: String,
    pub from: u32,
    pub source: String,
}

// A rule's body is built once per rule, so its size does not matter.
#[allow(clippy::large_enum_variant)]
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
    /// An effect's outcome, or for an admitted event its verb (`cancels`).
    pub outcome: String,
    /// For an admitted event: the roles it is from (appendix G.2.4).
    pub by: Vec<String>,
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
    /// In an `admit` condition or an event's reaction: the caller's unit,
    /// through its carried facts.
    Caller,
    /// In an `admit` condition or an event's reaction: a value the event
    /// carries, `the reason` (appendix Q.2.2). It is a whole path.
    Data(String),
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
    /// (appendix F.2.8). `kind` is that unit's kind; empty when `of` is the
    /// unit itself, else read through a reference (appendix AA.2.3).
    Taken {
        of: Path,
        name: String,
        ty: Ty,
        period: Option<String>,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        kind: String,
        /// `the days in a row of …`: instead of the day's total, how many
        /// days in a row, through the day, have a total above nothing
        /// (appendix AH.2.1).
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        run: bool,
        /// The total of the group a take is checked for, not one total
        /// (appendix AP.2.2); only while a take is checked.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        group: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Arith {
    Add,
    Sub,
    Mul,
    Div,
    /// `the days from x to y`: how many calendar days, in UTC, from the
    /// left time's to the right's, both counted (appendix AN.2.1).
    Days,
    /// `the weekdays from x to y`: the same, counting Monday to Friday only
    /// (appendix AW).
    Weekdays,
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

/// The end of a reaction's outcome when it is to a time passing:
/// `due passes`, for `when a Loan's due passes` (appendix AQ).
pub const PASSES: &str = " passes";

/// The built-in attribute every entity has: when it was created (appendix F.2.14).
pub const CREATED: &str = "created";

impl Program {
    pub fn role(&self, name: &str) -> Option<&Role> {
        self.roles.iter().find(|r| r.name == name)
    }

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

    pub fn source_of(&self, kind: &str, attr: &str) -> Option<&Source> {
        self.sources.iter().find(|s| s.kind == kind && s.attr == attr)
    }

    /// Every source of an attribute, in `prefer` order: as declared (appendix K.2.4).
    pub fn sources_of(&self, kind: &str, attr: &str) -> Vec<&Source> {
        self.sources.iter().filter(|s| s.kind == kind && s.attr == attr).collect()
    }

    /// What happens when every source of an attribute has failed: the one
    /// `otherwise` among them, or none.
    pub fn otherwise_of(&self, kind: &str, attr: &str) -> &Otherwise {
        static HOLD: Otherwise = Otherwise::Hold;
        self.sources.iter().filter(|s| s.kind == kind && s.attr == attr).map(|s| &s.otherwise).find(|o| **o != Otherwise::Hold).unwrap_or(&HOLD)
    }

    /// A kind, its owner, its owner's owner, … (appendix J.2.3).
    pub fn owner_chain(&self, kind: &str) -> Vec<String> {
        let mut out = vec![kind.to_string()];
        while let Some(o) = self.kind(out.last().expect("not empty")).and_then(|x| x.owner.as_ref()) {
            if out.contains(&o.kind) {
                break;
            }
            out.push(o.kind.clone());
        }
        out
    }

    /// Every choice a model may make (appendix K.2.1): a `uses` with a pool
    /// key, a slot, and a sourced attribute with more than one source.
    pub fn choice_points(&self) -> Vec<ChoicePoint> {
        let mut out = vec![];
        for b in self.bindings.iter().filter(|b| !b.key_attr.is_empty()) {
            out.push(ChoicePoint { name: b.target.clone(), chooser: b.subject.clone() });
        }
        for s in &self.slots {
            out.push(ChoicePoint { name: s.name.clone(), chooser: s.owner.clone() });
        }
        for s in &self.sources {
            let name = format!("{}'s {}", s.kind, s.attr);
            if self.sources_of(&s.kind, &s.attr).len() > 1 && !out.iter().any(|c: &ChoicePoint| c.name == name) {
                out.push(ChoicePoint { name, chooser: s.kind.clone() });
            }
        }
        out
    }

    /// The deadline of a request, by the kind it names: a part kind with an
    /// effect, or a source's `Order~credit`.
    pub fn request_within(&self, kind: &str) -> u64 {
        match kind.split_once('~') {
            Some((k, a)) => match a.split_once('@') {
                Some((a, via)) => self.sources_of(k, a).into_iter().find(|s| s.adapter == via).map_or(0, |s| s.within_secs),
                None => self.source_of(k, a).map_or(0, |s| s.within_secs),
            },
            None => self.effect_of(kind).map_or(0, |e| e.within_secs),
        }
    }

    /// The attributes a state may be read from, matched by name across kinds:
    /// the state's own name, and each state attribute that holds it.
    pub fn state_attrs<'a>(&'a self, state: &'a str) -> Vec<&'a str> {
        let mut out = vec![state];
        for a in self.kinds.iter().filter_map(|k| k.state_attr(state)) {
            if !out.contains(&a.name.as_str()) {
                out.push(&a.name);
            }
        }
        out
    }

    /// The states a node of `kind` can be in (spec B.3): its Bool attributes,
    /// the states of its state attributes, its defined states, its effect's
    /// marks and outcomes (`timed out` among them), `done` and `failed`.
    pub fn states(&self, kind: &str) -> Vec<String> {
        let mut out: Vec<String> = vec![];
        if self.slot(kind).is_some() {
            return vec!["done".into(), "failed".into()];
        }
        if let Some(k) = self.kind(kind) {
            out.extend(k.attrs.iter().filter(|a| a.ty == Ty::Bool).map(|a| a.name.clone()));
            out.extend(k.attrs.iter().flat_map(|a| a.states.iter().cloned()));
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

    /// The state attribute that holds `state` as one of its states (appendix Z.2.2).
    pub fn state_attr(&self, state: &str) -> Option<&Attr> {
        self.attrs.iter().find(|a| a.states.iter().any(|s| s == state))
    }
}
