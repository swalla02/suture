//! Unresolved syntax: what the parsers produce and the checker resolves.
//! Names are kept as written; the checker resolves them (spec B.3.8).

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Loc {
    pub file: String,
    pub line: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypeName {
    Text,
    Bool,
    Money,
    Count,
    /// A reference to another kind; in slice 1, only to a part's owner.
    Kind(String),
    /// `list of Line`: elements given with their owner (appendix E.2.6).
    List(String),
    /// `set of Country`: references to several shared entities (appendix F.2.2).
    Set(String),
    Time,
    Duration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Literal {
    Text(String),
    Bool(bool),
    Money { cents: i64, currency: String },
    Count(i64),
    /// `2 days`, in seconds (appendix F.2.14).
    Duration(i64),
    /// `{DE, FR}`: the names of shared entities, in scenarios only.
    Set(Vec<String>),
}

/// `it`, `its x`, `the Order`, `the order's total`, `its form's order's total`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NounPhrase {
    pub head: Head,
    /// The words after the head: `its form's order's total` gives [form, order, total].
    pub steps: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Head {
    /// `it`, `its`, `their`, or `the` followed by a lower-case word: the subject.
    It,
    /// `the Order`: the subject if it is an Order, otherwise its owner of that kind.
    The(String),
    /// `each Order`: in a bound for each entity, the subject (appendix E.2.4).
    Each(String),
    /// `all Refunds`: every entity of a kind, by its plural (appendix F.2.13).
    All(String),
}

/// A calculation (spec §3.3 "Numbers", appendix E).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Num {
    /// A value attribute.
    Np(NounPhrase),
    Lit(Literal),
    /// `a + b`, `a - b`, `a * b`, `a / b`.
    Bin(Box<Num>, char, Box<Num>),
    /// `the total amount of its Refunds [that …]`
    Total { attr: String, of: NounPhrase, filter: Option<Box<Cond>> },
    /// `the number of its Shipments [that …]`
    Number { of: NounPhrase, filter: Option<Box<Cond>> },
    /// `the total of the line's price * the line's quantity over its lines`
    Over { calc: Box<Num>, list: NounPhrase },
    /// `the total quantity taken from each Stock` (appendix F.2.8)
    Taken { attr: String, from: NounPhrase },
}

/// What follows `is`: a literal, a noun phrase, or a bare word that the
/// checker reads as text or as a state (spec B.3.7).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Term {
    Lit(Literal),
    Np(NounPhrase),
    Word(String),
    /// Any other calculation: `its total / 10 + 1 EUR`.
    Calc(Num),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CmpOp {
    AtMost,
    AtLeast,
    MoreThan,
    LessThan,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cond {
    /// `x is y`, `x is not y`
    Is { left: Num, negated: bool, right: Term },
    /// `x is at most y`, `x is at least y`, `x is more than y`, `x is less than y`
    Cmp { left: Num, op: CmpOp, right: Term },
    /// `x has a K [that is s]`, `x has no K [that is s]`
    Has { owner: NounPhrase, negated: bool, kind: String, state: Option<String> },
    /// `x contains y`, `x does not contain y` (appendix F.2.2)
    Contains { set: NounPhrase, item: NounPhrase, negated: bool },
    And(Vec<Cond>),
    Or(Vec<Cond>),
}

/// `[state] Kind [whose attr is v] [that cond]`. Every filter is about the subject.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Subject {
    pub kind: String,
    pub filter: Vec<Cond>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuleBody {
    /// `every S's attr …`, `every S is …`: the condition holds for each subject.
    Every { subject: Subject, cond: Cond },
    /// `no S has … [unless c]`: the predicate holds for no subject, unless `c` does.
    No { subject: Subject, pred: Cond, unless: Option<Cond> },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Module {
    pub name: String,
    pub version: u32,
    pub currency: String,
    /// `, attempts 5` (appendix E.2.8).
    pub attempts: Option<u32>,
    /// `, candidates 8` (appendix F.2.7).
    pub candidates: Option<u32>,
    pub at: Loc,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttrDecl {
    pub name: String,
    pub ty: TypeName,
    pub default: Option<Literal>,
    /// `= order's total`: computed once at creation (appendix E.2.5).
    pub formula: Option<Num>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KindDecl {
    pub name: String,
    /// `kind Person (plural People)`
    pub plural: Option<String>,
    pub attrs: Vec<AttrDecl>,
    pub at: Loc,
}

/// `Delivery is Shipment or Refund` (appendix D).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlotDecl {
    pub name: String,
    pub alternatives: Vec<String>,
    pub at: Loc,
}

/// `prefer Delivery: Shipment, then Refund after 2 days`: each alternative,
/// with the patience written after it (how long to wait on the one before).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreferDecl {
    pub choice: String,
    /// `for a line`: the element of a `uses … per line` (appendix F.2.7).
    pub per: Option<String>,
    pub order: Vec<(String, Option<u64>)>,
    /// For a `uses`: `lowest fee, then those whose …`.
    pub criteria: Vec<Criterion>,
    pub at: Loc,
}

/// How candidates of a `uses` are ranked (appendix F.2.7).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Criterion {
    Lowest(Num),
    Highest(Num),
    /// `those ⟨cond⟩`, `those whose ⟨attr⟩ …`: the candidate is `it`.
    Those(Cond),
}

/// `Shipment uses a Stock per line where …, taking the line's quantity` (appendix F.2.5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsesDecl {
    pub subject: String,
    pub target: String,
    pub per: Option<String>,
    pub cond: Cond,
    pub taking: Option<Num>,
    pub source: String,
    pub at: Loc,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NeedsDecl {
    pub owner: String,
    pub parts: Vec<String>,
    pub cond: Option<Cond>,
    pub at: Loc,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffectDecl {
    pub verb: String,
    pub kind: String,
    pub adapter: String,
    pub within_secs: u64,
    pub sends: Vec<NounPhrase>,
    pub marks: Option<String>,
    pub success: String,
    pub data: Vec<(String, TypeName)>,
    pub failures: Vec<String>,
    pub at: Loc,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuleDecl {
    pub name: String,
    pub body: RuleBody,
    /// `for each Customer and month, …`: one total per period (appendix F.2.12).
    pub period: Option<String>,
    /// `…, checked globally` (appendix F.2.13).
    pub globally: bool,
    /// The sentence as written, for read-backs.
    pub source: String,
    pub at: Loc,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DefineDecl {
    pub kind: String,
    pub state: String,
    pub cond: Cond,
    pub at: Loc,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoalDecl {
    pub subject: Subject,
    pub states: Vec<String>,
    pub within_secs: Option<u64>,
    pub source: String,
    pub at: Loc,
}

/// `its Order is cancelled`, `the Order is not cancelled`
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Action {
    pub target: NounPhrase,
    pub attr: String,
    pub value: bool,
    /// `… becomes ⟨num⟩` (appendix E.2.7); otherwise the action sets a Bool.
    pub becomes: Option<Num>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReactionDecl {
    pub kind: String,
    pub outcome: String,
    pub actions: Vec<Action>,
    pub at: Loc,
}

/// Everything parsed from a module's files.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Source {
    pub module: Option<Module>,
    pub kinds: Vec<KindDecl>,
    pub needs: Vec<NeedsDecl>,
    pub effects: Vec<EffectDecl>,
    pub rules: Vec<RuleDecl>,
    pub defines: Vec<DefineDecl>,
    pub goals: Vec<GoalDecl>,
    pub reactions: Vec<ReactionDecl>,
    pub slots: Vec<SlotDecl>,
    pub prefers: Vec<PreferDecl>,
    pub uses: Vec<UsesDecl>,
}
