//! From syntax to the compiled program (spec §4.1–§4.2, appendix B.3). It
//! resolves every name, checks types, owners and states, and attaches every
//! rule's read-back. Every check is fixed; no model decides anything (D4).

use crate::ast::{self, CmpOp, Head, Literal, Loc, NounPhrase, Source, Term, TypeName};
use crate::ir::{
    self,
    Action, Admit, Admitted, Arith, Attr, Binding, Body, Cond, Criterion, Decision, Define, Effect, Goal, Grant, Kind, Mark, Lit, Need, Num, Op, OwnerRef, Path,
    third_person, Otherwise, Program, Reaction, Role, Rule, Slot, Step, Taking, Ty, CREATED, TIMED_OUT,
};
use crate::lex::Error;
use crate::readback;

/// Checks a parsed module and compiles it, reporting every error it finds.
pub fn check(src: &Source) -> Result<Program, Vec<Error>> {
    let mut cx = Checker::default();
    let prog = cx.program(src);
    if cx.errors.is_empty() { Ok(prog) } else { Err(cx.errors) }
}

/// Resolves a condition about a `kind` against a compiled program, as a
/// scenario's `expect` needs (appendix E.2.9).
pub fn condition(p: &Program, kind: &str, c: &ast::Cond) -> Result<Cond, Vec<Error>> {
    let mut cx = Checker { scenario: true, ..Checker::default() };
    let at = Loc { file: String::new(), line: 0 };
    let c = cx.cond(p, &at, kind, c);
    // A total across units is kept only when the program reads it; one only
    // a scenario reads has no running total to show.
    for b in &cx.contribs {
        if !p.bindings.iter().any(|x| x.subject == b.subject && x.id == b.id) {
            let what = b.taking.as_ref().map_or(b.id.clone(), |t| t.name.clone());
            cx.errors.push(err(&at, format!("the program keeps no total {what} of each {}: no rule, need, admit, definition or formula reads it", b.target)));
        }
    }
    match c {
        Some(c) if cx.errors.is_empty() => Ok(c),
        _ => Err(cx.errors),
    }
}

/// `Refunds`, `Boxes`, `Branches`, `Countries`: the regular plural (spec §3.1).
pub(crate) fn plural_of(name: &str) -> String {
    if let Some(stem) = name.strip_suffix('y').filter(|s| s.ends_with(|c: char| !"AEIOUaeiou".contains(c))) {
        return format!("{stem}ies");
    }
    let es = ["s", "x", "ch", "sh"].iter().any(|e| name.ends_with(e));
    format!("{name}{}", if es { "es" } else { "s" })
}

/// `a Payment`, `an Order`, for where a loop's `a` hides [`a`].
fn a_(k: &str) -> String {
    a(k)
}

/// `a Payment`, `an Order`.
fn a(k: &str) -> String {
    let an = k.starts_with(|c: char| "AEIOUaeiou".contains(c));
    format!("{} {k}", if an { "an" } else { "a" })
}

fn show_num(n: &ast::Num) -> String {
    match n {
        ast::Num::Np(np) => show(np),
        ast::Num::Lit(l) => format!("{l:?}"),
        ast::Num::Bin(x, op, y) => format!("{} {op} {}", show_num(x), show_num(y)),
        ast::Num::Total { attr, of, .. } => format!("the total {attr} of {}", show(of)),
        ast::Num::Number { of, .. } => format!("the number of {}", show(of)),
        ast::Num::Over { calc, list } => format!("the total of {} over {}", show_num(calc), show(list)),
        ast::Num::Taken { attr, from } => format!("the total {attr} taken from {}", show(from)),
        ast::Num::Run { of, .. } => format!("the days in a row of {}", show(of)),
    }
}

/// Why a formula of `kind` cannot be known at creation, if it cannot: it
/// reads a value that an answer sets later, or parts that a need creates
/// only after the entity exists. List elements are given with it, so they can.
fn not_known_at_creation(p: &Program, kind: &str, n: &Num) -> Option<String> {
    match n {
        Num::Lit(_) => None,
        Num::Bin(x, _, y) => not_known_at_creation(p, kind, x).or_else(|| not_known_at_creation(p, kind, y)),
        Num::Path(path) => {
            let (last, rest) = path.steps.split_last()?;
            let Step::Attr(attr) = last else { return None };
            let at = end_kind(p, kind, rest)?;
            // A value read once is computed with once it comes (appendix AG.2.1).
            if let Some(s) = p.sources_of(&at, attr).into_iter().find(|s| s.good_for.is_some()) {
                let every = readback::duration_text(s.good_for.unwrap_or(0) as i64);
                return Some(format!("{attr}, which its source reads again every {every}; a formula is computed once, so it reads only a source with no `good for`"));
            }
            let answered = p.effect_of(&at).is_some_and(|e| e.data.iter().any(|(d, _)| d == attr));
            answered.then(|| format!("{attr}, which an answer sets later"))
        }
        Num::Sum { of, kind: k, .. } | Num::Count { of, kind: k, .. } if p.lists.iter().all(|l| l.element != *k) => {
            let owner = end_kind(p, kind, &of.steps).unwrap_or_else(|| kind.to_string());
            let plural = p.kind(k).map_or_else(|| k.clone(), |x| x.plural.clone());
            Some(format!("its {plural}, which do not exist yet when {} is created", a(&owner)))
        }
        Num::Sum { .. } | Num::Count { .. } => None,
        // Another unit's total is carried, so it is known: the formula keeps
        // its value at creation (appendix AA.2.3).
        Num::Taken { kind: k, .. } if !k.is_empty() => None,
        Num::Taken { name, .. } => Some(format!("the total {name} taken from it, which changes")),
    }
}

/// `Claim's payout`, for the first formula that reads an attribute `attr`.
fn formula_reading(p: &Program, attr: &str) -> Option<String> {
    p.kinds.iter().find_map(|k| {
        k.attrs.iter().find_map(|a| {
            let mut hit = false;
            num_reads(p, a.formula.as_ref()?, &mut |_, last| hit |= last == Some(attr));
            hit.then(|| format!("{}'s {}", k.name, a.name))
        })
    })
}

/// A sourced attribute that `n`, read from a `kind`, reads, if any.
fn sourced_in(p: &Program, kind: &str, n: &Num) -> Option<String> {
    let mut found = None;
    num_reads(p, n, &mut |path, last| {
        let (Some(attr), Some((_, rest))) = (last, path.steps.split_last()) else { return };
        if found.is_none() && end_kind(p, kind, rest).is_some_and(|k| p.source_of(&k, attr).is_some()) {
            found = Some(attr.to_string());
        }
    });
    found
}

/// The kind a walk of parts, slots and owner steps ends at, from `kind`.
fn end_kind(p: &Program, kind: &str, steps: &[Step]) -> Option<String> {
    let mut cur = kind.to_string();
    for s in steps {
        cur = match s {
            Step::Part(k) => k.clone(),
            Step::Owner => p.kind(&cur)?.owner.as_ref()?.kind.clone(),
            Step::Slot(_) | Step::Attr(_) | Step::Ref(_) | Step::Bound(_) | Step::Cand | Step::Elem | Step::Caller | Step::Data(_) => return None,
        };
    }
    Some(cur)
}

/// The kind of the node or shared entity a path to a value's owner ends at,
/// through parts, owners and references.
fn value_kind(p: &Program, kind: &str, steps: &[Step]) -> Option<String> {
    let mut cur = kind.to_string();
    for s in steps {
        cur = match s {
            Step::Part(k) => k.clone(),
            Step::Owner => p.kind(&cur)?.owner.as_ref()?.kind.clone(),
            Step::Ref(a) => p.kind(&cur)?.attr(a)?.of.clone()?,
            _ => return None,
        };
    }
    Some(cur)
}

/// The formula attributes of `kind` that read each other in a circle, if any.
fn formula_cycle(p: &Program, kind: &str) -> Option<Vec<String>> {
    let k = p.kind(kind)?;
    let reads = |n: &Num| {
        let mut out = vec![];
        fn walk(n: &Num, out: &mut Vec<String>) {
            match n {
                Num::Path(Path { steps }) if steps.len() == 1 => {
                    if let Step::Attr(a) = &steps[0] {
                        out.push(a.clone());
                    }
                }
                Num::Bin(x, _, y) => {
                    walk(x, out);
                    walk(y, out);
                }
                _ => {}
            }
        }
        walk(n, &mut out);
        out
    };
    let deps: Vec<(String, Vec<String>)> = k
        .attrs
        .iter()
        .filter_map(|a| a.formula.as_ref().map(|f| (a.name.clone(), reads(f))))
        .collect();
    // Repeatedly drop the formulas that read no other undropped formula.
    let mut left: Vec<&(String, Vec<String>)> = deps.iter().collect();
    loop {
        let before = left.len();
        let names: Vec<String> = left.iter().map(|(n, _)| n.clone()).collect();
        left.retain(|(_, r)| r.iter().any(|x| names.contains(x)));
        if left.is_empty() {
            return None;
        }
        if left.len() == before {
            let mut cycle: Vec<String> = left.iter().map(|(n, _)| n.clone()).collect();
            cycle.sort();
            return Some(cycle);
        }
    }
}

/// The type of `x op y`, by the table of appendix E.2.3.
fn arith_type(op: char, lt: Ty, rt: Ty) -> Result<(Arith, Ty), String> {
    Ok(match (op, lt, rt) {
        ('+', Ty::Money, Ty::Money) => (Arith::Add, Ty::Money),
        ('-', Ty::Money, Ty::Money) => (Arith::Sub, Ty::Money),
        ('+', Ty::Count, Ty::Count) => (Arith::Add, Ty::Count),
        ('-', Ty::Count, Ty::Count) => (Arith::Sub, Ty::Count),
        ('*', Ty::Money, Ty::Count) | ('*', Ty::Count, Ty::Money) => (Arith::Mul, Ty::Money),
        ('*', Ty::Count, Ty::Count) => (Arith::Mul, Ty::Count),
        ('/', Ty::Money, Ty::Count) => (Arith::Div, Ty::Money),
        ('/', Ty::Count, Ty::Count) => (Arith::Div, Ty::Count),
        ('-', Ty::Time, Ty::Time) => (Arith::Sub, Ty::Duration),
        ('+', Ty::Time, Ty::Duration) | ('+', Ty::Duration, Ty::Time) => (Arith::Add, Ty::Time),
        ('-', Ty::Time, Ty::Duration) => (Arith::Sub, Ty::Time),
        ('+', Ty::Duration, Ty::Duration) => (Arith::Add, Ty::Duration),
        ('-', Ty::Duration, Ty::Duration) => (Arith::Sub, Ty::Duration),
        ('*', Ty::Duration, Ty::Count) | ('*', Ty::Count, Ty::Duration) => (Arith::Mul, Ty::Duration),
        ('/', Ty::Duration, Ty::Count) => (Arith::Div, Ty::Duration),
        _ => {
            return Err(match op {
                '+' => format!("cannot add a {lt:?} and a {rt:?}"),
                '-' => format!("cannot subtract a {rt:?} from a {lt:?}"),
                '*' if (lt, rt) == (Ty::Money, Ty::Money) => {
                    "cannot multiply a Money by a Money; for a share of an amount, divide by an amount at once: `a * b / c`".to_string()
                }
                '*' => format!("cannot multiply a {lt:?} by a {rt:?}"),
                _ => format!("cannot divide a {lt:?} by a {rt:?}"),
            })
        }
    })
}

/// The type of `a * b / c`: a share of an amount when all three are
/// amounts (appendix AB.2.1), and otherwise a product, then a quotient.
fn product_quotient(ta: Ty, tb: Ty, tc: Ty) -> Result<Ty, String> {
    if (ta, tb, tc) == (Ty::Money, Ty::Money, Ty::Money) {
        return Ok(Ty::Money);
    }
    let (_, t) = arith_type('*', ta, tb)?;
    arith_type('/', t, tc).map(|(_, ty)| ty)
}

/// The kind of the parts of `owner` whose plural is `word`, if any.
fn plural_kind(p: &Program, owner: &str, word: &str) -> Option<String> {
    let list = p.lists.iter().find(|l| l.owner == owner && l.name == word).map(|l| l.element.clone());
    list.or_else(|| p.kinds.iter().find(|k| k.plural == word && p.has_part(owner, &k.name)).map(|k| k.name.clone()))
}

/// Whether a rule adds up or counts a relation: then its cost is per-group.
fn body_sums(b: &Body) -> bool {
    fn num(n: &Num) -> bool {
        match n {
            Num::Sum { .. } | Num::Count { .. } | Num::Taken { .. } => true,
            Num::Bin(x, _, y) => num(x) || num(y),
            Num::Path(_) | Num::Lit(_) => false,
        }
    }
    fn cond(c: &Cond) -> bool {
        match c {
            Cond::Cmp { left, right, .. } => num(left) || num(right),
            Cond::And(cs) | Cond::Or(cs) => cs.iter().any(cond),
            Cond::State { .. } | Cond::Has { .. } | Cond::Contains { .. } => false,
        }
    }
    match b {
        Body::Every(c) => cond(c),
        Body::No { pred, unless } => cond(pred) || unless.as_ref().is_some_and(cond),
    }
}

/// Where a noun phrase ends: at a node of some kind, at a value of some
/// type, at a reference to a shared entity, or at a set of them.
enum End {
    Node(String),
    Value(Ty),
    Ref(String),
    Set(String),
}

/// Why no sentence reads media (appendix V.2.1).
const MEDIA_READ: &str = "only a decision model sees media; a sentence, a formula, a key or a `sends` cannot read it, so that what a program means never depends on it";

/// Whether a path goes through a reference, to another unit's carried facts.
fn through_ref(path: &Path) -> bool {
    path.steps.iter().any(|s| matches!(s, Step::Ref(_) | Step::Bound(_) | Step::Cand))
}

/// Whether a calculation reads the candidate of a `uses`.
fn reads_cand(n: &Num) -> bool {
    match n {
        Num::Path(p) => p.steps.contains(&Step::Cand),
        Num::Bin(x, _, y) => reads_cand(x) || reads_cand(y),
        _ => false,
    }
}

/// A `uses` condition's pool key (appendix F.2.6): `the C's a is x` or
/// `the C's a contains x`, where `x` does not read the candidate.
fn pool_key(c: &Cond) -> Option<(String, bool, Num)> {
    let cand_attr = |n: &Num| match n {
        Num::Path(Path { steps }) => match steps.as_slice() {
            [Step::Cand, Step::Attr(a)] => Some(a.clone()),
            _ => None,
        },
        _ => None,
    };
    match c {
        Cond::Cmp { left, op: Op::Eq, right } => match (cand_attr(left), cand_attr(right)) {
            (Some(a), _) if !reads_cand(right) => Some((a, false, right.clone())),
            (_, Some(a)) if !reads_cand(left) => Some((a, false, left.clone())),
            _ => None,
        },
        Cond::Contains { set, item, negated: false } if !reads_cand(item) => cand_attr(set).map(|a| (a, true, item.clone())),
        _ => None,
    }
}

/// Inside a `uses` condition or its `prefer` (appendix F.2.4): `the C` is
/// the candidate, and `the line` the element of `per`. In a `prefer`, `it`
/// is the candidate too.
#[derive(Clone)]
struct Scope {
    cand: String,
    it_is_cand: bool,
    /// The names of one element, and its kind.
    elem: Option<(Vec<String>, String)>,
}

/// A path to a shared entity as the reference that leads to it, for
/// comparing identities: `[…, Ref(customer)]` becomes `[…, Attr(customer)]`.
fn as_reference(path: &Path) -> Option<Path> {
    let (last, rest) = path.steps.split_last()?;
    let Step::Ref(a) = last else { return None };
    let mut steps = rest.to_vec();
    steps.push(Step::Attr(a.clone()));
    Some(Path { steps })
}

/// Whether amounts of this type compare with at most, at least and so on.
fn ordered(ty: Ty) -> bool {
    matches!(ty, Ty::Money | Ty::Count | Ty::Time | Ty::Duration)
}

fn err(at: &Loc, msg: impl Into<String>) -> Error {
    Error { file: at.file.clone(), line: at.line, msg: msg.into() }
}

fn value_type(t: &TypeName) -> Option<Ty> {
    match t {
        TypeName::Text => Some(Ty::Text),
        TypeName::Bool => Some(Ty::Bool),
        TypeName::Money => Some(Ty::Money),
        TypeName::Count => Some(Ty::Count),
        TypeName::Time => Some(Ty::Time),
        TypeName::Duration => Some(Ty::Duration),
        TypeName::Media => Some(Ty::Media),
        TypeName::Kind(_) | TypeName::List(_) | TypeName::Set(_) => None,
    }
}

/// A noun phrase as written, for messages.
fn show(np: &NounPhrase) -> String {
    let mut s = match (&np.head, np.steps.is_empty()) {
        (Head::It, true) => "it".to_string(),
        (Head::It, false) if np.steps[0] == "caller" => "the".to_string(),
        (Head::It, false) => "its".to_string(),
        (Head::The(k), _) => format!("the {k}"),
        (Head::Each(k), _) => format!("each {k}"),
        (Head::All(k), _) => format!("all {k}"),
    };
    for (i, w) in np.steps.iter().enumerate() {
        s.push_str(if i == 0 && matches!(np.head, Head::It) { " " } else { "'s " });
        s.push_str(w);
    }
    s
}

#[derive(Default)]
struct Checker {
    /// While a scenario's `expect` is checked: dates may be written.
    scenario: bool,
    /// While an adapter's `sends` or a secret's formula is checked: secrets may be read.
    sending: bool,
    /// While a model source's `sends` is checked: media may be read (appendix AF.2.2).
    reading: bool,
    /// The marked attributes read while `sending`, as (kind, attribute, mark).
    marked: Vec<(String, String, Mark)>,
    /// The grants an effect or a source used, by index into `allows`.
    granted: Vec<(usize, usize)>,
    errors: Vec<Error>,
    scope: Option<Scope>,
    /// The period of the rule being checked, for its totals across units.
    period: Option<String>,
    /// And the attributes each entity spans the periods with (appendix AD.2.1).
    span: Option<(String, String)>,
    /// The implicit bindings of relations across units, found while checking rules.
    contribs: Vec<Binding>,
    /// In an `admit` condition: the kind of the caller's unit, or empty when
    /// one of its roles has no kind (appendix G.2.3).
    caller: Option<String>,
    /// In an `admit` condition or an event's reaction: the data the event
    /// carries (appendix Q.2.2).
    data: Vec<(String, Ty)>,
}

/// A relation across units (appendix F.2.11): from a Customer, `their
/// Refunds` when Refund is no part of it and has one attribute of kind
/// Customer; from the global unit `all Refunds`, `all Refunds`. The kind
/// related, and the attribute that points at `from` (none for `all`).
fn across(p: &Program, from: &str, of: &NounPhrase) -> Option<(String, Option<String>)> {
    match (&of.head, of.steps.as_slice()) {
        (Head::All(w), []) if from == format!("all {w}") => p.kinds.iter().find(|k| k.plural == *w).map(|k| (k.name.clone(), None)),
        (Head::It | Head::Each(_) | Head::The(_), [w]) => {
            if let Head::Each(k) | Head::The(k) = &of.head
                && k != from {
                return None;
            }
            if plural_kind(p, from, w).is_some() {
                return None;
            }
            let r = p.kinds.iter().find(|k| k.plural == *w)?;
            let refs: Vec<&Attr> = r.attrs.iter().filter(|a| a.ty == Ty::Ref && a.of.as_deref() == Some(from)).collect();
            (refs.len() == 1).then(|| (r.name.clone(), Some(refs[0].name.clone())))
        }
        _ => None,
    }
}

/// Attributes that change after creation: set by a reaction, an effect's
/// answer or an admitted change (appendix G.2.6).
fn changeable(p: &Program) -> Vec<String> {
    let mut out: Vec<String> = p.reactions.iter().flat_map(|r| r.actions.iter().map(|a| a.attr.clone())).collect();
    out.extend(p.effects.iter().flat_map(|e| e.data.iter().map(|(n, _)| n.clone())));
    // A value read once, and not again for a rule, stays as it came (appendix AG.2.3).
    out.extend(p.sources.iter().filter(|s| !read_once(p, &s.kind, &s.attr)).map(|s| s.attr.clone()));
    out.extend(admitted_changes(p).into_iter().map(|(_, a)| a));
    out
}

/// Whether a sourced attribute is read once: no source of it is good for a
/// while, and no rule wants it fresh, so it is read again by none.
fn read_once(p: &Program, kind: &str, attr: &str) -> bool {
    p.sources_of(kind, attr).iter().all(|s| s.good_for.is_none()) && !p.rules.iter().any(|r| r.fresh.iter().any(|(a, _)| a == attr))
}

/// The attributes callers may change, as (kind, attribute).
fn admitted_changes(p: &Program) -> Vec<(String, String)> {
    p.admits
        .iter()
        .filter_map(|a| match &a.what {
            Admitted::Change { kind, attr } => Some((kind.clone(), attr.clone())),
            _ => None,
        })
        .collect()
}

/// Why an attribute cannot be changed by a caller in slice 6 (appendix G.2.5).
pub fn unchangeable(p: &Program, kind: &str, attr: &str) -> Option<String> {
    if attr == CREATED {
        return Some(format!("`{CREATED}` is the clock when the {kind} was created, so it never changes"));
    }
    let Some(a) = p.kind(kind).and_then(|k| k.attr(attr)) else {
        return Some(format!("{} has no attribute `{attr}`", self::a(kind)));
    };
    if matches!(a.ty, Ty::Ref | Ty::Set) {
        return Some(format!("`{attr}` refers to shared entities; changing a reference or a set waits for a later slice"));
    }
    if a.formula.is_some() {
        return Some(format!("`{attr}` is calculated when the {kind} is created, so it cannot be changed"));
    }
    if p.source_of(kind, attr).is_some() {
        return Some(format!("`{attr}` comes from its source, so it cannot be changed"));
    }
    if let Some(b) = p.bindings.iter().find(|b| b.target == kind && b.key_attr == attr) {
        return Some(format!("`{attr}` keys the pool of `{} uses a {kind}`, and changing a pool key waits for a later slice", b.subject));
    }
    if let Some(b) = p.bindings.iter().find(|b| key_reads(p, b).iter().any(|(k, a)| k == kind && a == attr)) {
        return Some(format!("`{attr}` is the key `{} uses a {}` looks up its pool by, and changing a pool key waits for a later slice", b.subject, b.target));
    }
    None
}

/// The attributes a binding's key reads, as (kind, attribute), through
/// owners and references from its subject.
fn key_reads(p: &Program, b: &Binding) -> Vec<(String, String)> {
    fn num(p: &Program, from: &str, n: &Num, out: &mut Vec<(String, String)>) {
        match n {
            Num::Path(x) => {
                let mut cur = from.to_string();
                for s in &x.steps {
                    match s {
                        Step::Owner => match p.kind(&cur).and_then(|k| k.owner.clone()) {
                            Some(o) => cur = o.kind,
                            None => return,
                        },
                        Step::Ref(a) => match p.kind(&cur).and_then(|k| k.attr(a)).and_then(|a| a.of.clone()) {
                            Some(k) => cur = k,
                            None => return,
                        },
                        Step::Attr(a) => out.push((cur.clone(), a.clone())),
                        _ => return,
                    }
                }
            }
            Num::Bin(x, _, y) => {
                num(p, from, x, out);
                num(p, from, y, out);
            }
            _ => {}
        }
    }
    let mut out = vec![];
    num(p, &b.subject, &b.key, &mut out);
    out
}

/// The changeable attributes of other units a rule reads through carried
/// facts: each needs a watch (appendix G.2.7).
fn watched(p: &Program, r: &Rule) -> Vec<String> {
    carried_reads(p, r, &admitted_changes(p))
}

/// The attributes a reaction or an answer sets, as (kind, attribute), with
/// an empty kind: carried copies of these are never refreshed in v0.
fn set_inside(p: &Program) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = p.reactions.iter().flat_map(|r| r.actions.iter().map(|a| (String::new(), a.attr.clone()))).collect();
    out.extend(p.effects.iter().flat_map(|e| e.data.iter().map(|(n, _)| (String::new(), n.clone()))));
    out
}

/// The totals across units that `r` reads from another unit, directly or
/// through definitions (appendix AA.2.4), as (kind, total).
fn others_totals(p: &Program, r: &Rule) -> Vec<(String, String)> {
    fn num(p: &Program, n: &Num, seen: &mut Vec<String>, out: &mut Vec<(String, String)>) {
        match n {
            Num::Taken { kind, name, .. } if !kind.is_empty() => {
                let t = (kind.clone(), name.clone());
                if !out.contains(&t) {
                    out.push(t);
                }
            }
            Num::Bin(a, _, b) => {
                num(p, a, seen, out);
                num(p, b, seen, out);
            }
            Num::Sum { calc, filter, .. } => {
                num(p, calc, seen, out);
                if let Some(f) = filter {
                    cond(p, f, seen, out);
                }
            }
            Num::Count { filter: Some(f), .. } => cond(p, f, seen, out),
            _ => {}
        }
    }
    fn cond(p: &Program, c: &Cond, seen: &mut Vec<String>, out: &mut Vec<(String, String)>) {
        match c {
            Cond::Cmp { left, right, .. } | Cond::Contains { set: left, item: right, .. } => {
                num(p, left, seen, out);
                num(p, right, seen, out);
            }
            Cond::State { state, .. } | Cond::Has { state: Some(state), .. } => {
                if !seen.contains(state) {
                    seen.push(state.clone());
                    for d in p.defines.iter().filter(|d| d.state == *state) {
                        cond(p, &d.cond, seen, out);
                    }
                }
            }
            Cond::Has { .. } => {}
            Cond::And(cs) | Cond::Or(cs) => cs.iter().for_each(|c| cond(p, c, seen, out)),
        }
    }
    let (mut seen, mut out) = (vec![], vec![]);
    for c in &r.filter {
        cond(p, c, &mut seen, &mut out);
    }
    match &r.body {
        Body::Every(c) => cond(p, c, &mut seen, &mut out),
        Body::No { pred, unless } => {
            cond(p, pred, &mut seen, &mut out);
            if let Some(u) = unless {
                cond(p, u, &mut seen, &mut out);
            }
        }
    }
    out
}

/// The attributes in `changes` that `r` reads from another unit, through a
/// reference or a binding, named `K's a` (or `a`).
fn carried_reads(p: &Program, r: &Rule, changes: &[(String, String)]) -> Vec<String> {
    let mut out = vec![];
    let mut see = |x: &Path, last: Option<&str>| {
        if !x.steps.iter().any(|s| matches!(s, Step::Ref(_) | Step::Bound(_))) {
            return;
        }
        if let Some(a) = last {
            for (k, c) in changes {
                let name = if k.is_empty() { c.clone() } else { format!("{k}'s {c}") };
                if c == a && !out.contains(&name) {
                    out.push(name);
                }
            }
        }
    };
    rule_reads(p, r, &mut see);
    out
}

/// Calls `see` for every path a rule reads, with its last attribute, or for
/// a state, the state's name.
fn rule_reads(p: &Program, r: &Rule, see: &mut dyn FnMut(&Path, Option<&str>)) {
    for c in &r.filter {
        cond_reads(p, c, see);
    }
    match &r.body {
        Body::Every(c) => cond_reads(p, c, see),
        Body::No { pred, unless } => {
            cond_reads(p, pred, see);
            if let Some(u) = unless {
                cond_reads(p, u, see);
            }
        }
    }
}

fn num_reads(p: &Program, n: &Num, see: &mut dyn FnMut(&Path, Option<&str>)) {
    match n {
        Num::Path(x) => see(x, match x.steps.last() { Some(Step::Attr(a)) => Some(a.as_str()), _ => None }),
        Num::Bin(a, _, b) => {
            num_reads(p, a, see);
            num_reads(p, b, see);
        }
        Num::Sum { calc, filter, .. } => {
            num_reads(p, calc, see);
            if let Some(f) = filter {
                cond_reads(p, f, see);
            }
        }
        Num::Count { filter: Some(f), .. } => cond_reads(p, f, see),
        _ => {}
    }
}

fn cond_reads(p: &Program, c: &Cond, see: &mut dyn FnMut(&Path, Option<&str>)) {
    match c {
        Cond::Cmp { left, right, .. } => {
            num_reads(p, left, see);
            num_reads(p, right, see);
        }
        Cond::Contains { set, item, .. } => {
            num_reads(p, set, see);
            num_reads(p, item, see);
        }
        Cond::State { node, state, .. } => p.state_attrs(state).into_iter().for_each(|a| see(node, Some(a))),
        Cond::Has { state: Some(st), node, .. } => p.state_attrs(st).into_iter().for_each(|a| see(node, Some(a))),
        Cond::Has { .. } => {}
        Cond::And(cs) | Cond::Or(cs) => cs.iter().for_each(|c| cond_reads(p, c, see)),
    }
}

/// The sourced attributes a rule reads, directly or through definitions
/// (appendix I.2.4) and formulas (appendix AF), as (kind, attribute). Names are matched by attribute,
/// so a rule may be strict about more than it needs, never less.
fn sourced_reads(p: &Program, r: &Rule) -> Vec<(String, String)> {
    let mut names: Vec<String> = vec![];
    rule_reads(p, r, &mut |_, last| {
        if let Some(a) = last {
            names.push(a.to_string());
        }
    });
    sourced_through(p, names)
}

/// The sourced attributes the amount a binding takes reads, through
/// formulas (appendix AG.2.2).
fn taking_sources(p: &Program, b: &Binding) -> Vec<(String, String)> {
    let mut names: Vec<String> = vec![];
    if let Some(t) = &b.taking {
        num_reads(p, &t.amount, &mut |_, last| {
            if let Some(a) = last {
                names.push(a.to_string());
            }
        });
    }
    sourced_through(p, names)
}

/// The sourced attributes among `names` and what they read, through
/// definitions and formulas.
fn sourced_through(p: &Program, mut names: Vec<String>) -> Vec<(String, String)> {
    let mut states: Vec<String> = vec![];
    // A state may be a definition, which reads further.
    let mut i = 0;
    while i < names.len() {
        let n = names[i].clone();
        if !states.contains(&n) {
            states.push(n.clone());
            for d in p.defines.iter().filter(|d| d.state == n) {
                cond_reads(p, &d.cond, &mut |_, last| {
                    if let Some(a) = last {
                        names.push(a.to_string());
                    }
                });
            }
            // `payout: Money = amount - …` reads `amount`.
            for f in p.kinds.iter().flat_map(|k| k.attrs.iter()).filter(|a| a.name == n).filter_map(|a| a.formula.as_ref()) {
                num_reads(p, f, &mut |_, last| {
                    if let Some(a) = last {
                        names.push(a.to_string());
                    }
                });
            }
        }
        i += 1;
    }
    let mut out: Vec<(String, String)> = vec![];
    for s in p.sources.iter().filter(|s| names.contains(&s.attr)) {
        // An attribute with several sources is read once.
        let read = (s.kind.clone(), s.attr.clone());
        if !out.contains(&read) {
            out.push(read);
        }
    }
    out
}

/// The attributes that may change: [`changeable`] ones, and formulas that
/// read one, a part, a slot, a binding or a total. A formula is set once it can be
/// known, so such a formula may be unknown when a rule is first checked and
/// known later (appendix Y.2.2).
fn changeable_formulas(p: &Program) -> Vec<String> {
    fn reads(n: &Num, ch: &[String]) -> bool {
        match n {
            Num::Path(x) => {
                matches!(x.steps.last(), Some(Step::Attr(a)) if ch.contains(a))
                    || x.steps.iter().any(|s| matches!(s, Step::Part(_) | Step::Slot(_) | Step::Bound(_) | Step::Cand | Step::Elem | Step::Caller | Step::Data(_)))
            }
            Num::Lit(_) => false,
            Num::Bin(a, _, b) => reads(a, ch) || reads(b, ch),
            Num::Sum { .. } | Num::Count { .. } | Num::Taken { .. } => true,
        }
    }
    let mut ch = changeable(p);
    loop {
        let more: Vec<String> = p
            .kinds
            .iter()
            .flat_map(|k| &k.attrs)
            .filter(|a| !ch.contains(&a.name) && a.formula.as_ref().is_some_and(|f| reads(f, &ch)))
            .map(|a| a.name.clone())
            .collect();
        if more.is_empty() {
            return ch;
        }
        ch.extend(more);
    }
}

/// The parts a `no … has` rule refuses, each as the path to its owner and
/// its kind.
fn refused_parts(r: &Rule) -> Vec<(Vec<Step>, String)> {
    fn has(c: &Cond, out: &mut Vec<(Vec<Step>, String)>) {
        match c {
            Cond::Has { node, part, negated: false, .. } => out.push((node.steps.clone(), part.clone())),
            Cond::And(cs) => cs.iter().for_each(|c| has(c, out)),
            // Under an `or`, a branch that is no part may break the rule
            // while no part exists: then no part is known to exist.
            Cond::Or(cs) => {
                let mut each = vec![];
                for c in cs {
                    let before = each.len();
                    has(c, &mut each);
                    if each.len() == before {
                        out.push((vec![Step::Cand], String::new()));
                        return;
                    }
                }
                out.extend(each);
            }
            _ => {}
        }
    }
    let mut out = vec![];
    if let Body::No { pred, .. } = &r.body {
        has(pred, &mut out);
    }
    out
}

/// Whether a path to a node reaches only fixed nodes when the rule is broken:
/// its unit's own, or the part the rule refuses, which then exists. Through
/// `its Refund`, every part the rule refuses must be a Refund there; through
/// `its Payback`, an alternative of that slot (appendix Y.2.2).
fn fixed_node(p: &Program, parts: &[(Vec<Step>, String)], to: &[Step]) -> bool {
    if to.iter().all(|s| *s == Step::Owner) {
        return true;
    }
    let Some((last, owner)) = to.split_last() else { return true };
    !parts.is_empty()
        && parts.iter().all(|(o, k)| {
            o.as_slice() == owner
                && match last {
                    Step::Part(x) => x == k,
                    Step::Slot(x) => p.slot(x).is_some_and(|s| s.alternatives.contains(k)),
                    _ => false,
                }
        })
}

/// Whether a rule's filter and `unless` read only facts that never change
/// (appendix F.2.15): carried facts, attributes no reaction or answer sets,
/// and those of the part it refuses (appendix Y.2.2).
fn fixed_rule(p: &Program, r: &Rule) -> bool {
    let ch = changeable_formulas(p);
    let parts = refused_parts(r);
    let path = |x: &Path| {
        // A bound entity is known only once its binding is granted, after the
        // part exists, so only references are fixed.
        if x.steps.iter().any(|s| matches!(s, Step::Bound(_) | Step::Cand)) {
            return false;
        }
        if through_ref(x) {
            return !matches!(x.steps.last(), Some(Step::Attr(a)) if ch.contains(a));
        }
        match x.steps.split_last() {
            Some((Step::Attr(a), to)) => !ch.contains(a) && fixed_node(p, &parts, to),
            _ => false,
        }
    };
    fn num(n: &Num, path: &dyn Fn(&Path) -> bool) -> bool {
        match n {
            Num::Path(x) => path(x),
            Num::Lit(_) => true,
            Num::Bin(a, _, b) => num(a, path) && num(b, path),
            Num::Sum { .. } | Num::Count { .. } | Num::Taken { .. } => false,
        }
    }
    fn cond(p: &Program, c: &Cond, ch: &[String], parts: &[(Vec<Step>, String)], path: &dyn Fn(&Path) -> bool) -> bool {
        match c {
            Cond::Cmp { left, right, .. } => num(left, path) && num(right, path),
            Cond::Contains { set, item, .. } => num(set, path) && num(item, path),
            Cond::State { node, state, .. } => {
                let attrs = p.state_attrs(state);
                let value_attr = p.kinds.iter().any(|k| k.attr(state).is_some_and(|a| a.ty == Ty::Bool) || k.state_attr(state).is_some());
                let other = p.defines.iter().any(|d| d.state == *state)
                    || p.effects.iter().any(|e| e.marks.as_deref() == Some(state.as_str()) || e.success == *state || e.failures.contains(state))
                    || state == "done"
                    || state == "failed"
                    || state == TIMED_OUT;
                let carried = through_ref(node) && !node.steps.iter().any(|s| matches!(s, Step::Bound(_) | Step::Cand)) && !attrs.iter().any(|a| ch.iter().any(|c| c == a));
                carried || (value_attr && !other && !attrs.iter().any(|a| ch.iter().any(|c| c == a)) && fixed_node(p, parts, &node.steps))
            }
            Cond::Has { .. } => false,
            Cond::And(cs) | Cond::Or(cs) => cs.iter().all(|c| cond(p, c, ch, parts, path)),
        }
    }
    r.filter.iter().all(|c| cond(p, c, &ch, &parts, &path))
        && match &r.body {
            Body::Every(c) => cond(p, c, &ch, &parts, &path),
            Body::No { unless, .. } => unless.as_ref().is_none_or(|c| cond(p, c, &ch, &parts, &path)),
        }
}

/// What a rule on facts that never change leaves waiting for good: a part
/// it refuses that is no slot's alternative, or is the last one (appendix Y.2.3).
fn waits_for_good(p: &Program, r: &Rule) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    let mut seen: Vec<String> = vec![];
    for (_, k) in refused_parts(r) {
        if seen.contains(&k) {
            continue;
        }
        seen.push(k.clone());
        let slot = p.slots.iter().find(|s| s.alternatives.contains(&k));
        if slot.is_some_and(|s| s.alternatives.last() != Some(&k)) {
            continue;
        }
        let part = slot.map_or(k.clone(), |s| s.name.clone());
        let Some(need) = p.needs.iter().find(|n| n.part == part) else { continue };
        let last = slot.map_or(String::new(), |s| format!(", and {k} is the last alternative of {}", s.name));
        out.push(format!(
            "rule {}: {} it refuses waits for good, because the rule reads only facts that never change{last}. If none is wanted then, say so on the need: `{} needs {part} while …`",
            r.name,
            a(&k),
            need.owner
        ));
    }
    out
}

/// Whether a calculation reads a total per period.
fn per_period(b: &Body) -> bool {
    fn num(n: &Num) -> bool {
        match n {
            Num::Taken { period, .. } => period.is_some(),
            Num::Bin(x, _, y) => num(x) || num(y),
            _ => false,
        }
    }
    fn cond(c: &Cond) -> bool {
        match c {
            Cond::Cmp { left, right, .. } => num(left) || num(right),
            Cond::And(cs) | Cond::Or(cs) => cs.iter().any(cond),
            _ => false,
        }
    }
    match b {
        Body::Every(c) => cond(c),
        Body::No { pred, unless } => cond(pred) || unless.as_ref().is_some_and(cond),
    }
}

impl Checker {
    fn program(&mut self, src: &Source) -> Program {
        let (module, version, currency) = match &src.module {
            Some(m) => (m.name.clone(), m.version, m.currency.clone()),
            None => {
                self.errors.push(Error { file: String::new(), line: 0, msg: "no `module` statement".into() });
                (String::new(), 0, String::new())
            }
        };
        let mut p = Program {
            module,
            version,
            currency,
            kinds: vec![],
            needs: vec![],
            effects: vec![],
            sources: vec![],
            decisions: vec![],
            rules: vec![],
            defines: vec![],
            goals: vec![],
            reactions: vec![],
            slots: vec![],
            lists: vec![],
            bindings: vec![],
            globals: vec![],
            roles: vec![],
            admits: vec![],
            attempts: src.module.as_ref().and_then(|m| m.attempts).unwrap_or(3),
            candidates: src.module.as_ref().and_then(|m| m.candidates).unwrap_or(8),
            warnings: vec![],
            grants: vec![],
        };
        self.kinds(src, &mut p);
        self.slots(src, &mut p);
        self.lists(src, &mut p);
        let conds = self.owners(src, &mut p);
        self.refs(src, &mut p);
        self.bindings(src, &mut p);
        self.prefers(src, &mut p);
        self.effects(src, &mut p);
        self.sources(src, &mut p);
        self.list_elements(src, &p);
        self.state_names(src, &p);
        self.formulas(src, &mut p);
        self.defines(src, &mut p);
        for (i, owner, at, cond) in conds {
            let c = self.cond(&p, at, &owner, cond);
            p.needs[i].cond = c;
        }
        self.rules(src, &mut p);
        self.decides(src, &mut p);
        self.goals(src, &mut p);
        self.roles(src, &mut p);
        self.admits(src, &mut p);
        self.reactions(src, &mut p);
        // Totals across units read outside rules: in formulas, definitions,
        // needs and admits (appendix AA.2.3).
        self.add_contribs(&mut p);
        for i in 0..p.bindings.len() {
            p.bindings[i].sources = taking_sources(&p, &p.bindings[i]);
        }
        for i in 0..p.rules.len() {
            p.rules[i].fixed = fixed_rule(&p, &p.rules[i]);
            if p.rules[i].fixed {
                let w = waits_for_good(&p, &p.rules[i]);
                p.warnings.extend(w);
            }
            p.rules[i].sources = sourced_reads(&p, &p.rules[i]);
            for (attr, secs) in p.rules[i].fresh.clone() {
                let r = &p.rules[i];
                let at = src.rules.iter().find(|x| x.name == r.name).map_or_else(|| Loc { file: String::new(), line: 0 }, |x| x.at.clone());
                let old = readback::duration_text(secs as i64);
                if !r.sources.iter().any(|(_, a)| *a == attr) {
                    self.errors.push(err(&at, format!("rule {}: `with {attr} at most {old} old`: the rule reads no sourced `{attr}`", r.name)));
                } else if let Some(f) = formula_reading(&p, &attr) {
                    // Read again, it would leave the formula's value behind (appendix AG.2.1).
                    self.errors.push(err(&at, format!("rule {}: `with {attr} at most {old} old`: the formula of {f} reads {attr} once, so it is not read again", r.name)));
                }
            }
            // A carried fact that the other unit's own reactions or answers
            // change is never refreshed in v0 (appendix G.1), so the rule
            // could be broken without anyone knowing: refuse it.
            // Other units change a total across units without the reader's
            // rules being checked (appendix AA.2.4).
            for (kind, total) in others_totals(&p, &p.rules[i]) {
                let r = &p.rules[i];
                let at = src.rules.iter().find(|x| x.name == r.name).map_or_else(|| Loc { file: String::new(), line: 0 }, |x| x.at.clone());
                self.errors.push(err(
                    &at,
                    format!(
                        "rule {}: it reads `{total}`, a total of each {kind} that other units change without this rule being checked. State the rule about each {kind}, or read the total in a need, an admit or a definition no rule uses",
                        r.name
                    ),
                ));
            }
            let stale = carried_reads(&p, &p.rules[i], &set_inside(&p));
            if !stale.is_empty() {
                let r = &p.rules[i];
                let at = src.rules.iter().find(|x| x.name == r.name).map_or_else(|| Loc { file: String::new(), line: 0 }, |x| x.at.clone());
                self.errors.push(err(
                    &at,
                    format!(
                        "rule {}: it reads `{}` from another unit, which a reaction or an answer changes there; v0 refreshes such copies only after an `admit change`, so the rule could break unseen. Carry the fact with a formula, or change it with `admit change`",
                        r.name,
                        stale.join("`, `")
                    ),
                ));
            }
            let w = watched(&p, &p.rules[i]);
            if !w.is_empty() && p.rules[i].tier == "local" {
                p.rules[i].tier = format!("per-group (watches {})", w.join(", "));
            }
        }
        // After the tiers are known: only a local rule can apply from a version.
        self.applies(src, &mut p);
        self.grants(src, &mut p);
        p
    }

    /// The marked attributes just read by an adapter's `sends` need a grant
    /// to that adapter (appendix N.2.3).
    fn grants_for(&mut self, src: &Source, at: &Loc, adapter: &str) {
        for (kind, attr, m) in std::mem::take(&mut self.marked) {
            let found = src.allows.iter().enumerate().find_map(|(i, g)| {
                (g.adapter == adapter).then(|| g.fields.iter().position(|(k, a)| *k == kind && *a == attr).map(|j| (i, j))).flatten()
            });
            match found {
                Some(ij) => self.granted.push(ij),
                None => {
                    let what = if m == Mark::Secret { "secret" } else { "personal" };
                    self.errors.push(err(at, format!("{kind}'s {attr} is {what}, and {adapter} is not allowed to receive it; write `allow {adapter} to receive {kind}'s {attr}`")));
                }
            }
        }
    }

    /// `allow …`: each field must be a marked attribute, and is listed in the
    /// manifest. One that no effect or source sends is a warning.
    fn grants(&mut self, src: &Source, p: &mut Program) {
        let adapters: Vec<&str> = p.effects.iter().map(|e| e.adapter.as_str()).chain(p.sources.iter().map(|s| s.adapter.as_str())).collect();
        let mut out = vec![];
        for (i, g) in src.allows.iter().enumerate() {
            if !adapters.contains(&g.adapter.as_str()) {
                self.errors.push(err(&g.at, format!("no effect or source is via {}", g.adapter)));
                continue;
            }
            for (j, (kind, attr)) in g.fields.iter().enumerate() {
                let Some(k) = p.kind(kind) else {
                    self.errors.push(err(&g.at, format!("unknown kind {kind}")));
                    continue;
                };
                match k.attr(attr) {
                    None => self.errors.push(err(&g.at, format!("{} has no attribute {attr}", a(kind)))),
                    Some(x) if x.mark.is_none() => {
                        self.errors.push(err(&g.at, format!("{kind}'s {attr} is not marked secret or personal, so it needs no `allow`")))
                    }
                    Some(_) => {
                        if !self.granted.contains(&(i, j)) {
                            p.warnings.push(format!("`allow {} to receive {kind}'s {attr}`: no effect or source via {} sends it", g.adapter, g.adapter));
                        }
                        out.push(Grant { adapter: g.adapter.clone(), kind: kind.clone(), attr: attr.clone() });
                    }
                }
            }
        }
        p.grants = out;
    }

    /// `apply rule R to Ks created from version n` (appendix M.2.3).
    fn applies(&mut self, src: &Source, p: &mut Program) {
        for a in &src.applies {
            let Some(i) = p.rules.iter().position(|r| r.name == a.rule) else {
                self.errors.push(err(&a.at, format!("there is no rule {}", a.rule)));
                continue;
            };
            let Some(kind) = p.kinds.iter().find(|k| k.plural == a.plural).map(|k| k.name.clone()) else {
                self.errors.push(err(&a.at, format!("`{}` is not the plural of a kind", a.plural)));
                continue;
            };
            let r = &p.rules[i];
            if r.applies.is_some() {
                self.errors.push(err(&a.at, format!("rule {} already applies from a version", r.name)));
                continue;
            }
            if r.tier != "local" {
                self.errors.push(err(&a.at, format!("rule {} is {}, and only a rule about one entity at a time (local) can apply from a version", r.name, r.tier)));
                continue;
            }
            let chain = p.owner_chain(&r.subject);
            if !chain.contains(&kind) || p.kind(&kind).is_some_and(|k| k.owner.is_some()) {
                self.errors.push(err(&a.at, format!("rule {} is about {}; it can apply to the {} that hold them, not to {}", r.name, plural_name(p, &r.subject), plural_name(p, chain.last().expect("not empty")), a.plural)));
                continue;
            }
            if a.from == 0 || a.from > p.version {
                self.errors.push(err(&a.at, format!("version {} is not from 1 to this module's version, {}", a.from, p.version)));
                continue;
            }
            p.rules[i].applies = Some(crate::ir::Applies { kind, from: a.from, source: a.source.clone() });
        }
    }

    fn kinds(&mut self, src: &Source, p: &mut Program) {
        for k in &src.kinds {
            if p.kind(&k.name).is_some() {
                self.errors.push(err(&k.at, format!("kind {} is declared twice", k.name)));
                continue;
            }
            let mut seen: Vec<&str> = vec![];
            let mut attrs = vec![];
            for a in &k.attrs {
                if seen.contains(&a.name.as_str()) {
                    self.errors.push(err(&k.at, format!("{} has two attributes called {}", k.name, a.name)));
                    continue;
                }
                seen.push(&a.name);
                if a.name == "done" || a.name == "failed" {
                    self.errors.push(err(&k.at, format!("{}.{}: `{}` is a state of every kind", k.name, a.name, a.name)));
                    continue;
                }
                if a.name == CREATED {
                    self.errors.push(err(&k.at, format!("{}.{CREATED}: every entity has `{CREATED}`, the time it was created", k.name)));
                    continue;
                }
                let Some(ty) = value_type(&a.ty) else { continue }; // an owner reference: see `owners`
                if !a.states.is_empty() && !self.states_ok(k, a) {
                    continue;
                }
                let default = match &a.default {
                    None => None,
                    Some(l) => match self.lit(&k.at, l, &p.currency) {
                        Some((lit, t)) if t == ty => Some(lit),
                        Some((_, t)) => {
                            self.errors.push(err(&k.at, format!("{}.{} is {ty:?} but its default is {t:?}", k.name, a.name)));
                            None
                        }
                        None => None,
                    },
                };
                if let Some(m) = a.mark {
                    if m == Mark::Secret && ty != Ty::Text {
                        self.errors.push(err(&k.at, format!("{}.{} is {ty:?}; only a Text attribute can be secret", k.name, a.name)));
                    }
                    if matches!(ty, Ty::Ref | Ty::Set) {
                        self.errors.push(err(&k.at, format!("{}.{} refers to shared entities; only a value can be marked", k.name, a.name)));
                    }
                    if m == Mark::Secret && default.is_some() {
                        self.errors.push(err(&k.at, format!("{}.{} is secret, so it has no default", k.name, a.name)));
                    }
                }
                attrs.push(Attr { name: a.name.clone(), ty, of: None, default, formula: None, mark: a.mark, states: a.states.clone() });
            }
            let plural = k.plural.clone().unwrap_or_else(|| plural_of(&k.name));
            // `all Boxes` and the like must name one kind (appendix T.2.3).
            if let Some(o) = p.kinds.iter().find(|o| o.plural == plural || o.name == plural) {
                self.errors.push(err(&k.at, format!("{}'s plural is {plural}, which {} already is; give one of them another with `(plural …)`", k.name, if o.name == plural { format!("the kind {}", o.name) } else { format!("the plural of {}", o.name) })));
            } else if let Some(o) = src.kinds.iter().find(|o| o.name != k.name && o.name == plural) {
                self.errors.push(err(&k.at, format!("{}'s plural is {plural}, which is the kind {}; give it another with `(plural …)`", k.name, o.name)));
            }
            p.kinds.push(Kind { name: k.name.clone(), plural, attrs, owner: None });
        }
    }

    /// A state attribute's states (appendix Z.2.1): each a new name on its
    /// kind, and its default one of them.
    fn states_ok(&mut self, k: &ast::KindDecl, a: &ast::AttrDecl) -> bool {
        let mut ok = true;
        for (i, st) in a.states.iter().enumerate() {
            let msg = if st == "done" || st == "failed" {
                Some(format!("{}.{}: `{st}` is a state of every kind", k.name, a.name))
            } else if a.states[..i].contains(st) {
                Some(format!("{}.{} lists the state {st} twice", k.name, a.name))
            } else if k.attrs.iter().any(|x| x.name == *st) {
                Some(format!("{}.{}: {} has an attribute called {st}, so it cannot also be a state", k.name, a.name, k.name))
            } else {
                k.attrs.iter().take_while(|x| x.name != a.name).find(|x| x.states.contains(st)).map(|x| format!("{}.{}: {st} is already a state of {}.{}", k.name, a.name, k.name, x.name))
            };
            if let Some(m) = msg {
                self.errors.push(err(&k.at, m));
                ok = false;
            }
        }
        if let Some(Literal::Text(d)) = &a.default
            && !a.states.contains(d)
        {
            self.errors.push(err(&k.at, format!("{}.{} starts in {d}, which is not one of its states: {}", k.name, a.name, a.states.join(", "))));
            ok = false;
        }
        ok
    }

    /// A literal's value and type. Money must be in the module's currency.
    fn lit(&mut self, at: &Loc, l: &Literal, currency: &str) -> Option<(Lit, Ty)> {
        Some(match l {
            Literal::Text(s) => (Lit::Text(s.clone()), Ty::Text),
            Literal::Bool(b) => (Lit::Bool(*b), Ty::Bool),
            Literal::Count(n) => (Lit::Count(*n), Ty::Count),
            Literal::Duration(secs) => (Lit::Duration(*secs), Ty::Duration),
            Literal::Time(t) if self.scenario => (Lit::Time(*t), Ty::Time),
            Literal::Time(_) => {
                // A served program's clock counts from when its log started
                // (appendix AB.2.3), so a date means something only in scenarios.
                self.errors.push(err(at, "a date is written only in scenarios: a program's times come from `created` and the clock"));
                return None;
            }
            Literal::Set(_) => {
                self.errors.push(err(at, "a set of names is written only in scenarios"));
                return None;
            }
            Literal::File(_) => {
                self.errors.push(err(at, "a media file is given only in scenarios and inputs"));
                return None;
            }
            Literal::Money { cents, currency: c } if c == currency => (Lit::Money(*cents), Ty::Money),
            Literal::Money { currency: c, .. } => {
                self.errors.push(err(at, format!("an amount in {c} is not in this module's currency, {currency}")));
                return None;
            }
        })
    }

    /// Who owns which part (spec §3.2: a part has exactly one attribute of its
    /// owner's kind). Returns the needs' conditions, which are resolved once
    /// effects and definitions are known.
    fn owners<'s>(&mut self, src: &'s Source, p: &mut Program) -> Vec<(usize, String, &'s Loc, &'s ast::Cond)> {
        let mut conds = vec![];
        for n in &src.needs {
            if p.kind(&n.owner).is_none() {
                self.errors.push(err(&n.at, format!("unknown kind {}", n.owner)));
                continue;
            }
            for part in &n.parts {
                if p.needs.iter().any(|x| x.owner == n.owner && x.part == *part) {
                    self.errors.push(err(&n.at, format!("{} needs {part} twice; give one need, with its conditions joined by `or`", n.owner)));
                    continue;
                }
                if let Some(i) = p.slots.iter().position(|s| s.name == *part) {
                    if !p.slots[i].owner.is_empty() && p.slots[i].owner != n.owner {
                        self.errors.push(err(&n.at, format!("the slot {part} is already needed by {}; a slot has one owner", p.slots[i].owner)));
                        continue;
                    }
                    p.slots[i].owner = n.owner.clone();
                    let alternatives = p.slots[i].alternatives.clone();
                    if !alternatives.iter().all(|a| self.link(src, p, &n.at, &n.owner, a)) {
                        continue;
                    }
                } else if !self.link(src, p, &n.at, &n.owner, part) {
                    continue;
                }
                if let Some(c) = &n.cond {
                    conds.push((p.needs.len(), n.owner.clone(), &n.at, c));
                }
                p.needs.push(Need { owner: n.owner.clone(), part: part.clone(), cond: None });
            }
        }
        for k in &p.kinds {
            let mut seen = vec![k.name.clone()];
            let mut cur = k.owner.as_ref().map(|o| o.kind.clone());
            while let Some(c) = cur {
                if seen.contains(&c) {
                    self.errors.push(Error { file: String::new(), line: 0, msg: format!("needs loop: {} → {c}", seen.join(" → ")) });
                    break;
                }
                seen.push(c.clone());
                cur = p.kind(&c).and_then(|x| x.owner.as_ref()).map(|o| o.kind.clone());
            }
        }
        conds
    }

    /// References and sets (appendix F.2.2): every attribute whose type is a
    /// kind, other than the one that names the owner. It must refer to a root
    /// kind, a shared entity; a part lives inside its owner's unit.
    fn refs(&mut self, src: &Source, p: &mut Program) {
        for k in &src.kinds {
            for a in &k.attrs {
                let (target, ty) = match &a.ty {
                    TypeName::Kind(t) => (t, Ty::Ref),
                    TypeName::Set(t) => (t, Ty::Set),
                    _ => continue,
                };
                let me = p.kind(&k.name);
                if ty == Ty::Ref && me.and_then(|x| x.owner.as_ref()).is_some_and(|o| o.attr == a.name) {
                    continue;
                }
                if me.is_some_and(|x| x.attr(&a.name).is_some()) {
                    continue; // declared twice: reported in `kinds`
                }
                match p.kind(target).map(|t| t.owner.as_ref().map(|o| o.kind.clone())) {
                    None => {
                        self.errors.push(err(&k.at, format!("{}.{}: unknown kind {target}", k.name, a.name)));
                        continue;
                    }
                    Some(Some(owner)) => {
                        self.errors.push(err(
                            &k.at,
                            format!("{}.{}: {target} is a part of {owner}, so nothing outside {} can refer to it", k.name, a.name, a_(&owner)),
                        ));
                        continue;
                    }
                    Some(None) => {}
                }
                if a.default.is_some() {
                    self.errors.push(err(&k.at, format!("{}.{} refers to {}; it has no default", k.name, a.name, a_(target))));
                }
                let kind = p.kinds.iter_mut().find(|x| x.name == k.name).expect("declared kinds are compiled");
                kind.attrs.push(Attr { name: a.name.clone(), ty, of: Some(target.clone()), default: None, formula: None, mark: None, states: vec![] });
                if a.mark.is_some() {
                    self.errors.push(err(&k.at, format!("{}.{} refers to shared entities; only a value can be marked", k.name, a.name)));
                }
            }
        }
    }

    /// Makes `part` a part of `owner`: it must name its owner with exactly one
    /// attribute, and a part has one owner.
    fn link(&mut self, src: &Source, p: &mut Program, at: &Loc, owner: &str, part: &str) -> bool {
        let Some(decl) = src.kinds.iter().find(|k| k.name == part) else {
            self.errors.push(err(at, format!("unknown kind {part}")));
            return false;
        };
        let owner_type = TypeName::Kind(owner.to_string());
        let refs: Vec<&str> = decl.attrs.iter().filter(|a| a.ty == owner_type).map(|a| a.name.as_str()).collect();
        if refs.len() != 1 {
            self.errors.push(err(at, format!("{part} needs exactly one attribute of kind {owner} to name its owner; it has {}", refs.len())));
            return false;
        }
        let kind = p.kinds.iter_mut().find(|k| k.name == part).expect("declared kinds are compiled");
        if let Some(o) = &kind.owner
            && o.kind != owner {
            self.errors.push(err(at, format!("{part} is already a part of {}; a part has one owner", o.kind)));
            return false;
        }
        kind.owner = Some(OwnerRef { attr: refs[0].to_string(), kind: owner.to_string() });
        true
    }

    /// `lines: list of Line`: the element kind names its owner like a part,
    /// and has no needs or effects of its own in slice 4 (appendix E.2.6).
    fn lists(&mut self, src: &Source, p: &mut Program) {
        for k in &src.kinds {
            for a in &k.attrs {
                let TypeName::List(element) = &a.ty else { continue };
                if p.lists.iter().any(|l| l.owner == k.name && l.element == *element) {
                    self.errors.push(err(&k.at, format!("{} has two lists of {element}; a kind is the element of one list", k.name)));
                    continue;
                }
                if self.link(src, p, &k.at, &k.name, element) {
                    p.lists.push(crate::ir::List { owner: k.name.clone(), name: a.name.clone(), element: element.clone() });
                }
            }
        }
    }

    fn list_elements(&mut self, src: &Source, p: &Program) {
        for l in &p.lists {
            let at = src.kinds.iter().find(|k| k.name == l.element).map_or(Loc { file: String::new(), line: 0 }, |k| k.at.clone());
            if p.needs.iter().any(|n| n.owner == l.element) || src.effects.iter().any(|e| e.kind == l.element) {
                self.errors.push(err(&at, format!("{} is an element of a list; in slice 4 it has no needs or effects of its own", l.element)));
            }
        }
    }

    /// Formula attributes, once owners, lists and effects are known (appendix E.2.5).
    fn formulas(&mut self, src: &Source, p: &mut Program) {
        for k in &src.kinds {
            for a in &k.attrs {
                let Some(f) = &a.formula else { continue };
                let Some(attr) = p.kind(&k.name).and_then(|x| x.attr(&a.name)).cloned() else { continue };
                let want = attr.ty;
                if want == Ty::Set {
                    self.errors.push(err(&k.at, format!("{}.{} is a set; a set has no formula", k.name, a.name)));
                    continue;
                }
                if want == Ty::Ref {
                    // A reference's formula copies another reference: `country: Country = customer's country`.
                    let target = attr.of.clone().unwrap_or_default();
                    let found = match f {
                        ast::Num::Np(np) => Some(self.np(p, &k.at, &k.name, np)),
                        _ => None,
                    };
                    match found {
                        Some(Some((path, End::Ref(t)))) if t == target => {
                            let kind = p.kinds.iter_mut().find(|x| x.name == k.name).expect("declared kinds are compiled");
                            if let Some(x) = kind.attrs.iter_mut().find(|x| x.name == a.name) {
                                x.formula = Some(Num::Path(path));
                            }
                        }
                        Some(None) => {} // the noun phrase's own error is reported
                        _ => self.errors.push(err(
                            &k.at,
                            format!("{}.{} refers to {}, so its formula must name one, as in `customer's {}`", k.name, a.name, a_(&target), a.name),
                        )),
                    }
                    continue;
                }
                // A secret's formula may copy a secret; nothing else may read one.
                self.sending = attr.mark == Some(Mark::Secret);
                let num = self.num(p, &k.at, &k.name, f);
                self.sending = false;
                let Some((num, ty)) = num else { continue };
                if ty != want {
                    self.errors.push(err(&k.at, format!("{}.{} is {want:?}, but its formula gives a {ty:?}", k.name, a.name)));
                    continue;
                }
                if let Some(why) = not_known_at_creation(p, &k.name, &num) {
                    let why = if why.contains("computed once") { why } else { format!("{why}; a formula is computed when the entity is created") };
                    self.errors.push(err(&k.at, format!("{}.{} reads {why}", k.name, a.name)));
                    continue;
                }
                let kind = p.kinds.iter_mut().find(|x| x.name == k.name).expect("declared kinds are compiled");
                if let Some(x) = kind.attrs.iter_mut().find(|x| x.name == a.name) {
                    x.formula = Some(num);
                }
            }
            if let Some(cycle) = formula_cycle(p, &k.name) {
                self.errors.push(err(&k.at, format!("the formulas of {} depend on each other: {}", k.name, cycle.join(", "))));
            }
        }
    }

    /// Slots, before needs give them owners (appendix D.2).
    fn slots(&mut self, src: &Source, p: &mut Program) {
        for s in &src.slots {
            if p.kind(&s.name).is_some() || p.slot(&s.name).is_some() {
                self.errors.push(err(&s.at, format!("{} is already declared; a slot needs a name of its own", s.name)));
                continue;
            }
            let n = s.alternatives.len();
            p.slots.push(Slot { name: s.name.clone(), owner: String::new(), alternatives: s.alternatives.clone(), patience: vec![None; n] });
        }
    }

    /// Each slot's `prefer` order and patience, or a warning without one.
    fn prefers(&mut self, src: &Source, p: &mut Program) {
        // One `prefer` per choice, or the second would silently win or lose (appendix T.2.3).
        for (i, pr) in src.prefers.iter().enumerate() {
            if let Some(first) = src.prefers[..i].iter().find(|o| o.choice == pr.choice && o.per == pr.per) {
                let per = pr.per.as_ref().map_or(String::new(), |l| format!(" for a {l}"));
                self.errors.push(err(&pr.at, format!("prefer {}{per} is written twice; the first is at line {}", pr.choice, first.at.line)));
            }
        }
        for (s, decl) in p.slots.iter().zip(&src.slots) {
            if s.owner.is_empty() {
                self.errors.push(err(&decl.at, format!("no kind needs the slot {}", s.name)));
            }
            for a in &s.alternatives {
                if p.needs.iter().any(|n| n.owner == s.owner && n.part == *a) {
                    self.errors.push(err(&decl.at, format!("{a} is an alternative of {}, so {} cannot also need it directly", s.name, s.owner)));
                }
            }
        }
        for pr in src.prefers.iter().filter(|pr| pr.criteria.is_empty() && !p.bindings.iter().any(|b| b.target == pr.choice)) {
            let Some(i) = p.slots.iter().position(|s| s.name == pr.choice) else {
                self.errors.push(err(&pr.at, format!("prefer {}: there is no slot called {}", pr.choice, pr.choice)));
                continue;
            };
            let s = &p.slots[i];
            let mut listed: Vec<&String> = pr.order.iter().map(|(k, _)| k).collect();
            listed.sort();
            let mut want: Vec<&String> = s.alternatives.iter().collect();
            want.sort();
            if listed != want {
                self.errors.push(err(&pr.at, format!("prefer {} must list each of {} once", s.name, s.alternatives.join(", "))));
                continue;
            }
            let alternatives: Vec<String> = pr.order.iter().map(|(k, _)| k.clone()).collect();
            let mut patience: Vec<Option<u64>> = pr.order.iter().skip(1).map(|(_, after)| *after).collect();
            patience.push(None);
            p.slots[i].alternatives = alternatives;
            p.slots[i].patience = patience;
        }
        for s in &p.slots {
            if !src.prefers.iter().any(|pr| pr.choice == s.name) {
                p.warnings.push(format!("{} has no `prefer`; its alternatives are tried in the order declared: {}", s.name, s.alternatives.join(", ")));
            }
        }
    }

    /// `uses` bindings (appendix F.2.5–F.2.8), and the `prefer` that ranks each.
    fn bindings(&mut self, src: &Source, p: &mut Program) {
        for u in &src.uses {
            let Some(subject) = p.kind(&u.subject) else {
                self.errors.push(err(&u.at, format!("unknown kind {}", u.subject)));
                continue;
            };
            match p.kind(&u.target) {
                None => {
                    self.errors.push(err(&u.at, format!("unknown kind {}", u.target)));
                    continue;
                }
                Some(t) if t.owner.is_some() => {
                    self.errors.push(err(&u.at, format!("{} is a part, so it is not shared; a `uses` binds to a root kind", u.target)));
                    continue;
                }
                Some(_) => {}
            }
            if subject.owner.is_none() {
                self.errors.push(err(&u.at, format!("{} is a root kind; in slice 5 a part of a unit `uses` a shared entity", u.subject)));
                continue;
            }
            let (per, elem) = match &u.per {
                None => (None, None),
                Some(w) => {
                    let mut chain = vec![u.subject.clone()];
                    while let Some(o) = p.kind(chain.last().expect("never empty")).and_then(|x| x.owner.as_ref()) {
                        if chain.len() > p.kinds.len() {
                            break;
                        }
                        chain.push(o.kind.clone());
                    }
                    let Some(l) = chain.iter().find_map(|k| p.lists.iter().find(|l| l.owner == *k && l.singulars().contains(w))) else {
                        self.errors.push(err(&u.at, format!("`per {w}`: neither {} nor an owner of it has a list of {w}s", a(&u.subject))));
                        continue;
                    };
                    (Some(l.name.clone()), Some((l.singulars(), l.element.clone())))
                }
            };
            self.scope = Some(Scope { cand: u.target.clone(), it_is_cand: false, elem: elem.clone() });
            let cond = self.cond(p, &u.at, &u.subject, &u.cond);
            let taking = match &u.taking {
                Some(n) => {
                    self.scope = Some(Scope { cand: String::new(), it_is_cand: false, elem: elem.clone() });
                    let name = match n {
                        ast::Num::Np(np) => np.steps.last().cloned(),
                        // `taking 1`: a count, named after the kind that takes it, so
                        // `the total repairs taken from each Garage` (appendix W.2.3).
                        ast::Num::Lit(ast::Literal::Count(_)) => p.kind(&u.subject).map(|k| k.plural.to_lowercase()),
                        _ => None,
                    };
                    let t = self.num(p, &u.at, &u.subject, n);
                    match (name, t) {
                        (Some(name), Some((amount, ty @ (Ty::Money | Ty::Count)))) => Some(Some(Taking { name, amount, ty, period: None, span: None })),
                        (None, Some(_)) => {
                            self.errors.push(err(&u.at, format!("`taking {}`: name the attribute taken, as in `taking the line's quantity`, or take a count, as in `taking 1`", show_num(n))));
                            None
                        }
                        (_, Some((_, ty))) => {
                            self.errors.push(err(&u.at, format!("`taking {}` is {ty:?}; only amounts and counts are taken", show_num(n))));
                            None
                        }
                        (_, None) => None,
                    }
                }
                None => Some(None),
            };
            self.scope = None;
            let (Some(cond), Some(taking)) = (cond, taking) else { continue };
            let parts = match cond {
                Cond::And(cs) => cs,
                c => vec![c],
            };
            let Some(i) = parts.iter().position(|c| pool_key(c).is_some()) else {
                let t = &u.target;
                self.errors.push(err(
                    &u.at,
                    format!("`{} uses a {t}` needs a pool key: one `and` part `the {t}'s ⟨attribute⟩ is …` or `the {t}'s ⟨attribute⟩ contains …` that does not read the {t}", u.subject),
                ));
                continue;
            };
            let (key_attr, key_member, key) = pool_key(&parts[i]).expect("found above");
            let filter = parts.into_iter().enumerate().filter(|(j, _)| *j != i).map(|(_, c)| c).collect();
            p.bindings.push(Binding {
                id: u.target.clone(),
                subject: u.subject.clone(),
                target: u.target.clone(),
                per,
                key_attr,
                key_member,
                key,
                filter,
                taking,
                prefer: vec![],
                source: u.source.clone(),
                held: None,
                shared: false,
                sources: vec![],
            });
        }
        for pr in &src.prefers {
            let bound: Vec<usize> = (0..p.bindings.len()).filter(|&i| p.bindings[i].target == pr.choice).collect();
            if bound.is_empty() {
                if let (false, Some(slot)) = (pr.criteria.is_empty(), p.slot(&pr.choice)) {
                    self.errors.push(err(&pr.at, format!("prefer {}: {} is a slot; list its alternatives in order: {}", pr.choice, pr.choice, slot.alternatives.join(", "))));
                } else if !pr.criteria.is_empty() {
                    self.errors.push(err(&pr.at, format!("prefer {}: no kind `uses` a {}; `lowest`, `highest` and `those` rank shared entities", pr.choice, pr.choice)));
                }
                continue;
            }
            if pr.criteria.is_empty() {
                self.errors.push(err(&pr.at, format!("prefer {}: rank the {} with `lowest …`, `highest …` or `those …`", pr.choice, p.kind(&pr.choice).map_or(pr.choice.clone(), |k| k.plural.clone()))));
                continue;
            }
            let matching: Vec<usize> = bound
                .into_iter()
                .filter(|&i| match (&pr.per, &p.bindings[i].per) {
                    (None, None) => true,
                    (Some(w), Some(list)) => p.lists.iter().any(|l| l.name == *list && l.singulars().contains(w)),
                    _ => false,
                })
                .collect();
            let Some(&i) = matching.first() else {
                let hint = p.bindings.iter().find(|b| b.target == pr.choice).and_then(|b| b.per.clone());
                let want = hint.and_then(|l| p.lists.iter().find(|x| x.name == l).map(|x| format!("prefer {} for a {}", pr.choice, x.singular())));
                self.errors.push(err(&pr.at, format!("prefer {}: it does not match how its `uses` binds{}", pr.choice, want.map(|w| format!("; write `{w}`")).unwrap_or_default())));
                continue;
            };
            if matching.len() > 1 {
                self.errors.push(err(&pr.at, format!("prefer {}: more than one kind uses a {}; slice 5 ranks one binding per kind", pr.choice, pr.choice)));
                continue;
            }
            let b = &p.bindings[i];
            let elem = b.per.as_ref().and_then(|l| p.lists.iter().find(|x| x.name == *l)).map(|l| (l.singulars(), l.element.clone()));
            let subject = b.subject.clone();
            self.scope = Some(Scope { cand: pr.choice.clone(), it_is_cand: true, elem });
            let mut criteria = vec![];
            for c in &pr.criteria {
                let done = match c {
                    ast::Criterion::Lowest(n) | ast::Criterion::Highest(n) => match self.num(p, &pr.at, &subject, n) {
                        Some((num, ty)) if ordered(ty) => Some(if matches!(c, ast::Criterion::Lowest(_)) { Criterion::Lowest(num) } else { Criterion::Highest(num) }),
                        Some((_, ty)) => {
                            self.errors.push(err(&pr.at, format!("`{}` is {ty:?}; `lowest` and `highest` rank amounts, counts, times and durations", show_num(n))));
                            None
                        }
                        None => None,
                    },
                    ast::Criterion::Those(cond) => self.cond(p, &pr.at, &subject, cond).map(Criterion::Those),
                };
                criteria.extend(done);
            }
            self.scope = None;
            if criteria.len() == pr.criteria.len() {
                p.bindings[i].prefer = criteria;
            }
        }
        for b in &p.bindings {
            if !src.prefers.iter().any(|pr| pr.choice == b.target) {
                p.warnings.push(format!("{} has no `prefer`; {} are chosen in the order they were created", b.target, p.kind(&b.target).map_or(b.target.clone(), |k| k.plural.clone())));
            }
        }
    }

    fn effects(&mut self, src: &Source, p: &mut Program) {
        for e in &src.effects {
            if p.kind(&e.kind).is_none() {
                self.errors.push(err(&e.at, format!("unknown kind {}", e.kind)));
                continue;
            }
            if p.effect_of(&e.kind).is_some() {
                self.errors.push(err(&e.at, format!("{} already has an effect; slice 1 allows one per kind", e.kind)));
                continue;
            }
            let mut data = vec![];
            for (name, t) in &e.data {
                let Some(ty) = value_type(t) else {
                    self.errors.push(err(&e.at, format!("an answer carries values, not a {t:?}")));
                    continue;
                };
                let kind = p.kinds.iter_mut().find(|k| k.name == e.kind).expect("checked above");
                if let Some(x) = kind.state_attr(name) {
                    self.errors.push(err(&e.at, format!("{name} is a state of {}.{}, so an answer cannot also carry it", e.kind, x.name)));
                    continue;
                }
                match kind.attr(name).map(|a| a.ty) {
                    Some(have) if have != ty => {
                        self.errors.push(err(&e.at, format!("{}.{name} is {have:?}, but the answer gives {ty:?}", e.kind)));
                        continue;
                    }
                    Some(_) => {}
                    None => kind.attrs.push(Attr { name: name.clone(), ty, of: None, default: None, formula: None, mark: None, states: vec![] }),
                }
                data.push((name.clone(), ty));
            }
            let mut sends = vec![];
            self.sending = true;
            self.marked.clear();
            for np in &e.sends {
                match self.np(p, &e.at, &e.kind, np) {
                    Some((path, End::Value(_) | End::Ref(_) | End::Set(_))) => sends.push((np.steps.last().cloned().unwrap_or_default(), path)),
                    Some((_, End::Node(k))) => {
                        self.errors.push(err(&e.at, format!("an effect sends values; `{}` is a {k}", show(np))));
                    }
                    None => {}
                }
            }
            self.sending = false;
            self.grants_for(src, &e.at, &e.adapter);
            p.effects.push(Effect {
                kind: e.kind.clone(),
                verb: e.verb.clone(),
                adapter: e.adapter.clone(),
                within_secs: e.within_secs,
                sends,
                marks: e.marks.clone(),
                success: e.success.clone(),
                data,
                failures: e.failures.clone(),
            });
        }
    }

    /// Which model handles which choices (appendix J.2.1–J.2.3).
    fn decides(&mut self, src: &Source, p: &mut Program) {
        // A step waits for its model, so the wait is bounded (appendix K.2.5).
        const MAX_WITHIN_MS: u64 = 10 * 60_000;
        let points = p.choice_points();
        let mut kept: Vec<(usize, &ast::DecideDecl)> = vec![];
        for d in &src.decides {
            if let Some(point) = &d.point
                && !points.iter().any(|c| c.name == *point) {
                let msg = match point.split_once("'s ") {
                    Some((k, x)) if p.source_of(k, x).is_some() => format!("{point} has one source, so there is no choice to decide"),
                    Some(_) => format!("{point} has no sources, so there is no choice to decide"),
                    None => format!("no `uses`, slot or source picks {}, so there is no choice to decide", a(point)),
                };
                self.errors.push(err(&d.at, msg));
                continue;
            }
            let scope = match &d.scope {
                None => None,
                Some(w) => match p.kinds.iter().find(|k| k.plural == *w || k.name == *w) {
                    Some(k) => Some(k.name.clone()),
                    None => {
                        self.errors.push(err(&d.at, format!("unknown kind `{w}`")));
                        continue;
                    }
                },
            };
            if let Some(k) = &scope {
                let inside = points.iter().any(|c| d.point.as_ref().is_none_or(|x| *x == c.name) && p.owner_chain(&c.chooser).contains(k));
                if !inside {
                    let what = d.point.as_deref().map_or_else(|| "choice is made".to_string(), |x| format!("{x} is chosen"));
                    self.errors.push(err(&d.at, format!("no {what} inside {}", a(k))));
                    continue;
                }
            }
            if let Some(m) = &d.model {
                let ok = m.split_once('@').is_some_and(|(n, v)| !n.is_empty() && !v.is_empty() && !v.contains('@'));
                if !ok {
                    self.errors.push(err(&d.at, format!("a model is named `name@version`, as in \"{m}@1\"")));
                    continue;
                }
            }
            let cond = match (&d.cond, &scope) {
                (Some(c), Some(k)) => match self.cond(p, &d.at, k, c) {
                    Some(c) => Some(c),
                    None => continue,
                },
                (None, _) => None,
                (Some(_), None) => unreachable!("the parser reads `whose` only after `for`"),
            };
            let scope_text = d.scope.as_ref().map(|w| {
                let whose = d.source.split_once(" whose ").and_then(|(_, r)| r.rsplit_once(" with ")).map(|(c, _)| format!(" whose {c}"));
                format!("{w}{}", whose.unwrap_or_default())
            });
            kept.push((p.decisions.len(), d));
            let within_ms = d.within_ms.unwrap_or(1000);
            if !(1..=MAX_WITHIN_MS).contains(&within_ms) {
                self.errors.push(err(&d.at, format!("a model's `within` is from 1 millisecond to {} minutes", MAX_WITHIN_MS / 60_000)));
                continue;
            }
            p.decisions.push(Decision { point: d.point.clone(), scope, cond, model: d.model.clone(), scope_text, within_ms, source: d.source.clone() });
        }
        // Two that can both decide one choice, at the same rank (J.2.3).
        let mut seen = vec![];
        for c in &points {
            let chain = p.owner_chain(&c.chooser);
            let rank = |x: &Decision| -> Option<(bool, usize, bool)> {
                let near = match &x.scope {
                    None => 0,
                    Some(k) => chain.len() - chain.iter().position(|c| c == k)?,
                };
                x.point.as_ref().is_none_or(|q| *q == c.name).then_some((x.point.is_some(), near, x.cond.is_some()))
            };
            for (i, (di, ai)) in kept.iter().enumerate() {
                for (dj, aj) in &kept[i + 1..] {
                    let (x, y) = (&p.decisions[*di], &p.decisions[*dj]);
                    if rank(x).is_some() && rank(x) == rank(y) && !seen.contains(&(*di, *dj)) {
                        seen.push((*di, *dj));
                        self.errors.push(err(
                            &aj.at,
                            format!(
                                "`{}` and `{}` (line {}) can both decide {}, and neither is more specific",
                                y.source,
                                x.source,
                                ai.at.line,
                                match c.name.split_once("'s ") {
                                    Some((k, attr)) => format!("the source of {}'s {attr}", a(k)),
                                    None => format!("{}'s {}", a(&c.chooser), c.name),
                                }
                            ),
                        ));
                    }
                }
            }
        }
    }

    /// Sourced attributes (appendix I.2.1–I.2.2).
    fn sources(&mut self, src: &Source, p: &mut Program) {
        for s in &src.sources {
            let Some(k) = src.kinds.iter().find(|k| k.name == s.kind) else {
                self.errors.push(err(&s.at, format!("unknown kind {}", s.kind)));
                continue;
            };
            let Some(attr) = p.kind(&s.kind).and_then(|k| k.attr(&s.attr)).cloned() else {
                self.errors.push(err(&s.at, format!("{} has no attribute `{}` to source", a(&s.kind), s.attr)));
                continue;
            };
            let decl = k.attrs.iter().find(|x| x.name == s.attr);
            if decl.is_some_and(|x| x.formula.is_some()) || attr.formula.is_some() {
                self.errors.push(err(&s.at, format!("{}.{} is calculated by its formula, so it cannot be sourced", s.kind, s.attr)));
                continue;
            }
            if attr.default.is_some() {
                self.errors.push(err(&s.at, format!("{}.{} has a default, so it cannot be sourced", s.kind, s.attr)));
                continue;
            }
            if matches!(attr.ty, Ty::Ref | Ty::Set) {
                self.errors.push(err(&s.at, format!("{}.{} refers to shared entities; a source gives a value", s.kind, s.attr)));
                continue;
            }
            if s.model && attr.ty == Ty::Media {
                self.errors.push(err(&s.at, format!("{}.{} is media; a model reads values out of media, it does not make it", s.kind, s.attr)));
                continue;
            }
            if attr.ty == Ty::Bool {
                self.errors.push(err(&s.at, format!("{}.{} is a state; v0 sources values, not states: source a Text or Count and compare it", s.kind, s.attr)));
                continue;
            }
            if p.sources_of(&s.kind, &s.attr).into_iter().any(|x| x.adapter == s.adapter) {
                self.errors.push(err(&s.at, format!("{}.{} already has a source via {}", s.kind, s.attr, s.adapter)));
                continue;
            }
            if s.otherwise.is_some() && *p.otherwise_of(&s.kind, &s.attr) != Otherwise::Hold {
                self.errors.push(err(&s.at, format!("{}.{} already has an `otherwise`; it applies when every source has failed, so give it once", s.kind, s.attr)));
                continue;
            }
            if p.effects.iter().any(|e| e.kind == s.kind && e.data.iter().any(|(n, _)| *n == s.attr)) {
                self.errors.push(err(&s.at, format!("{}.{} is set by its effect's answer, so it cannot also be sourced", s.kind, s.attr)));
                continue;
            }
            let referrer = p.kinds.iter().find(|x| x.attrs.iter().any(|y| matches!(y.ty, Ty::Ref | Ty::Set) && y.of.as_deref() == Some(s.kind.as_str())));
            if let Some(r) = referrer {
                self.errors.push(err(&s.at, format!("{} is shared ({} refers to it), so its {} cannot be sourced in v0", s.kind, r.name, s.attr)));
                continue;
            }
            if p.bindings.iter().any(|b| b.target == s.kind) {
                self.errors.push(err(&s.at, format!("{} is shared (a `uses` picks it), so its {} cannot be sourced in v0", s.kind, s.attr)));
                continue;
            }
            if let Some((extra, _)) = s.data.iter().find(|(n, _)| *n != s.attr) {
                self.errors.push(err(&s.at, format!("a source's answer carries only {}; drop `with {extra}`", s.attr)));
                continue;
            }
            if attr.mark == Some(Mark::Secret) {
                self.errors.push(err(&s.at, format!("{}.{} is secret, so it cannot be sourced", s.kind, s.attr)));
                continue;
            }
            let mut sends = vec![];
            self.sending = true;
            self.reading = s.model;
            self.marked.clear();
            for np in &s.sends {
                match self.np(p, &s.at, &s.kind, np) {
                    Some((path, End::Value(_) | End::Ref(_) | End::Set(_))) => sends.push((np.steps.last().cloned().unwrap_or_default(), path)),
                    Some((_, End::Node(k))) => self.errors.push(err(&s.at, format!("a source sends values; `{}` is a {k}", show(np)))),
                    None => {}
                }
            }
            self.sending = false;
            self.reading = false;
            if s.model {
                // A model is not granted marked fields in v0 (appendix AF.2.2).
                if let Some((kind, attr, m)) = std::mem::take(&mut self.marked).into_iter().next() {
                    let what = if m == Mark::Secret { "secret" } else { "personal" };
                    self.errors.push(err(&s.at, format!("{kind}'s {attr} is {what}, and a model is not sent marked attributes")));
                }
            } else {
                self.grants_for(src, &s.at, &s.adapter);
            }
            let otherwise = match &s.otherwise {
                None => Otherwise::Hold,
                Some(ast::OtherwiseDecl::Again(secs)) => Otherwise::Again(*secs),
                Some(ast::OtherwiseDecl::Use(l)) => {
                    let currency = p.currency.clone();
                    let Some((lit, ty)) = self.lit(&s.at, l, &currency) else { continue };
                    if ty != attr.ty {
                        self.errors.push(err(&s.at, format!("{}.{} is {:?}, but `otherwise use` gives {ty:?}", s.kind, s.attr, attr.ty)));
                        continue;
                    }
                    Otherwise::Use(lit)
                }
                Some(ast::OtherwiseDecl::Act(actions)) => {
                    let sourced = |a: &ast::Action| src.sources.iter().any(|x| x.attr == a.attr && (x.kind == s.kind || !a.target.steps.is_empty()));
                    if let Some(a) = actions.iter().find(|a| sourced(a)) {
                        self.errors.push(err(&s.at, format!("`{}` comes from a source; `otherwise` cannot set it", a.attr)));
                        continue;
                    }
                    let r = ast::ReactionDecl { kind: s.kind.clone(), outcome: String::new(), by: vec![], actions: actions.clone(), at: s.at.clone() };
                    Otherwise::Act(self.actions(p, &r))
                }
            };
            p.sources.push(ir::Source {
                kind: s.kind.clone(),
                attr: s.attr.clone(),
                ty: attr.ty,
                adapter: s.adapter.clone(),
                model: s.model,
                asking: s.asking.clone(),
                sure: s.sure,
                within_secs: s.within_secs,
                sends,
                success: s.success.clone(),
                failures: s.failures.clone(),
                good_for: s.good_for,
                otherwise,
            });
        }
    }

    /// A state name has one meaning in a kind (appendix B.3): its effect's
    /// marks and outcomes may not repeat each other, an attribute, `done` or
    /// `failed`.
    fn state_names(&mut self, src: &Source, p: &Program) {
        for e in &src.effects {
            let Some(kind) = p.kind(&e.kind) else { continue };
            let mut seen: Vec<&str> = vec![];
            for name in e.marks.iter().chain(std::iter::once(&e.success)).chain(&e.failures) {
                if name == "done" || name == "failed" {
                    self.errors.push(err(&e.at, format!("`{name}` is a state of every kind; {}'s effect cannot mark or report it", e.kind)));
                } else if kind.attr(name).is_some() {
                    self.errors.push(err(&e.at, format!("{} has an attribute called {name}, so its effect cannot also mark or report {name}", e.kind)));
                } else if let Some(x) = kind.state_attr(name) {
                    self.errors.push(err(&e.at, format!("{name} is a state of {}.{}, so its effect cannot also mark or report it", e.kind, x.name)));
                } else if seen.contains(&name.as_str()) {
                    self.errors.push(err(&e.at, format!("{}'s effect uses the name {name} twice", e.kind)));
                }
                seen.push(name);
            }
        }
    }

    /// Definitions are resolved in order, so a definition can use only the
    /// states defined before it. That rules out cycles.
    fn defines(&mut self, src: &Source, p: &mut Program) {
        for d in &src.defines {
            let Some(k) = p.kind(&d.kind) else {
                self.errors.push(err(&d.at, format!("unknown kind {}", d.kind)));
                continue;
            };
            if k.attr(&d.state).is_some() || p.states(&d.kind).contains(&d.state) {
                self.errors.push(err(&d.at, format!("{} already has a state or attribute called `{}`", d.kind, d.state)));
                continue;
            }
            let Some(cond) = self.cond(p, &d.at, &d.kind, &d.cond) else { continue };
            p.defines.push(Define { kind: d.kind.clone(), state: d.state.clone(), cond });
        }
    }

    fn rules(&mut self, src: &Source, p: &mut Program) {
        for r in &src.rules {
            if p.rules.iter().any(|x| x.name == r.name) {
                self.errors.push(err(&r.at, format!("two rules are called {}", r.name)));
                continue;
            }
            let (ast::RuleBody::Every { subject, .. } | ast::RuleBody::No { subject, .. }) = &r.body;
            let global = subject.kind.strip_prefix("all ").map(str::to_string);
            if let Some(plural) = &global {
                if !p.kinds.iter().any(|k| k.plural == *plural) {
                    self.errors.push(err(&r.at, format!("`all {plural}`: no kind is called {plural} in the plural")));
                    continue;
                }
                if !r.globally {
                    self.errors.push(err(
                        &r.at,
                        format!("rule {}: `all {plural}` has no group key, so one total would hold up every entity it counts; end the rule with `, checked globally`", r.name),
                    ));
                    continue;
                }
                if p.kind(&subject.kind).is_none() {
                    p.kinds.push(Kind { name: subject.kind.clone(), plural: subject.kind.clone(), attrs: vec![], owner: None });
                    p.globals.push(subject.kind.clone());
                }
            } else if r.globally {
                self.errors.push(err(&r.at, format!("rule {}: only a rule about `all …` is checked globally", r.name)));
                continue;
            }
            if p.kind(&subject.kind).is_none() {
                self.errors.push(err(&r.at, format!("unknown kind {}", subject.kind)));
                continue;
            }
            let Some(filter) = self.filter(p, &r.at, subject) else { continue };
            self.period = r.period.clone();
            self.span = r.span.clone();
            let body = match &r.body {
                ast::RuleBody::Every { cond, .. } => match self.cond(p, &r.at, &subject.kind, cond) {
                    Some(c) => Body::Every(c),
                    None => continue,
                },
                ast::RuleBody::No { pred, unless, .. } => {
                    let Some(pred) = self.cond(p, &r.at, &subject.kind, pred) else { continue };
                    let unless = match unless {
                        Some(u) => match self.cond(p, &r.at, &subject.kind, u) {
                            Some(c) => Some(c),
                            None => continue,
                        },
                        None => None,
                    };
                    Body::No { pred, unless }
                }
            };
            self.period = None;
            self.span = None;
            if r.period.is_some() && !per_period(&body) {
                self.errors.push(err(
                    &r.at,
                    format!("rule {}: a total per period adds up a relation across units, as in `the total amount of their Refunds`", r.name),
                ));
                continue;
            }
            self.add_contribs(p);
            let tier = if global.is_some() {
                "global (bottleneck)"
            } else if body_sums(&body) {
                "per-group"
            } else {
                "local"
            };
            let mut rule = Rule {
                name: r.name.clone(),
                subject: subject.kind.clone(),
                filter,
                body,
                source: r.source.clone(),
                formula: String::new(),
                paraphrase: String::new(),
                tier: tier.into(),
                period: r.period.clone(),
                fixed: false,
                sources: vec![],
                fresh: r.fresh.clone(),
                applies: None,
            };
            rule.formula = readback::formula(p, &rule);
            rule.paraphrase = readback::paraphrase(p, &rule);
            p.rules.push(rule);
        }
    }

    fn goals(&mut self, src: &Source, p: &mut Program) {
        for g in &src.goals {
            let kind = &g.subject.kind;
            // A goal about a part is about each such part of a unit (appendix R.2.1).
            if p.kind(kind).is_none() {
                self.errors.push(err(&g.at, format!("unknown kind {kind}")));
                continue;
            }
            let Some(filter) = self.filter(p, &g.at, &g.subject) else { continue };
            let known: Vec<bool> = g.states.iter().map(|s| self.state(p, &g.at, kind, s).is_some()).collect();
            if known.contains(&false) {
                continue;
            }
            p.goals.push(Goal {
                kind: kind.clone(),
                filter,
                states: g.states.clone(),
                within_secs: g.within_secs,
                source: g.source.clone(),
            });
        }
    }

    fn reactions(&mut self, src: &Source, p: &mut Program) {
        for r in &src.reactions {
            if !r.by.is_empty() {
                if self.event(p, r) {
                    // The caller and the event's data can be read (appendix Q.2.3).
                    let admit = p.admits.iter().find(|a| matches!(&a.what, Admitted::Event { verb, kind } if third_person(verb) == r.outcome && *kind == r.kind));
                    self.data = admit.map(|a| a.data.clone()).unwrap_or_default();
                    let mut kinds: Vec<String> = r.by.iter().map(|b| p.role(b).and_then(|x| x.kind.clone()).unwrap_or_default()).collect();
                    kinds.sort();
                    kinds.dedup();
                    self.caller = Some(match kinds.as_slice() {
                        [k] => k.clone(),
                        ks if ks.iter().any(String::is_empty) => String::new(),
                        _ => "*".to_string(),
                    });
                    let actions = self.actions(p, r);
                    self.caller = None;
                    self.data = vec![];
                    p.reactions.push(Reaction { kind: r.kind.clone(), outcome: r.outcome.clone(), by: r.by.clone(), actions });
                }
                continue;
            }
            let Some(e) = p.effect_of(&r.kind) else {
                self.errors.push(err(&r.at, format!("a {} has no effect, so nothing reports `{}` for it", r.kind, r.outcome)));
                continue;
            };
            if e.success != r.outcome && !e.failures.contains(&r.outcome) && r.outcome != TIMED_OUT {
                self.errors.push(err(
                    &r.at,
                    format!("a {} reports `{}` or fails with {}, not `{}`", r.kind, e.success, e.failures.join(", "), r.outcome),
                ));
                continue;
            }
            let actions = self.actions(p, r);
            p.reactions.push(Reaction { kind: r.kind.clone(), outcome: r.outcome.clone(), by: vec![], actions });
        }
    }

    /// An admitted event's reaction names an admitted event, and only roles
    /// its admits name (appendix G.2.4).
    fn event(&mut self, p: &Program, r: &ast::ReactionDecl) -> bool {
        let admits: Vec<&Admit> =
            p.admits.iter().filter(|a| matches!(&a.what, Admitted::Event { verb, kind } if third_person(verb) == r.outcome && *kind == r.kind)).collect();
        if admits.is_empty() {
            let applied = r.outcome.strip_suffix("ies").map(|s| format!("{s}y"));
            let verb = applied
                .as_deref()
                .filter(|v| third_person(v) == r.outcome)
                .or(r.outcome.strip_suffix("es").filter(|v| third_person(v) == r.outcome))
                .or(r.outcome.strip_suffix('s'))
                .unwrap_or(&r.outcome);
            self.errors.push(err(&r.at, format!("no `admit` lets anyone {verb} {}; add `admit {verb} {} by {}`", a(&r.kind), r.kind, r.by.join(", "))));
            return false;
        }
        let mut ok = true;
        for role in &r.by {
            if !admits.iter().any(|a| a.roles.contains(role)) {
                let names: Vec<&str> = admits.iter().flat_map(|a| a.roles.iter().map(String::as_str)).collect();
                self.errors.push(err(&r.at, format!("`{}`: only {} may do this, not {role}", r.outcome, names.join(", "))));
                ok = false;
            }
        }
        ok
    }

    fn actions(&mut self, p: &Program, r: &ast::ReactionDecl) -> Vec<Action> {
            let mut actions = vec![];
            for a in &r.actions {
                if let Some((_, End::Node(k))) = self.np(p, &r.at, &r.kind, &a.target)
                    && p.source_of(&k, &a.attr).is_some() {
                    self.errors.push(err(&r.at, format!("{k}.{} comes from its source; a reaction cannot set it", a.attr)));
                    continue;
                }
                match self.np(p, &r.at, &r.kind, &a.target) {
                    Some((target, End::Node(k) | End::Ref(k)))
                        if through_ref(&target) || target.steps.first() == Some(&Step::Caller) || matches!(target.steps.last(), Some(Step::Attr(_))) =>
                    {
                        self.errors.push(err(&r.at, format!("`{}` is a shared {k}; a reaction changes only its own unit", show(&a.target))));
                    }
                    Some((target, End::Node(k))) if a.becomes.is_some() => {
                        let Some(want) = p.kind(&k).and_then(|x| x.attr(&a.attr)).map(|x| x.ty) else {
                            self.errors.push(err(&r.at, format!("{} has no attribute `{}` to change", self::a(&k), a.attr)));
                            continue;
                        };
                        let Some((num, ty)) = self.num(p, &r.at, &r.kind, a.becomes.as_ref().expect("checked above")) else { continue };
                        if ty != want {
                            self.errors.push(err(&r.at, format!("{k}.{} is {want:?}, but it would become a {ty:?}", a.attr)));
                            continue;
                        }
                        let states = p.kind(&k).and_then(|x| x.attr(&a.attr)).map(|x| x.states.clone()).unwrap_or_default();
                        if let Num::Lit(Lit::Text(v)) = &num
                            && !states.is_empty()
                            && !states.contains(v)
                        {
                            self.errors.push(err(&r.at, format!("{k}.{} is one of {}, so it cannot become {v}", a.attr, states.join(", "))));
                            continue;
                        }
                        if let Some(x) = sourced_in(p, &r.kind, &num) {
                            self.errors.push(err(&r.at, format!("`{} becomes …` reads {x}, which comes from its source and goes stale; read it in a rule instead", a.attr)));
                            continue;
                        }
                        actions.push(Action { target, attr: a.attr.clone(), value: true, becomes: Some(num) });
                    }
                    Some((target, End::Node(k))) => {
                        let is_bool = p.kind(&k).and_then(|x| x.attr(&a.attr)).is_some_and(|x| x.ty == Ty::Bool);
                        let state = p.kind(&k).and_then(|x| x.state_attr(&a.attr));
                        if is_bool {
                            actions.push(Action { target, attr: a.attr.clone(), value: a.value, becomes: None });
                        } else if let Some(x) = state {
                            // `the Item is received` puts its reception in that state (appendix Z.2.3).
                            if a.value {
                                actions.push(Action { target, attr: x.name.clone(), value: true, becomes: Some(Num::Lit(Lit::Text(a.attr.clone()))) });
                            } else {
                                self.errors.push(err(&r.at, format!("`is not {}` does not say which state a {k}'s {} goes to; name one: {}", a.attr, x.name, x.states.join(", "))));
                            }
                        } else {
                            self.errors.push(err(&r.at, format!("a {k} has no Bool attribute `{}` to set", a.attr)));
                        }
                    }
                    Some((_, End::Value(_) | End::Set(_) | End::Ref(_))) => {
                        self.errors.push(err(&r.at, format!("`{}` is a value; an action sets an attribute of a node", show(&a.target))));
                    }
                    None => {}
                }
            }
            actions
    }

    fn roles(&mut self, src: &Source, p: &mut Program) {
        for r in &src.roles {
            if p.role(&r.name).is_some() {
                self.errors.push(err(&r.at, format!("role {} is declared twice", r.name)));
                continue;
            }
            if let Some(k) = &r.kind {
                match p.kind(k) {
                    None => {
                        self.errors.push(err(&r.at, format!("role {}: there is no kind {k}", r.name)));
                        continue;
                    }
                    Some(x) if x.owner.is_some() => {
                        self.errors.push(err(&r.at, format!("role {}: {k} is a part, so no caller can be one", r.name)));
                        continue;
                    }
                    Some(_) => {}
                }
            }
            p.roles.push(Role { name: r.name.clone(), kind: r.kind.clone() });
        }
    }

    /// Each `admit`, one for each kind it names (appendix G.2.2).
    fn admits(&mut self, src: &Source, p: &mut Program) {
        for a in &src.admits {
            let mut kinds = vec![];
            for r in &a.roles {
                match p.role(r) {
                    Some(role) => kinds.push(role.kind.clone().unwrap_or_default()),
                    None => {
                        let known: Vec<&str> = p.roles.iter().map(|r| r.name.as_str()).collect();
                        let known = if known.is_empty() { "none is declared".to_string() } else { format!("the roles are {}", known.join(", ")) };
                        self.errors.push(err(&a.at, format!("`admit … by {r}`: there is no role {r}; {known}")));
                    }
                }
            }
            if kinds.len() < a.roles.len() {
                continue;
            }
            kinds.sort();
            kinds.dedup();
            // Empty when a role has no kind, `*` when the roles' kinds differ.
            let caller = match kinds.as_slice() {
                [k] => k.clone(),
                ks if ks.iter().any(String::is_empty) => String::new(),
                _ => "*".to_string(),
            };
            let targets: Vec<Admitted> = match &a.what {
                ast::Admitted::New { verb, kinds } => kinds.iter().map(|k| Admitted::New { verb: verb.clone(), kind: k.clone() }).collect(),
                ast::Admitted::Event { verb, kind } => vec![Admitted::Event { verb: verb.clone(), kind: kind.clone() }],
                ast::Admitted::Change { kind, attr } => vec![Admitted::Change { kind: kind.clone(), attr: attr.clone() }],
            };
            for what in targets {
                let k = what.kind().to_string();
                match p.kind(&k) {
                    None => {
                        self.errors.push(err(&a.at, format!("`{}`: there is no kind {k}", a.source)));
                        continue;
                    }
                    // An event can go to a part (appendix R.2.2).
                    Some(x) if x.owner.is_some() && !matches!(what, Admitted::Event { .. }) => {
                        let owner = x.owner.as_ref().map(|o| o.kind.clone()).unwrap_or_default();
                        self.errors.push(err(&a.at, format!("`{}`: {k} is a part of {owner}; its owner's needs create and change it", a.source)));
                        continue;
                    }
                    Some(_) => {}
                }
                if let Admitted::Change { kind, attr } = &what
                    && let Some(why) = unchangeable(p, kind, attr) {
                    self.errors.push(err(&a.at, format!("`admit {}`: {why}", a.source)));
                    continue;
                }
                let Some(data) = self.event_data(p, a, &k) else { continue };
                self.caller = Some(caller.clone());
                self.data = data.clone();
                let cond = a.cond.as_ref().map(|c| self.cond(p, &a.at, &k, c));
                self.caller = None;
                self.data = vec![];
                let cond = match cond {
                    Some(None) => continue,
                    Some(c) => c,
                    None => None,
                };
                if let Admitted::Event { verb, kind } = &what
                    && let Some(other) = p.admits.iter().find(|x| matches!(&x.what, Admitted::Event { verb: v, kind: k } if v == verb && k == kind))
                    && other.data != data
                {
                    self.errors.push(err(&a.at, format!("`admit {}`: `admit {}` gives this event other data; an event carries the same data whoever sends it", a.source, other.source)));
                    continue;
                }
                if let Admitted::Event { verb, kind } = &what {
                    let handled = |role: &String| src.reactions.iter().any(|r| r.by.contains(role) && r.kind == *kind && r.outcome == third_person(verb));
                    let missing: Vec<&String> = a.roles.iter().filter(|r| !handled(r)).collect();
                    if !missing.is_empty() {
                        let who = missing.iter().map(|r| r.as_str()).collect::<Vec<_>>().join(" or ");
                        self.errors.push(err(&a.at, format!("`admit {}`: no reaction handles it; add `when a {who} {} {}: …`", a.source, third_person(verb), self::a(kind))));
                    }
                }
                p.admits.push(Admit { what, roles: a.roles.clone(), data, cond, source: a.source.clone() });
            }
        }
    }

    /// An event's data (appendix Q.2.1): values, each named once, and not
    /// after an attribute of the kind, which `the …` would also name.
    fn event_data(&mut self, p: &Program, a: &ast::AdmitDecl, kind: &str) -> Option<Vec<(String, Ty)>> {
        let mut out: Vec<(String, Ty)> = vec![];
        for (name, t) in &a.data {
            let Some(ty) = value_type(t) else {
                self.errors.push(err(&a.at, format!("`admit {}`: `{name}` is a value, such as Text, Count or Money; an event carries no references or lists", a.source)));
                return None;
            };
            if out.iter().any(|(n, _)| n == name) {
                self.errors.push(err(&a.at, format!("`admit {}`: `{name}` is named twice", a.source)));
                return None;
            }
            if p.kind(kind).is_some_and(|k| k.attr(name).is_some()) || name == "caller" {
                self.errors.push(err(&a.at, format!("`admit {}`: `{name}` is already {}; name the event's value otherwise", a.source, if name == "caller" { "the caller".to_string() } else { format!("an attribute of {}", self::a(kind)) })));
                return None;
            }
            out.push((name.clone(), ty));
        }
        Some(out)
    }

    fn filter(&mut self, p: &Program, at: &Loc, s: &ast::Subject) -> Option<Vec<Cond>> {
        s.filter.iter().map(|c| self.cond(p, at, &s.kind, c)).collect()
    }

    /// A calculation and its type (appendix E.2.3).
    fn num(&mut self, p: &Program, at: &Loc, from: &str, n: &ast::Num) -> Option<(Num, Ty)> {
        match n {
            ast::Num::Lit(l) => {
                let (lit, ty) = self.lit(at, l, &p.currency)?;
                Some((Num::Lit(lit), ty))
            }
            ast::Num::Np(np) => match self.np(p, at, from, np)? {
                (path, End::Value(ty)) => Some((Num::Path(path), ty)),
                (path, End::Ref(_)) => Some((Num::Path(path), Ty::Ref)),
                (path, End::Set(_)) => Some((Num::Path(path), Ty::Set)),
                (_, End::Node(k)) => {
                    self.errors.push(err(at, format!("`{}` is {}, not a value", show(np), a(&k))));
                    None
                }
            },
            // `a * b / c` over amounts: a share of an amount (appendix AB.2.1).
            ast::Num::Bin(x, '/', y) if matches!(x.as_ref(), ast::Num::Bin(_, '*', _)) => {
                let ast::Num::Bin(a, _, b) = x.as_ref() else { unreachable!("matched above") };
                let (na, ta) = self.num(p, at, from, a)?;
                let (nb, tb) = self.num(p, at, from, b)?;
                let (r, rt) = self.num(p, at, from, y)?;
                match product_quotient(ta, tb, rt) {
                    Ok(ty) => Some((Num::Bin(Box::new(Num::Bin(Box::new(na), Arith::Mul, Box::new(nb))), Arith::Div, Box::new(r)), ty)),
                    Err(what) => {
                        self.errors.push(err(at, format!("`{}`: {what}", show_num(n))));
                        None
                    }
                }
            }
            ast::Num::Bin(x, op, y) => {
                let (l, lt) = self.num(p, at, from, x)?;
                let (r, rt) = self.num(p, at, from, y)?;
                match arith_type(*op, lt, rt) {
                    Ok((arith, ty)) => Some((Num::Bin(Box::new(l), arith, Box::new(r)), ty)),
                    Err(what) => {
                        self.errors.push(err(at, format!("`{}`: {what}", show_num(n))));
                        None
                    }
                }
            }
            ast::Num::Total { attr, of, filter } => {
                if let Some((kind, by)) = across(p, from, of) {
                    let ty = match p.kind(&kind).and_then(|k| k.attr(attr)).map(|a| a.ty) {
                        Some(t @ (Ty::Money | Ty::Count)) => t,
                        _ => {
                            self.errors.push(err(at, format!("`the total {attr} of {}`: {} has no amount or count `{attr}`", show(of), a(&kind))));
                            return None;
                        }
                    };
                    return self.contribution(p, at, from, &kind, by, Some((attr, ty)), filter.as_deref());
                }
                if let Some(r) = self.sibling(p, at, from, of, Some(attr), filter.as_deref()) {
                    return r;
                }
                let (path, kind) = self.rel(p, at, from, of)?;
                let ty = match p.kind(&kind).and_then(|k| k.attr(attr)).map(|a| a.ty) {
                    Some(t @ (Ty::Money | Ty::Count)) => t,
                    Some(t) => {
                        self.errors.push(err(at, format!("`the total {attr} of {}`: {attr} is {t:?}; only amounts and counts add up", show(of))));
                        return None;
                    }
                    None => {
                        self.errors.push(err(at, format!("{} has no attribute {attr}", a(&kind))));
                        return None;
                    }
                };
                let filter = match filter {
                    Some(f) => Some(Box::new(self.cond(p, at, &kind, f)?)),
                    None => None,
                };
                let calc = Box::new(Num::Path(Path { steps: vec![Step::Attr(attr.clone())] }));
                Some((Num::Sum { of: path, kind, calc, filter, ty }, ty))
            }
            ast::Num::Number { of, filter } => {
                if let Some((kind, by)) = across(p, from, of) {
                    return self.contribution(p, at, from, &kind, by, None, filter.as_deref());
                }
                if let Some(r) = self.sibling(p, at, from, of, None, filter.as_deref()) {
                    return r;
                }
                let (path, kind) = self.rel(p, at, from, of)?;
                let filter = match filter {
                    Some(f) => Some(Box::new(self.cond(p, at, &kind, f)?)),
                    None => None,
                };
                Some((Num::Count { of: path, kind, filter }, Ty::Count))
            }
            ast::Num::Over { calc, list } => {
                if list.steps.len() != 1 || !matches!(list.head, Head::It) {
                    self.errors.push(err(at, format!("`{}`: a total over a list reads the entity's own list, as in `over its lines`", show_num(n))));
                    return None;
                }
                let (path, element) = self.rel(p, at, from, list)?;
                let Some(l) = p.lists.iter().find(|l| l.owner == from && l.name == list.steps[0]) else {
                    self.errors.push(err(at, format!("`{}` is not a list of {}", show(list), a(from))));
                    return None;
                };
                let names = l.singulars();
                let (calc, ty) = self.over(p, at, from, &element, &names, calc)?;
                if !matches!(ty, Ty::Money | Ty::Count) {
                    self.errors.push(err(at, format!("`{}` is {ty:?}; only amounts and counts add up", show_num(n))));
                    return None;
                }
                Some((Num::Sum { of: path, kind: element, calc: Box::new(calc), filter: None, ty }, ty))
            }
            ast::Num::Run { of, filter } => {
                // Days in a row are of a total per day a span covers (appendix AH.2.1).
                let Some((kind, by)) = across(p, from, of).filter(|_| self.period.as_deref() == Some("day") && self.span.is_some()) else {
                    self.errors.push(err(at, format!("`{}` counts the days a span covers: write it in `for each {from} and day from their ⟨time⟩ to their ⟨time⟩, …`", show_num(n))));
                    return None;
                };
                let (num, ty) = self.contribution(p, at, from, &kind, by, None, filter.as_deref())?;
                let Num::Taken { of, name, period, kind, .. } = num else { unreachable!("a contribution is a running total") };
                Some((Num::Taken { of, name, ty, period, kind, run: true }, ty))
            }
            ast::Num::Taken { attr, from: of } => {
                let (path, end) = self.np(p, at, from, of)?;
                if !path.steps.is_empty() {
                    self.errors.push(err(at, format!("`{}`: only a unit knows what was taken from it; write `the total {attr} taken from each {from}`", show_num(n))));
                    return None;
                }
                let End::Node(k) = end else { unreachable!("an empty path ends at the subject") };
                let Some(t) = p.bindings.iter().filter(|b| b.target == k).find_map(|b| b.taking.as_ref().filter(|t| t.name == *attr)) else {
                    self.errors.push(err(at, format!("`{}`: no `uses a {k}` takes {attr}", show_num(n))));
                    return None;
                };
                Some((Num::Taken { of: path, name: attr.clone(), ty: t.ty, period: None, kind: String::new(), run: false }, t.ty))
            }
        }
    }

    /// A total over a relation across units (appendix F.2.11): each entity
    /// of `kind` adds its `attr` (or 1, to count) to a running total of
    /// `from`'s, as an implicit `uses a ⟨from⟩ where the ⟨from⟩ is its ⟨by⟩`.
    /// With `that`, it adds only while the filter holds of it (appendix AA.2.1).
    #[allow(clippy::too_many_arguments)]
    fn contribution(&mut self, p: &Program, at: &Loc, from: &str, kind: &str, by: Option<String>, attr: Option<(&String, Ty)>, filter: Option<&ast::Cond>) -> Option<(Num, Ty)> {
        let plural = p.kind(kind).map_or_else(|| plural_of(kind), |k| k.plural.clone());
        let held = match filter {
            Some(f) => {
                // The filter is about each entity of the relation, in its own
                // unit: there is no caller or event data there.
                let saved = (self.caller.take(), std::mem::take(&mut self.data));
                let c = self.cond(p, at, kind, f);
                (self.caller, self.data) = saved;
                Some(c?)
            }
            None => None,
        };
        let that = held.as_ref().map_or_else(String::new, |c| readback::that_words(p, kind, Some(c)));
        let (what, amount, ty) = match attr {
            Some((a, ty)) => (format!("{a} of {plural}{that}"), Num::Path(Path { steps: vec![Step::Attr(a.clone())] }), ty),
            None => (format!("number of {plural}{that}"), Num::Lit(Lit::Count(1)), Ty::Count),
        };
        let name = match (&self.period, &self.span) {
            (Some(per), Some((start, end))) => format!("{what} per {per} from its {start} to its {end}"),
            (Some(per), None) => format!("{what} per {per}"),
            (None, _) => what,
        };
        let span = match &self.span {
            Some((start, end)) => {
                for a in [start, end] {
                    if p.kind(kind).and_then(|k| k.attr(a)).is_none_or(|x| x.ty != Ty::Time) {
                        self.errors.push(err(at, format!("a {kind} spans the periods with two of its times, and `{a}` is not a Time attribute of it")));
                        return None;
                    }
                }
                let attr = |a: &String| Num::Path(Path { steps: vec![Step::Attr(a.clone())] });
                Some((attr(start), attr(end)))
            }
            None => None,
        };
        let key = match &by {
            Some(b) => Num::Path(Path { steps: vec![Step::Attr(b.clone())] }),
            None => Num::Lit(Lit::Text(from.to_string())),
        };
        let source = match &by {
            Some(b) => format!("each {kind} adds to its {b}'s total {name}"),
            None => format!("each {kind} adds to the total {name}"),
        };
        self.contribs.push(Binding {
            id: format!("{from}({name})"),
            subject: kind.to_string(),
            target: from.to_string(),
            per: None,
            key_attr: String::new(),
            key_member: false,
            key,
            filter: vec![],
            taking: Some(Taking { name: name.clone(), amount, ty, period: self.period.clone(), span }),
            prefer: vec![],
            source,
            held,
            shared: false,
            sources: vec![],
        });
        Some((Num::Taken { of: Path::default(), name, ty, period: self.period.clone(), kind: String::new(), run: false }, ty))
    }

    /// Adds the implicit bindings of the totals across units found so far,
    /// once each; a total any referring unit reads is shared.
    fn add_contribs(&mut self, p: &mut Program) {
        for b in self.contribs.drain(..) {
            match p.bindings.iter_mut().find(|x| x.subject == b.subject && x.id == b.id) {
                Some(x) => x.shared |= b.shared,
                None => p.bindings.push(b),
            }
        }
    }

    /// A total across units read from a unit that refers to their target:
    /// from an Item, `the number of its unit's Items that are received` is
    /// its unit's running total, carried and refreshed (appendix AA.2.3).
    /// `None` when the phrase is no such relation, and nothing was reported.
    fn sibling(&mut self, p: &Program, at: &Loc, from: &str, of: &NounPhrase, attr: Option<&String>, filter: Option<&ast::Cond>) -> Option<Option<(Num, Ty)>> {
        let (last, rest) = of.steps.split_last()?;
        if rest.is_empty() && matches!(of.head, Head::It) {
            return None;
        }
        let kind = p.kinds.iter().find(|k| k.plural == *last)?.name.clone();
        let owner = NounPhrase { head: of.head.clone(), steps: rest.to_vec() };
        let errors = self.errors.len();
        let (mut path, end) = match self.np(p, at, from, &owner) {
            Some((path, End::Ref(target))) => (path, target),
            _ => {
                self.errors.truncate(errors);
                return None;
            }
        };
        let target = end;
        // The path goes into the referred unit, whose carried fact holds the total.
        if let Some(Step::Attr(a)) = path.steps.last().cloned() {
            *path.steps.last_mut().expect("a last step") = Step::Ref(a);
        }
        let refs: Vec<&Attr> = p.kind(&kind)?.attrs.iter().filter(|a| a.ty == Ty::Ref && a.of.as_deref() == Some(target.as_str())).collect();
        let [by] = refs.as_slice() else {
            self.errors.push(err(at, format!("`{}`: {kind} has {} attributes of kind {target}, and a relation across units needs exactly one", show(of), refs.len())));
            return Some(None);
        };
        let by = by.name.clone();
        if self.period.is_some() {
            self.errors.push(err(at, format!("`{}`: a total per period is read only by its own unit", show(of))));
            return Some(None);
        }
        let attr = match attr {
            Some(x) => match p.kind(&kind).and_then(|k| k.attr(x)).map(|a| a.ty) {
                Some(t @ (Ty::Money | Ty::Count)) => Some((x, t)),
                _ => {
                    self.errors.push(err(at, format!("`the total {x} of {}`: {} has no amount or count `{x}`", show(of), a(&kind))));
                    return Some(None);
                }
            },
            None => None,
        };
        let Some((Num::Taken { name, ty, .. }, _)) = self.contribution(p, at, &target, &kind, Some(by), attr, filter) else { return Some(None) };
        if let Some(b) = self.contribs.last_mut() {
            b.shared = true;
        }
        Some(Some((Num::Taken { of: path, name, ty, period: None, kind: target, run: false }, ty)))
    }

    /// The calculation inside `the total of … over its lines`, evaluated at
    /// each element: `the line's price` reads the element, anything else
    /// reads the owner through the element's owner step.
    fn over(&mut self, p: &Program, at: &Loc, owner: &str, element: &str, names: &[String], n: &ast::Num) -> Option<(Num, Ty)> {
        match n {
            ast::Num::Np(np) if matches!(np.head, Head::It) && np.steps.first().is_some_and(|w| names.contains(w)) => {
                let inner = NounPhrase { head: Head::It, steps: np.steps[1..].to_vec() };
                self.num(p, at, element, &ast::Num::Np(inner))
            }
            ast::Num::Np(_) => {
                let (num, ty) = self.num(p, at, owner, n)?;
                let Num::Path(path) = num else { unreachable!("a noun phrase resolves to a path") };
                let mut steps = vec![Step::Owner];
                steps.extend(path.steps);
                Some((Num::Path(Path { steps }), ty))
            }
            ast::Num::Lit(_) => self.num(p, at, owner, n),
            ast::Num::Bin(x, '/', y) if matches!(x.as_ref(), ast::Num::Bin(_, '*', _)) => {
                let ast::Num::Bin(a, _, b) = x.as_ref() else { unreachable!("matched above") };
                let (na, ta) = self.over(p, at, owner, element, names, a)?;
                let (nb, tb) = self.over(p, at, owner, element, names, b)?;
                let (r, rt) = self.over(p, at, owner, element, names, y)?;
                match product_quotient(ta, tb, rt) {
                    Ok(ty) => Some((Num::Bin(Box::new(Num::Bin(Box::new(na), Arith::Mul, Box::new(nb))), Arith::Div, Box::new(r)), ty)),
                    Err(what) => {
                        self.errors.push(err(at, format!("`{}`: {what}", show_num(n))));
                        None
                    }
                }
            }
            ast::Num::Bin(x, op, y) => {
                let (l, lt) = self.over(p, at, owner, element, names, x)?;
                let (r, rt) = self.over(p, at, owner, element, names, y)?;
                match arith_type(*op, lt, rt) {
                    Ok((arith, ty)) => Some((Num::Bin(Box::new(l), arith, Box::new(r)), ty)),
                    Err(what) => {
                        self.errors.push(err(at, format!("`{}`: {what}", show_num(n))));
                        None
                    }
                }
            }
            _ => {
                self.errors.push(err(at, format!("`{}`: a total over a list cannot hold another total", show_num(n))));
                None
            }
        }
    }

    /// A relation, `its Refunds` or `each Order's Shipments`: the path to the
    /// node that owns them, and their kind (appendix E.2.1).
    fn rel(&mut self, p: &Program, at: &Loc, from: &str, np: &NounPhrase) -> Option<(Path, String)> {
        let Some((last, rest)) = np.steps.split_last() else {
            self.errors.push(err(at, format!("`{}` is not a relation; name one, such as `its Refunds`", show(np))));
            return None;
        };
        let owner = NounPhrase { head: np.head.clone(), steps: rest.to_vec() };
        let (path, end) = self.np(p, at, from, &owner)?;
        let End::Node(k) = end else {
            self.errors.push(err(at, format!("`{}` is a value; only an entity has a relation", show(&owner))));
            return None;
        };
        match plural_kind(p, &k, last) {
            Some(kind) => Some((path, kind)),
            None => {
                self.errors.push(err(at, format!("{} has no {last}", a(&k))));
                None
            }
        }
    }

    /// Resolves a noun phrase from a node of kind `from` (spec B.3.8): each
    /// word is an attribute, a part, or the owner, tried in that order. Past a
    /// reference, the phrase reads a shared entity's carried facts, so only
    /// attributes can follow (appendix F.2.3).
    fn np(&mut self, p: &Program, at: &Loc, from: &str, np: &NounPhrase) -> Option<(Path, End)> {
        let mut steps = vec![];
        let mut cur = from.to_string();
        let mut shared = false;
        let mut words: &[String] = &np.steps;
        if let (Head::It, Some(w)) = (&np.head, np.steps.first())
            && let Some((_, ty)) = self.data.iter().find(|(n, _)| n == w)
            && p.kind(from).is_none_or(|k| k.attr(w).is_none())
        {
            // `the reason`: a value the event carries (appendix Q.2.2).
            if np.steps.len() > 1 {
                self.errors.push(err(at, format!("`the {}`: `the {w}` is a value the event carries; nothing follows it", np.steps.join("'s "))));
                return None;
            }
            if *ty == Ty::Media {
                self.errors.push(err(at, format!("`the {w}` is media; {MEDIA_READ}")));
                return None;
            }
            return Some((Path { steps: vec![Step::Data(w.clone())] }, End::Value(*ty)));
        }
        if let (Some(ck), Head::It, Some(w)) = (self.caller.clone(), &np.head, np.steps.first()) {
            // `the caller`, `the caller's country` (appendix G.2.3).
            if w == "caller" && p.kind(from).is_none_or(|k| k.attr("caller").is_none()) {
                if ck.is_empty() {
                    self.errors.push(err(at, format!("`{}`: a caller here has a role with no kind, so it is only a name; write `role … is a ⟨Kind⟩`", show(np))));
                    return None;
                }
                if ck == "*" {
                    self.errors.push(err(at, format!("`{}`: the roles here are of different kinds; write one for each", show(np))));
                    return None;
                }
                steps.push(Step::Caller);
                cur = ck;
                shared = true;
                words = &np.steps[1..];
                if words.is_empty() {
                    return Some((Path { steps }, End::Ref(cur)));
                }
            }
        }
        if let Some(sc) = self.scope.clone() {
            let elem = sc.elem.as_ref().filter(|(names, _)| matches!(np.head, Head::It) && np.steps.first().is_some_and(|w| names.contains(w)));
            if let Some((_, kind)) = elem {
                steps.push(Step::Elem);
                cur = kind.clone();
                words = &np.steps[1..];
            } else if matches!(&np.head, Head::The(k) if *k == sc.cand) || (sc.it_is_cand && matches!(np.head, Head::It)) {
                steps.push(Step::Cand);
                cur = sc.cand.clone();
                shared = true;
            }
        }
        let scoped = !steps.is_empty();
        if let Head::All(k) = &np.head {
            self.errors.push(err(at, format!("`all {k}`: add it up with `the total … of all {k}`, or count it with `the number of all {k}`")));
            return None;
        }
        if let (false, Head::Each(k)) = (scoped, &np.head)
            && k != from {
            self.errors.push(err(at, format!("`each {k}`: this sentence is about {}", a(from))));
            return None;
        }
        if let (false, Head::The(k)) = (scoped, &np.head) {
            // The subject, else an owner of that kind, else the subject's or
            // an owner's unique attribute of that kind (appendix F.2.4). A
            // valid chain of owners is shorter than the number of kinds; a
            // longer walk is going round a needs loop, reported elsewhere.
            let mut chain = vec![from.to_string()];
            while let Some(o) = p.kind(chain.last().expect("never empty")).and_then(|x| x.owner.as_ref()) {
                if chain.len() > p.kinds.len() {
                    break;
                }
                chain.push(o.kind.clone());
            }
            let unique_ref = |kind: &str| {
                let refs: Vec<&Attr> =
                    p.kind(kind).map(|x| x.attrs.iter().filter(|a| a.ty == Ty::Ref && a.of.as_deref() == Some(k)).collect()).unwrap_or_default();
                if refs.len() == 1 { Some(refs[0].name.clone()) } else { None }
            };
            if let Some(i) = chain.iter().position(|c| c == k) {
                steps.extend(std::iter::repeat_n(Step::Owner, i));
            } else if let Some((i, attr)) = chain.iter().enumerate().find_map(|(i, c)| unique_ref(c).map(|r| (i, r))) {
                steps.extend(std::iter::repeat_n(Step::Owner, i));
                steps.push(Step::Ref(attr));
                shared = true;
            } else {
                self.errors.push(err(at, format!("`the {k}`: {} is not part of {} and has no attribute of kind {k}", a(from), a(k))));
                return None;
            }
            cur = k.clone();
        }
        for (i, w) in words.iter().enumerate() {
            let next = words.get(i + 1);
            if w == CREATED {
                if let Some(next) = next {
                    self.errors.push(err(at, format!("`{}`: {w} is a value, so it has no `{next}`", show(np))));
                    return None;
                }
                steps.push(Step::Attr(w.clone()));
                return Some((Path { steps }, End::Value(Ty::Time)));
            }
            if !shared && plural_kind(p, &cur, w).is_some() {
                self.errors.push(err(
                    at,
                    format!("`{}` is a relation; count it with `the number of …` or add it up with `the total … of …`", show(np)),
                ));
                return None;
            }
            let kind = p.kind(&cur)?;
            if let Some(a) = kind.attr(w) {
                match (a.ty, next) {
                    (Ty::Ref, None) => {
                        steps.push(Step::Attr(w.clone()));
                        return Some((Path { steps }, End::Ref(a.of.clone().unwrap_or_default())));
                    }
                    (Ty::Ref, Some(_)) => {
                        steps.push(Step::Ref(w.clone()));
                        cur = a.of.clone().unwrap_or_default();
                        shared = true;
                        continue;
                    }
                    (Ty::Set, None) => {
                        steps.push(Step::Attr(w.clone()));
                        return Some((Path { steps }, End::Set(a.of.clone().unwrap_or_default())));
                    }
                    (_, Some(next)) => {
                        self.errors.push(err(at, format!("`{}`: {w} is a value, so it has no `{next}`", show(np))));
                        return None;
                    }
                    (Ty::Media, None) if self.reading => {
                        // A model reads media (appendix AF.2.2).
                        steps.push(Step::Attr(w.clone()));
                        return Some((Path { steps }, End::Value(Ty::Media)));
                    }
                    (Ty::Media, None) => {
                        self.errors.push(err(at, format!("`{}`: {}'s {w} is media; {MEDIA_READ}", show(np), kind.name)));
                        return None;
                    }
                    (ty, None) => {
                        if let Some(m) = a.mark {
                            if m == Mark::Secret && !self.sending {
                                self.errors.push(err(at, format!("`{}`: {}'s {w} is secret; secrets can be sent to an adapter, never compared or computed with", show(np), kind.name)));
                                return None;
                            }
                            self.marked.push((kind.name.clone(), w.clone(), m));
                        }
                        steps.push(Step::Attr(w.clone()));
                        return Some((Path { steps }, End::Value(ty)));
                    }
                }
            }
            if !shared && p.bindings.iter().any(|b| b.subject == cur && b.target == *w && b.per.is_none()) {
                // `its Carrier`: the entity its binding was granted (appendix F.1).
                steps.push(Step::Bound(w.clone()));
                cur = w.clone();
                shared = true;
                continue;
            }
            if shared {
                self.errors.push(err(
                    at,
                    format!("`{}`: {} is shared, so only its attributes can be read, and it has no attribute `{w}`", show(np), a_(&cur)),
                ));
                return None;
            }
            if p.slot(w).is_some() && p.needs.iter().any(|n| n.owner == cur && n.part == *w) {
                if next.is_some() {
                    let first = p.slot(w).map(|s| s.alternatives[0].clone()).unwrap_or_default();
                    self.errors.push(err(at, format!("`{}`: {w} is a slot; name its alternative, such as its {first}", show(np))));
                    return None;
                }
                steps.push(Step::Slot(w.clone()));
                return Some((Path { steps }, End::Node(w.clone())));
            }
            if p.has_part(&cur, w) {
                steps.push(Step::Part(w.clone()));
                cur = w.clone();
                continue;
            }
            match kind.owner.as_ref().filter(|o| o.attr == *w || o.kind == *w) {
                Some(o) => {
                    steps.push(Step::Owner);
                    cur = o.kind.clone();
                }
                None => {
                    self.errors.push(err(at, format!("{} has no attribute, part or owner called `{w}`", a(&cur))));
                    return None;
                }
            }
        }
        Some((Path { steps }, End::Node(cur)))
    }

    fn cond(&mut self, p: &Program, at: &Loc, from: &str, c: &ast::Cond) -> Option<Cond> {
        match c {
            ast::Cond::And(cs) => Some(Cond::And(cs.iter().map(|c| self.cond(p, at, from, c)).collect::<Option<Vec<_>>>()?)),
            ast::Cond::Or(cs) => Some(Cond::Or(cs.iter().map(|c| self.cond(p, at, from, c)).collect::<Option<Vec<_>>>()?)),
            ast::Cond::Has { owner, negated, kind, state } => {
                let (node, end) = self.np(p, at, from, owner)?;
                let k = match end {
                    End::Node(k) if !through_ref(&node) => k,
                    End::Node(k) | End::Ref(k) => {
                        self.errors.push(err(at, format!("`{}` is a shared {k}; its parts are not read from another unit", show(owner))));
                        return None;
                    }
                    End::Value(_) | End::Set(_) => {
                        self.errors.push(err(at, format!("`{}` is a value; only a node has parts", show(owner))));
                        return None;
                    }
                };
                if let Some(slot) = p.slot(kind) {
                    // `has a Outcome that is sent`: any of its alternatives, so a rule
                    // about the slot holds each one, however many there are (appendix W.2.2).
                    if !p.has_part(&k, &slot.alternatives[0]) {
                        self.errors.push(err(at, format!("{} does not need {}", a(&k), a(kind))));
                        return None;
                    }
                    let mut each = vec![];
                    for alt in &slot.alternatives {
                        if let Some(s) = state {
                            self.state(p, at, alt, s)?;
                        }
                        each.push(Cond::Has { node: node.clone(), part: alt.clone(), state: state.clone(), negated: *negated });
                    }
                    return Some(if *negated { Cond::And(each) } else { Cond::Or(each) });
                }
                if !p.has_part(&k, kind) {
                    self.errors.push(err(at, format!("{} does not need {}", a(&k), a(kind))));
                    return None;
                }
                if let Some(s) = state {
                    self.state(p, at, kind, s)?;
                }
                Some(Cond::Has { node, part: kind.clone(), state: state.clone(), negated: *negated })
            }
            ast::Cond::Contains { set, item, negated } => {
                let (set_path, end) = self.np(p, at, from, set)?;
                let End::Set(k) = end else {
                    self.errors.push(err(at, format!("`{}` is not a set, so it contains nothing", show(set))));
                    return None;
                };
                let (item_path, end) = self.np(p, at, from, item)?;
                let item_path = match end {
                    End::Ref(k2) if k2 == k => item_path,
                    End::Node(k2) if k2 == k && as_reference(&item_path).is_some() => as_reference(&item_path)?,
                    _ => {
                        self.errors.push(err(at, format!("`{}` holds references to {}; `{}` is not one", show(set), p.kind(&k).map_or(k.clone(), |x| x.plural.clone()), show(item))));
                        return None;
                    }
                };
                Some(Cond::Contains { set: Num::Path(set_path), item: Num::Path(item_path), negated: *negated })
            }
            ast::Cond::Cmp { left, op, right } => {
                let (num, ty) = self.num(p, at, from, left)?;
                if !ordered(ty) {
                    self.errors.push(err(
                        at,
                        format!("`{}` is {ty:?}; only amounts, counts, times and durations compare with at most, at least, more than or less than", show_num(left)),
                    ));
                    return None;
                }
                let right = self.operand(p, at, from, right, ty)?;
                let op = match op {
                    CmpOp::AtMost => Op::Le,
                    CmpOp::AtLeast => Op::Ge,
                    CmpOp::MoreThan => Op::Gt,
                    CmpOp::LessThan => Op::Lt,
                };
                Some(Cond::Cmp { left: num, op, right })
            }
            ast::Cond::Is { left, negated, right } => {
                let ast::Num::Np(left) = left else {
                    let (num, ty) = self.num(p, at, from, left)?;
                    let right = self.operand(p, at, from, right, ty)?;
                    return Some(Cond::Cmp { left: num, op: if *negated { Op::Ne } else { Op::Eq }, right });
                };
                let (path, end) = self.np(p, at, from, left)?;
                match end {
                    End::Node(k) if matches!(path.steps.last(), Some(Step::Cand | Step::Bound(_))) => {
                        let bools = shared_states(p, &k);
                        match right {
                            Term::Word(s) if bools.contains(s) => Some(Cond::State { node: path, state: s.clone(), negated: *negated }),
                            _ => {
                                let states = if bools.is_empty() { String::new() } else { format!(", or a state: {}", bools.join(", ")) };
                                self.errors.push(err(at, format!("`{}` is a shared {k}; compare its attributes{states}", show(left))));
                                None
                            }
                        }
                    }
                    End::Node(k) if through_ref(&path) => {
                        let Some(reference) = as_reference(&path) else {
                            self.errors.push(err(at, format!("`{}` is a shared {k}; compare its attributes", show(left))));
                            return None;
                        };
                        self.shared_is(p, at, from, left, &k, reference, *negated, right)
                    }
                    End::Ref(k) => self.shared_is(p, at, from, left, &k, path, *negated, right),
                    End::Set(_) => {
                        self.errors.push(err(at, format!("`{}` is a set; ask whether it contains something", show(left))));
                        None
                    }
                    End::Node(k) => {
                        let Term::Word(s) = right else {
                            self.errors.push(err(at, format!("`{}` is a {k}; compare it with a state: {}", show(left), p.states(&k).join(", "))));
                            return None;
                        };
                        self.state(p, at, &k, s)?;
                        Some(Cond::State { node: path, state: s.clone(), negated: *negated })
                    }
                    End::Value(ty) => {
                        let right = self.operand(p, at, from, right, ty)?;
                        if let (Some((Step::Attr(x), rest)), Num::Lit(Lit::Text(v))) = (path.steps.split_last(), &right)
                            && let Some(a) = value_kind(p, from, rest).and_then(|k| p.kind(&k)).and_then(|k| k.attr(x)).filter(|a| !a.states.is_empty())
                            && !a.states.contains(v)
                        {
                            self.errors.push(err(at, format!("`{}` is never {v}; its states are {}", show(left), a.states.join(", "))));
                            return None;
                        }
                        Some(Cond::Cmp { left: Num::Path(path), op: if *negated { Op::Ne } else { Op::Eq }, right })
                    }
                }
            }
        }
    }

    /// `x is [not] …` where `x` refers to a shared K (`reference` is the path
    /// to the reference): a Bool attribute of K, its only kind of state
    /// (appendix F.2.3), or another reference to a K.
    #[allow(clippy::too_many_arguments)]
    fn shared_is(
        &mut self,
        p: &Program,
        at: &Loc,
        from: &str,
        left: &NounPhrase,
        k: &str,
        reference: Path,
        negated: bool,
        right: &Term,
    ) -> Option<Cond> {
        let bools = shared_states(p, k);
        match right {
            Term::Word(s) if bools.contains(s) => {
                let Some((Step::Attr(a), rest)) = reference.steps.split_last() else { unreachable!("a reference ends at its attribute") };
                let mut steps = rest.to_vec();
                steps.push(Step::Ref(a.clone()));
                Some(Cond::State { node: Path { steps }, state: s.clone(), negated })
            }
            Term::Np(np) => {
                let (other, end) = self.np(p, at, from, np)?;
                let other = match end {
                    End::Ref(k2) if k2 == k => other,
                    End::Node(k2) if k2 == k && as_reference(&other).is_some() => as_reference(&other)?,
                    _ => {
                        self.errors.push(err(at, format!("`{}` refers to {}; `{}` does not", show(left), a(k), show(np))));
                        return None;
                    }
                };
                Some(Cond::Cmp { left: Num::Path(reference), op: if negated { Op::Ne } else { Op::Eq }, right: Num::Path(other) })
            }
            _ => {
                let states = if bools.is_empty() { String::new() } else { format!(" or a state: {}", bools.join(", ")) };
                self.errors.push(err(at, format!("`{}` refers to {}; compare it with another reference to {}{states}", show(left), a(k), a(k))));
                None
            }
        }
    }

    /// The other side of a comparison with a value of type `ty`. A bare word
    /// is text when `ty` is Text (spec B.3.7).
    fn operand(&mut self, p: &Program, at: &Loc, from: &str, t: &Term, ty: Ty) -> Option<Num> {
        let (operand, found) = match t {
            Term::Word(w) if ty == Ty::Text => (Num::Lit(Lit::Text(w.clone())), Ty::Text),
            Term::Word(w) => {
                self.errors.push(err(at, format!("expected a {ty:?} value, found `{w}`")));
                return None;
            }
            Term::Lit(l) => self.num(p, at, from, &ast::Num::Lit(l.clone()))?,
            Term::Np(np) => self.num(p, at, from, &ast::Num::Np(np.clone()))?,
            Term::Calc(n) => self.num(p, at, from, n)?,
        };
        if found != ty {
            self.errors.push(err(at, format!("cannot compare a {ty:?} with a {found:?}")));
            return None;
        }
        Some(operand)
    }

    fn state(&mut self, p: &Program, at: &Loc, kind: &str, s: &str) -> Option<()> {
        let states = p.states(kind);
        if states.iter().any(|x| x == s) {
            return Some(());
        }
        self.errors.push(err(at, format!("{} has no state `{s}`; its states are {}", a(kind), states.join(", "))));
        None
    }
}

/// The states a shared entity is read in: its Bool attributes and the
/// states of its state attributes (appendix F.2.3, Z.2.2).
fn shared_states(p: &Program, k: &str) -> Vec<String> {
    let Some(x) = p.kind(k) else { return vec![] };
    x.attrs.iter().filter(|a| a.ty == Ty::Bool).map(|a| a.name.clone()).chain(x.attrs.iter().flat_map(|a| a.states.iter().cloned())).collect()
}

fn plural_name(p: &Program, kind: &str) -> String {
    p.kind(kind).map_or_else(|| plural_of(kind), |k| k.plural.clone())
}
