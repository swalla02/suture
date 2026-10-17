//! The interpreter (spec appendix B.3): one step of a unit for one message.
//! It is a pure function of the program, the unit and the message, so
//! replaying the journal reproduces every step.

use std::collections::BTreeMap;

use suture_lang::ir::{Arith, Body, Cond, Criterion, Effect, Num, Op, Path, Program, Rule, Step, Ty, CREATED, TIMED_OUT};

use crate::choice::{choose, pick};
use crate::state::{Ask, Bind, Decide, Fact, Facts, Msg, Node, Status, Unit, Val};

static NO_TOTALS: BTreeMap<String, Val> = BTreeMap::new();

/// What a sentence reads: a unit's tree, the facts carried into it
/// (appendix F.2.3), and what was taken from it; in a `uses` condition, also
/// the candidate and the element of `per`.
#[derive(Clone, Copy)]
pub struct View<'a> {
    pub root: &'a Node,
    pub facts: &'a Facts,
    pub taken: &'a BTreeMap<String, Val>,
    pub cand: Option<&'a str>,
    pub elem: Option<&'a [usize]>,
}

impl<'a> View<'a> {
    pub fn of(u: &'a Unit) -> View<'a> {
        View { root: &u.root, facts: &u.facts, taken: &u.taken, cand: None, elem: None }
    }

    /// A tree being built, with its facts and nothing taken yet.
    fn bare(root: &'a Node, facts: &'a Facts) -> View<'a> {
        View { root, facts, taken: &NO_TOTALS, cand: None, elem: None }
    }

    fn with(self, root: &'a Node) -> View<'a> {
        View { root, ..self }
    }
}

/// What a step sends out: requests to the outside world, messages to other
/// units, and choices for the host to make.
#[derive(Default)]
struct Out {
    asks: Vec<Ask>,
    sends: Vec<(String, Msg)>,
    decides: Vec<Decide>,
}

/// Where a walk ends: a node of this unit, or a shared entity's carried facts.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Place {
    Here(Vec<usize>),
    There(String),
}

/// A bound on settling. A unit's parts are bounded, so a valid program
/// settles long before this.
const SETTLE_ROUNDS: usize = 256;

/// The unit after a step, and what the step sends out.
pub struct Outcome {
    pub unit: Unit,
    pub asks: Vec<Ask>,
    pub sends: Vec<(String, Msg)>,
    pub decides: Vec<Decide>,
}

impl Outcome {
    fn new(unit: Unit, out: Out) -> Outcome {
        Outcome { unit, asks: out.asks, sends: out.sends, decides: out.decides }
    }

    /// The unit as it was, with only a note.
    fn kept(unit: &Unit, note: String, out: Out) -> Outcome {
        let mut kept = unit.clone();
        kept.notes = vec![note];
        Outcome::new(kept, out)
    }
}

/// A new unit for a root entity: its defaults, then the given attributes.
pub fn new_unit(prog: &Program, label: &str, kind: &str, given: &[(String, Val)]) -> Result<Unit, String> {
    new_unit_with(prog, label, kind, given, &[])
}

/// A new unit with the elements of its lists, at time 0 and reading no
/// shared entity.
pub fn new_unit_with(
    prog: &Program,
    label: &str,
    kind: &str,
    given: &[(String, Val)],
    items: &[(String, Vec<(String, Val)>)],
) -> Result<Unit, String> {
    new_unit_full(prog, label, kind, given, items, Facts::new(), 0)
}

/// A new unit with the elements of its lists (appendix E.2.6), each given as
/// the list's name and the element's attributes, the facts of the shared
/// entities it refers to, and the clock. Formulas are computed after
/// defaults and given values, in whatever order they read each other.
pub fn new_unit_full(
    prog: &Program,
    label: &str,
    kind: &str,
    given: &[(String, Val)],
    items: &[(String, Vec<(String, Val)>)],
    facts: Facts,
    now: i64,
) -> Result<Unit, String> {
    let k = prog.kind(kind).ok_or_else(|| format!("there is no kind {kind}"))?;
    if let Some(o) = &k.owner {
        return Err(format!("{kind} is a part of {}; its owner's needs create it", o.kind));
    }
    let mut root = blank(prog, kind, 1, now);
    give(prog, kind, &mut root, given)?;
    for (list, attrs) in items {
        let l = prog.lists.iter().find(|l| l.owner == kind && l.name == *list).ok_or_else(|| format!("{kind} has no list {list}"))?;
        let mut element = blank(prog, &l.element, 1, now);
        give(prog, &l.element, &mut element, attrs)?;
        element.status = Status::Done;
        root.parts.push(element);
    }
    // Formulas may read each other and the owner in any order: compute
    // until nothing changes. Each attribute is set at most once.
    while (0..root.parts.len()).fold(formulas(prog, &mut root, &facts, &[]), |changed, i| formulas(prog, &mut root, &facts, &[i]) || changed) {}
    let unit = Unit {
        label: label.to_string(),
        root,
        notes: vec![],
        met: vec![],
        matched: vec![],
        alerts: vec![],
        timers: vec![],
        facts,
        taken: BTreeMap::new(),
    };
    if let Some(r) = violated(prog, &step_at(prog, &unit, &Msg::Create, now).unit) {
        return Err(format!("refused by rule {}: {}", r.name, r.paraphrase));
    }
    Ok(unit)
}

/// Sets given attributes, which must exist, have the right type and not be formulas.
fn give(prog: &Program, kind: &str, n: &mut Node, given: &[(String, Val)]) -> Result<(), String> {
    let k = prog.kind(kind).ok_or_else(|| format!("there is no kind {kind}"))?;
    for (name, v) in given {
        let a = k.attr(name).ok_or_else(|| format!("{kind} has no attribute {name}"))?;
        if a.formula.is_some() {
            return Err(format!("{kind}.{name} is computed by its formula; it is not given"));
        }
        if v.ty() != a.ty {
            let want = a.of.as_ref().map_or_else(|| format!("{:?}", a.ty), |k| format!("{:?} of {k}", a.ty));
            return Err(format!("{kind}.{name} is {want}, not {}", v.show(&prog.currency)));
        }
        n.attrs.insert(name.clone(), v.clone());
    }
    Ok(())
}

/// Computes the formula attributes of the node at `at` that are not set
/// yet and can be known now; returns whether it set any. A formula whose
/// value is unknown, or a negative count, leaves its attribute unknown.
fn formulas(prog: &Program, root: &mut Node, facts: &Facts, at: &[usize]) -> bool {
    let kind = node_at(root, at).kind.clone();
    let Some(k) = prog.kind(&kind) else { return false };
    let mut changed = false;
    for a in &k.attrs {
        let Some(f) = &a.formula else { continue };
        if node_at(root, at).attrs.contains_key(&a.name) {
            continue;
        }
        match num(prog, View::bare(root, facts), at, f) {
            Some(Val::Count(c)) if c < 0 => {}
            Some(v) => {
                node_at_mut(root, at).attrs.insert(a.name.clone(), v);
                changed = true;
            }
            None => {}
        }
    }
    changed
}

fn blank(prog: &Program, kind: &str, attempt: u32, now: i64) -> Node {
    let attrs = prog
        .kind(kind)
        .map(|k| k.attrs.iter().filter_map(|a| a.default.as_ref().map(|d| (a.name.clone(), Val::from_lit(d)))).collect())
        .unwrap_or_default();
    Node {
        kind: kind.to_string(),
        attrs,
        created: now,
        status: Status::Idle,
        outcome: None,
        attempt,
        sends: 0,
        waiting: None,
        held: Default::default(),
        slots: Default::default(),
        binds: Default::default(),
        parts: vec![],
    }
}

/// One step at time 0.
pub fn step(prog: &Program, unit: &Unit, msg: &Msg) -> Outcome {
    step_at(prog, unit, msg, 0)
}

/// One step: apply the message, then settle the unit (create, replace and
/// withdraw parts, finish parts, send the requests every rule allows), then
/// note the goals that became met. `now` is the clock when the message was
/// delivered; parts created by the step are created then.
pub fn step_at(prog: &Program, unit: &Unit, msg: &Msg, now: i64) -> Outcome {
    let mut u = unit.clone();
    u.notes.clear();
    let mut out = Out::default();
    match msg {
        Msg::Create => {
            let attrs: Vec<String> = u.root.attrs.iter().map(|(k, v)| format!("{k} {}", v.show(&prog.currency))).collect();
            u.notes.push(format!("placed: {}", attrs.join(", ")));
        }
        Msg::GoalDue { goal } => {
            goal_due(prog, &mut u, *goal);
            return Outcome::new(u, out);
        }
        Msg::Patience { id } => {
            if !patience(prog, &mut u, id, &mut out) {
                return Outcome::new(u, out);
            }
        }
        Msg::Answer { key, outcome, .. } if outcome == TIMED_OUT => {
            if let Err(refusal) = deadline(prog, &mut u, key, &mut out) {
                return Outcome::kept(unit, refusal.note, Out::default());
            }
        }
        Msg::Answer { key, outcome, data } => {
            if let Err(refusal) = answer(prog, &mut u, key, outcome, data) {
                out.asks.extend(refusal.again.map(|a| *a));
                return Outcome::kept(unit, refusal.note, out);
            }
        }
        Msg::Take { from, key, taken } => {
            let mut totals = u.taken.clone();
            if let Some((name, amount)) = taken {
                let before = totals.get(name).cloned().unwrap_or_else(|| zero(amount.ty()));
                let Some(after) = arith(before, Arith::Add, amount.clone()) else {
                    let refused = Msg::Refused { key: key.clone(), by: u.label.clone(), rule: String::new() };
                    return Outcome::kept(unit, format!("refused {key}: the total {name} would overflow"), Out { sends: vec![(from.clone(), refused)], ..Out::default() });
                };
                totals.insert(name.clone(), after);
            }
            if let Some(r) = broken_rule(prog, View { taken: &totals, ..View::of(&u) }) {
                let refused = Msg::Refused { key: key.clone(), by: u.label.clone(), rule: r.name.clone() };
                return Outcome::kept(unit, format!("refused {key}: rule {}: {}", r.name, r.paraphrase), Out { sends: vec![(from.clone(), refused)], ..Out::default() });
            }
            u.taken = totals;
            u.notes.push(match taken {
                Some((name, amount)) => format!("granted {key}: {} {name} taken", amount.show(&prog.currency)),
                None => format!("granted {key}"),
            });
            out.sends.push((from.clone(), Msg::Granted { key: key.clone(), by: u.label.clone(), taken: taken.clone(), facts: own_facts(&u) }));
            return Outcome::new(u, out);
        }
        Msg::Release { from, key, taken } => {
            if let Some((name, amount)) = taken {
                let before = u.taken.get(name).cloned().unwrap_or_else(|| zero(amount.ty()));
                if let Some(after) = arith(before, Arith::Sub, amount.clone()) {
                    u.taken.insert(name.clone(), after);
                }
                u.notes.push(format!("{from} gave back {} {name} ({key})", amount.show(&prog.currency)));
            } else {
                u.notes.push(format!("{from} gave back {key}"));
            }
            return Outcome::new(u, out);
        }
        Msg::Granted { key, by, taken, facts } => granted(prog, &mut u, key, by, taken, facts, &mut out),
        Msg::Refused { key, by, rule } => {
            let Some((at, id)) = find_bind(&u, key) else { return Outcome::kept(unit, format!("ignored a refusal for {key}: it is gone"), out) };
            let n = node_at_mut(&mut u.root, &at);
            let (Some(Bind::Asked { skip } | Bind::Waiting { skip }), Status::Idle) = (n.binds.get(&id).cloned(), n.status) else {
                return Outcome::kept(unit, format!("ignored a refusal for {key}: not asked"), out);
            };
            let mut skip = skip;
            skip.push(by.clone());
            n.binds.insert(id.clone(), Bind::Asked { skip: skip.clone() });
            let why = if rule.is_empty() { String::new() } else { format!(": rule {rule}") };
            u.notes.push(format!("{} {by} refused {key}{why}", target_of(&id)));
            out.decides.extend(decide_for(prog, &u, &at, &id, skip));
            return Outcome::new(u, out);
        }
        Msg::Waiting { key } => {
            let Some((at, id)) = find_bind(&u, key) else { return Outcome::kept(unit, format!("ignored a wait for {key}: it is gone"), out) };
            let n = node_at_mut(&mut u.root, &at);
            if let Some(Bind::Asked { skip }) = n.binds.get(&id).cloned() {
                n.binds.insert(id.clone(), Bind::Waiting { skip });
                let kind = n.kind.clone();
                u.notes.push(format!("{kind} waits for a {}", target_of(&id)));
            }
            return Outcome::new(u, out);
        }
    }
    settle(prog, &mut u, &mut out, now);
    goals(prog, &mut u);
    if let (Some(r), Msg::Answer { key, outcome, .. }) = (violated(prog, &u), msg) {
        // A consequence no pass guards, such as a withdrawn part, would
        // break a rule: refuse the answer and ask again (appendix B.3.4).
        // A deadline is not an answer, so nothing is asked again for it.
        let again = open_request(&unit.root, &unit.label, key)
            .filter(|_| outcome != TIMED_OUT)
            .and_then(|at| prog.effect_of(&node_at(&unit.root, &at).kind).map(|e| ask_for(&unit.label, View::of(unit), &at, e)));
        let out = Out { asks: again.into_iter().collect(), ..Out::default() };
        return Outcome::kept(unit, format!("refused by rule {}: {}", r.name, r.paraphrase), out);
    }
    if let (Some(r), Msg::Patience { .. }) = (violated(prog, &u), msg) {
        // Moving on would break a rule: the slot stays where it is.
        return Outcome::kept(unit, format!("patience held: rule {}: {}", r.name, r.paraphrase), Out::default());
    }
    Outcome::new(u, out)
}

/// A binding was granted: it is recorded, with the facts of the entity bound.
/// One that is no longer wanted gives back at once what it took.
fn granted(prog: &Program, u: &mut Unit, key: &str, by: &str, taken: &Option<(String, Val)>, facts: &Facts, out: &mut Out) {
    let give_back = |u: &mut Unit, out: &mut Out, why: &str| {
        u.notes.push(format!("gave back {} {by}: {why}", target_of(key.rsplit_once('@').map_or(key, |(_, id)| id))));
        out.sends.push((by.to_string(), Msg::Release { from: u.label.clone(), key: key.to_string(), taken: taken.clone() }));
    };
    let Some((at, id)) = find_bind(u, key) else { return give_back(u, out, "its part is gone") };
    let n = node_at_mut(&mut u.root, &at);
    if n.status != Status::Idle || !matches!(n.binds.get(&id), Some(Bind::Asked { .. } | Bind::Waiting { .. })) {
        return give_back(u, out, "not asked");
    }
    n.binds.insert(id.clone(), Bind::Granted { to: by.to_string(), taken: taken.clone() });
    let kind = n.kind.clone();
    for (label, f) in facts {
        u.facts.entry(label.clone()).or_insert_with(|| f.clone());
    }
    let what = match &id.split_once('.') {
        Some((_, j)) => format!(" for {} {}", element_name(prog, &kind, &target_of(&id)), j.parse::<usize>().map_or(0, |j| j + 1)),
        None => String::new(),
    };
    u.notes.push(format!("{kind} gets {} {by}{what}", target_of(&id)));
}

/// `line`, for the element of the `per` binding of `kind` to `target`.
fn element_name(prog: &Program, kind: &str, target: &str) -> String {
    let b = prog.bindings.iter().find(|b| b.subject == kind && b.target == target);
    b.and_then(|b| b.per.as_ref()).and_then(|l| prog.lists.iter().find(|x| x.name == *l)).map_or_else(|| "element".into(), |l| l.singular())
}

/// The kind a binding id binds: `Stock` for `Stock.0`.
fn target_of(id: &str) -> String {
    id.split_once('.').map_or(id, |(t, _)| t).to_string()
}

/// A unit as other units carry it: its own fact, and the facts it carries.
fn own_facts(u: &Unit) -> Facts {
    let mut facts = u.facts.clone();
    facts.insert(u.label.clone(), Fact { kind: u.root.kind.clone(), created: u.root.created, attrs: u.root.attrs.clone() });
    facts
}

/// The part and binding id a binding key names, if the part still has it.
fn find_bind(u: &Unit, key: &str) -> Option<(Vec<usize>, String)> {
    let (node, id) = key.rsplit_once('@')?;
    let at = positions(&u.root).into_iter().find(|at| !at.is_empty() && request_key(&u.label, &u.root, at) == node)?;
    node_at(&u.root, &at).binds.contains_key(id).then(|| (at, id.to_string()))
}

/// Whether the binding `key` is waiting for a candidate.
pub fn waiting(u: &Unit, key: &str) -> bool {
    find_bind(u, key).is_some_and(|(at, id)| matches!(node_at(&u.root, &at).binds.get(&id), Some(Bind::Waiting { .. })))
}

/// A binding's id, its index in the program, and for `per`, the element's
/// number and position.
type BindSlot = (String, usize, Option<(u32, Vec<usize>)>);

/// The bindings the node at `at` has (appendix F.2.5): each id, the
/// binding's index and, for `per`, the element's number and position.
fn bind_slots(prog: &Program, root: &Node, at: &[usize]) -> Vec<BindSlot> {
    let n = node_at(root, at);
    let mut out = vec![];
    for (i, b) in prog.bindings.iter().enumerate().filter(|(_, b)| b.subject == n.kind) {
        let Some(list) = &b.per else {
            out.push((b.target.clone(), i, None));
            continue;
        };
        // The list belongs to the part or to its nearest owner that has it.
        let found = (0..=at.len()).rev().find_map(|d| {
            let owner = node_at(root, &at[..d]);
            prog.lists.iter().find(|l| l.name == *list && l.owner == owner.kind).map(|l| (d, l))
        });
        let Some((d, l)) = found else { continue };
        let owner = &at[..d];
        let elements = node_at(root, owner).parts.iter().enumerate().filter(|(_, p)| p.kind == l.element).map(|(j, _)| j);
        for (j, idx) in elements.enumerate() {
            out.push((format!("{}.{j}", b.target), i, Some((j as u32, child(owner, idx)))));
        }
    }
    out
}

/// Whether every binding of the node at `at` is granted.
fn bound(prog: &Program, root: &Node, at: &[usize]) -> bool {
    let n = node_at(root, at);
    bind_slots(prog, root, at).iter().all(|(id, ..)| matches!(n.binds.get(id), Some(Bind::Granted { .. })))
}

/// The choice request for the binding `id` of the part at `at`, if its
/// amount is known.
fn decide_for(prog: &Program, u: &Unit, at: &[usize], id: &str, skip: Vec<String>) -> Option<Decide> {
    let (_, i, elem) = bind_slots(prog, &u.root, at).into_iter().find(|(x, ..)| x == id)?;
    let b = &prog.bindings[i];
    let taken = match &b.taking {
        None => None,
        Some(t) => {
            let v = View { elem: elem.as_ref().map(|(_, p)| p.as_slice()), ..View::of(u) };
            Some((t.name.clone(), num(prog, v, at, &t.amount)?))
        }
    };
    Some(Decide {
        label: u.label.clone(),
        node: request_key(&u.label, &u.root, at),
        bind: id.to_string(),
        binding: i as u32,
        elem: elem.map(|(j, _)| j),
        taken,
        skip,
    })
}

/// Asks for each binding of a new part (appendix F.2.5). Makes at most one change.
fn bind_pass(prog: &Program, u: &mut Unit, at: &[usize], out: &mut Out) -> bool {
    if at.is_empty() || node_at(&u.root, at).status != Status::Idle {
        return false;
    }
    for (id, ..) in bind_slots(prog, &u.root, at) {
        if node_at(&u.root, at).binds.contains_key(&id) {
            continue;
        }
        let Some(d) = decide_for(prog, u, at, &id, vec![]) else { continue };
        node_at_mut(&mut u.root, at).binds.insert(id, Bind::Asked { skip: vec![] });
        out.decides.push(d);
        return true;
    }
    false
}

/// A part leaves before reaching the outside world: every binding it and its
/// parts hold gives back what it took (appendix F.2.10).
fn give_back(label: &str, root: &Node, at: &[usize], out: &mut Out) {
    let n = node_at(root, at);
    for (id, b) in &n.binds {
        if let Bind::Granted { to, taken } = b {
            let key = format!("{}@{id}", request_key(label, root, at));
            out.sends.push((to.clone(), Msg::Release { from: label.to_string(), key, taken: taken.clone() }));
        }
    }
    for i in 0..n.parts.len() {
        give_back(label, root, &child(at, i), out);
    }
}

/// What a choice request asks for, read at its part: the pool key.
pub fn decide_key(prog: &Program, u: &Unit, d: &Decide) -> Option<Val> {
    let (at, elem) = decide_place(prog, u, d)?;
    let v = View { elem: elem.as_deref(), ..View::of(u) };
    num(prog, v, &at, &prog.bindings.get(d.binding as usize)?.key)
}

fn decide_place(prog: &Program, u: &Unit, d: &Decide) -> Option<(Vec<usize>, Option<Vec<usize>>)> {
    let at = positions(&u.root).into_iter().find(|at| !at.is_empty() && request_key(&u.label, &u.root, at) == d.node)?;
    let (_, _, elem) = bind_slots(prog, &u.root, &at).into_iter().find(|(id, ..)| *id == d.bind)?;
    Some((at, elem.map(|(_, p)| p)))
}

/// The choice point for a binding (appendix F.2.7): of the candidates, in
/// creation order, the first `candidates` that meet the binding's filter
/// are ranked by its `prefer`. Returns the pick and how many were gathered.
pub fn choose_among(prog: &Program, u: &Unit, d: &Decide, candidates: impl Iterator<Item = Unit>) -> Option<(String, usize)> {
    let b = prog.bindings.get(d.binding as usize)?;
    let (at, elem) = decide_place(prog, u, d)?;
    let mut gathered = vec![];
    for c in candidates {
        if gathered.len() >= prog.candidates as usize {
            break;
        }
        let mut facts = u.facts.clone();
        facts.extend(own_facts(&c));
        let v = View { facts: &facts, cand: Some(&c.label), elem: elem.as_deref(), ..View::of(u) };
        if b.filter.iter().all(|f| eval(prog, v, &at, f) == T::Yes) {
            let scores = b.prefer.iter().map(|cr| score(prog, v, &at, cr)).collect::<Vec<_>>();
            gathered.push((c.label.clone(), scores));
        }
    }
    let scores: Vec<Vec<(u8, i128)>> = gathered.iter().map(|(_, s)| s.clone()).collect();
    pick(&scores).map(|i| (gathered[i].0.clone(), gathered.len()))
}

/// A candidate's rank on one criterion; lower comes first, and an unknown value last.
fn score(prog: &Program, v: View, at: &[usize], c: &Criterion) -> (u8, i128) {
    let amount = |n: &Num| match num(prog, v, at, n) {
        Some(Val::Money(x) | Val::Count(x) | Val::Time(x) | Val::Duration(x)) => Some(i128::from(x)),
        _ => None,
    };
    match c {
        Criterion::Lowest(n) => amount(n).map_or((1, 0), |x| (0, x)),
        Criterion::Highest(n) => amount(n).map_or((1, 0), |x| (0, -x)),
        Criterion::Those(cond) => (u8::from(eval(prog, v, at, cond) != T::Yes), 0),
    }
}

/// The position of the requested part whose key is `key`, if it is still open.
fn open_request(root: &Node, label: &str, key: &str) -> Option<Vec<usize>> {
    positions(root).into_iter().find(|at| node_at(root, at).status == Status::Requested && request_key(label, root, at) == key)
}

/// Why an answer was not recorded, and the request to send again, if any.
struct Refusal {
    note: String,
    again: Option<Box<Ask>>,
}

fn answer(prog: &Program, u: &mut Unit, key: &str, outcome: &str, data: &[(String, Val)]) -> Result<(), Refusal> {
    let Some(at) = open_request(&u.root, &u.label, key) else {
        return Err(Refusal { note: format!("ignored an answer for {key}: no open request"), again: None });
    };
    let kind = node_at(&u.root, &at).kind.clone();
    let effect = prog.effect_of(&kind).expect("a requested part has an effect");
    let again = Some(Box::new(ask_for(&u.label, View::of(u), &at, effect)));
    let refuse = |why: String| Refusal { note: format!("refused an answer for {key}: {why}"), again: again.clone() };
    let success = outcome == effect.success;
    if !success && !effect.failures.iter().any(|f| f == outcome) {
        return Err(refuse(format!("a {kind} cannot report `{outcome}`")));
    }
    for (name, v) in data {
        match effect.data.iter().find(|(n, _)| n == name) {
            Some((_, ty)) if *ty == v.ty() => {}
            Some((_, ty)) => return Err(refuse(format!("{name} must be {ty:?}, not {}", v.show(&prog.currency)))),
            None => return Err(refuse(format!("a {kind} answer has no field {name}"))),
        }
    }
    if success {
        if let Some((name, _)) = effect.data.iter().find(|(n, _)| !data.iter().any(|(d, _)| d == n)) {
            return Err(refuse(format!("the answer is missing {name}")));
        }
    }
    let mut t = u.root.clone();
    {
        let n = node_at_mut(&mut t, &at);
        for (name, v) in data {
            n.attrs.insert(name.clone(), v.clone());
        }
        n.outcome = Some(outcome.to_string());
        n.status = if success { Status::Done } else { Status::Failed };
        n.waiting = None;
    }
    let shown: Vec<String> = data.iter().map(|(k, v)| format!("{k} {}", v.show(&prog.currency))).collect();
    let mut notes =
        vec![if shown.is_empty() { format!("{kind} {outcome}") } else { format!("{kind} {outcome}: {}", shown.join(", ")) }];
    react(prog, &mut t, &u.facts, &at, &kind, outcome, &mut notes);
    if let Some(r) = broken_rule(prog, View::of(u).with(&t)) {
        return Err(Refusal { note: format!("refused by rule {}: {}", r.name, r.paraphrase), again });
    }
    u.root = t;
    u.notes.extend(notes);
    Ok(())
}

/// A request's deadline passed with no answer (appendix C.3.3): it goes out
/// again with the same key, or after the last send the part fails as
/// `timed out`. A timed-out part is never replaced (D9).
fn deadline(prog: &Program, u: &mut Unit, key: &str, out: &mut Out) -> Result<(), Refusal> {
    let Some(at) = open_request(&u.root, &u.label, key) else {
        return Err(Refusal { note: format!("ignored a deadline for {key}: no open request"), again: None });
    };
    let kind = node_at(&u.root, &at).kind.clone();
    let effect = prog.effect_of(&kind).expect("a requested part has an effect");
    let n = node_at_mut(&mut u.root, &at);
    let attempts = prog.attempts;
    if n.sends < attempts {
        n.sends += 1;
        let sends = n.sends;
        u.notes.push(format!(
            "{kind}: no answer within {}: asking again (send {sends} of {attempts})",
            duration(effect.within_secs)
        ));
        out.asks.push(ask_for(&u.label, View::of(u), &at, effect));
        return Ok(());
    }
    n.status = Status::Failed;
    n.outcome = Some(TIMED_OUT.to_string());
    u.notes.push(format!("{kind} timed out after {attempts} sends"));
    // A time-out is a fact: it is always recorded. Reactions that would
    // break a rule are held back instead.
    let mut t = u.root.clone();
    let mut notes = vec![];
    react(prog, &mut t, &u.facts, &at, &kind, TIMED_OUT, &mut notes);
    match newly_broken(prog, View::of(u), &t) {
        Some(r) => u.notes.push(format!("reaction held: rule {}: {}", r.name, r.paraphrase)),
        None => {
            u.root = t;
            u.notes.extend(notes);
        }
    }
    Ok(())
}

/// A goal's deadline came (appendix C.3.4). If the goal applies and is not
/// met, the unit records one alert, with why: each current part not done,
/// and each need held back by a rule.
fn goal_due(prog: &Program, u: &mut Unit, goal: u32) {
    let Some(g) = prog.goals.get(goal as usize) else { return };
    if g.kind != u.root.kind || u.met.contains(&goal) || u.alerts.contains(&goal) {
        return;
    }
    if !g.filter.iter().all(|f| eval(prog, View::of(u), &[], f) == T::Yes) {
        return;
    }
    u.alerts.push(goal);
    u.notes.push(format!("alert: not met within {}: {}", duration(g.within_secs.unwrap_or(0)), g.source));
    for at in positions(&u.root) {
        let n = node_at(&u.root, &at);
        for (part, rule) in &n.held {
            u.notes.push(format!("  because {part} waits: rule {rule}"));
        }
        for (id, b) in &n.binds {
            if matches!(b, Bind::Waiting { .. }) {
                u.notes.push(format!("  because {} waits for a {}", n.kind, target_of(id)));
            }
        }
        let Some((&i, owner)) = at.split_last() else { continue };
        if current(node_at(&u.root, owner), &n.kind) != Some(i) {
            continue;
        }
        let why = match (n.status, &n.waiting) {
            (Status::Done, _) => continue,
            (Status::Failed, _) => format!("{} {}", n.kind, n.outcome.as_deref().unwrap_or("failed")),
            (Status::Requested, _) => format!("{} has no answer yet", n.kind),
            (Status::Idle, Some(rule)) => format!("{} waits: rule {rule}", n.kind),
            (Status::Idle, None) => format!("{} is not done", n.kind),
        };
        u.notes.push(format!("  because {why}"));
    }
}

/// `30 seconds`, `3 days`: the largest unit up to days that divides evenly.
pub fn duration(secs: u64) -> String {
    let (n, unit) = [(86_400, "day"), (3_600, "hour"), (60, "minute")]
        .into_iter()
        .find(|(n, _)| secs >= *n && secs % n == 0)
        .map_or((secs, "second"), |(n, unit)| (secs / n, unit));
    format!("{n} {unit}{}", if n == 1 { "" } else { "s" })
}

/// Applies the `when` reactions to an outcome (spec §3.3).
fn react(prog: &Program, root: &mut Node, facts: &Facts, at: &[usize], kind: &str, outcome: &str, notes: &mut Vec<String>) {
    for r in prog.reactions.iter().filter(|r| r.kind == kind && r.outcome == outcome) {
        for a in &r.actions {
            let Some(target) = here(View::bare(root, facts), at, &a.target.steps) else { continue };
            let Some(calc) = &a.becomes else {
                let n = node_at_mut(root, &target);
                n.attrs.insert(a.attr.clone(), Val::Bool(a.value));
                notes.push(format!("{} is {}{}", n.kind, if a.value { "" } else { "not " }, a.attr));
                continue;
            };
            let value = match num(prog, View::bare(root, facts), at, calc) {
                None => {
                    notes.push(format!("{} not set: its new value is not known yet", a.attr));
                    continue;
                }
                Some(Val::Count(c)) if c < 0 => {
                    notes.push(format!("{} not set: a count cannot be negative", a.attr));
                    continue;
                }
                Some(v) => v,
            };
            let n = node_at_mut(root, &target);
            notes.push(format!("{}'s {} becomes {}", n.kind, a.attr, value.show(&prog.currency)));
            n.attrs.insert(a.attr.clone(), value);
        }
    }
}

fn settle(prog: &Program, u: &mut Unit, out: &mut Out, now: i64) {
    for _ in 0..SETTLE_ROUNDS {
        let changed = positions(&u.root).iter().any(|at| {
            needs_pass(prog, u, at, now, out) || bind_pass(prog, u, at, out) || done_pass(prog, u, at) || request_pass(prog, u, at, out)
        });
        if !changed {
            return;
        }
    }
    u.notes.push("stopped settling: the program keeps changing this unit".into());
}

/// Creates a part an active need lacks, replaces one that failed, or
/// withdraws one whose need is no longer active and that has not reached the
/// outside world (spec §3.2, D9). A part whose creation would break a rule
/// waits on that rule, as a request does (appendix B.3.5). A slot's need
/// works on its current alternative: an empty slot is filled, and one whose
/// alternative failed for good moves on (appendix D.3). Makes at most one
/// change.
fn needs_pass(prog: &Program, u: &mut Unit, at: &[usize], now: i64, out: &mut Out) -> bool {
    enum Change {
        Add(Node, String),
        Remove(usize, String),
        Hold(String, String),
        Release(String),
        Fill(String, String, String),
        Unfill(String),
    }
    let key = node_key(&u.label, &u.root, at);
    let change = {
        let n = node_at(&u.root, at);
        if n.status == Status::Failed {
            return false;
        }
        let mut change = None;
        for need in prog.needs.iter().filter(|need| need.owner == n.kind) {
            let active = need.cond.as_ref().is_none_or(|c| eval(prog, View::of(u), at, c) == T::Yes);
            let slot = prog.slot(&need.part);
            let want = match slot {
                None => need.part.clone(),
                Some(s) => match n.slots.get(&s.name) {
                    Some(alt) => alt.clone(),
                    None if active => {
                        change = choose(s, None).map(|first| Change::Fill(s.name.clone(), first.clone(), format!("{}: trying {first}", s.name)));
                        if change.is_some() {
                            break;
                        }
                        continue;
                    }
                    None => continue,
                },
            };
            let add = match current(n, &want).map(|i| (i, &n.parts[i])) {
                None if active => Some((blank(prog, &want, 1, now), format!("needs {want}"))),
                Some((_, p))
                    if active && p.status == Status::Failed && p.attempt < prog.attempts && p.outcome.as_deref() != Some(TIMED_OUT) =>
                {
                    let note = format!("{want} {}: attempt {}", p.outcome.as_deref().unwrap_or("failed"), p.attempt + 1);
                    Some((blank(prog, &want, p.attempt + 1, now), note))
                }
                Some((_, p)) if active && p.status == Status::Failed => {
                    // Failed for good: a slot moves on to its next alternative.
                    if let Some((s, next)) = slot.and_then(|s| choose(s, Some(&want)).map(|next| (s, next))) {
                        change = Some(Change::Fill(s.name.clone(), next.clone(), format!("{}: {want} failed, trying {next}", s.name)));
                    }
                    None
                }
                Some((i, p)) if !active && untouched(p) => {
                    change = Some(Change::Remove(i, format!("{want} withdrawn")));
                    None
                }
                _ => None,
            };
            if let Some((part, note)) = add {
                // The part's formulas are computed in place before the rules
                // are checked, so a rule sees the values it will have.
                let mut t = u.root.clone();
                node_at_mut(&mut t, at).parts.push(part);
                let pos = child(at, node_at(&t, at).parts.len() - 1);
                while formulas(prog, &mut t, &u.facts, &pos) {}
                let part = node_at(&t, &pos).clone();
                change = match newly_broken(prog, View::of(u), &t) {
                    Some(r) if n.held.get(&want) == Some(&r.name) => None,
                    Some(r) => Some(Change::Hold(want.clone(), r.name.clone())),
                    None => Some(Change::Add(part, note)),
                };
            } else if change.is_none() && !active && n.held.contains_key(&want) {
                change = Some(Change::Release(want.clone()));
            } else if change.is_none() && !active && slot.is_some() && current(n, &want).is_none() {
                // An inactive need empties its slot; when it is active
                // again, the slot is filled afresh, with a new patience timer.
                change = Some(Change::Unfill(need.part.clone()));
            }
            if change.is_some() {
                break;
            }
        }
        change
    };
    let n = node_at_mut(&mut u.root, at);
    match change {
        Some(Change::Add(part, note)) => {
            n.held.remove(&part.kind);
            n.parts.push(part);
            u.notes.push(note);
            true
        }
        Some(Change::Hold(part, rule)) => {
            u.notes.push(format!("{part} waits: rule {rule}"));
            n.held.insert(part, rule);
            true
        }
        Some(Change::Release(part)) => {
            n.held.remove(&part);
            true
        }
        Some(Change::Fill(slot, alt, note)) => {
            n.slots.insert(slot.clone(), alt.clone());
            u.notes.push(note);
            ask_patience(prog, u, &key, &slot, &alt);
            true
        }
        Some(Change::Unfill(slot)) => {
            n.slots.remove(&slot);
            true
        }
        Some(Change::Remove(i, note)) => {
            give_back(&u.label, &u.root, &child(at, i), out);
            node_at_mut(&mut u.root, at).parts.remove(i);
            u.notes.push(note);
            true
        }
        None => false,
    }
}

/// Asks the host for the alternative's patience timer, if it has patience.
fn ask_patience(prog: &Program, u: &mut Unit, owner_key: &str, slot: &str, alt: &str) {
    let Some(s) = prog.slot(slot) else { return };
    let Some(i) = s.alternatives.iter().position(|a| a == alt) else { return };
    if let Some(secs) = s.patience[i] {
        let fill = u.timers.len() + 1;
        u.timers.push((format!("{owner_key}/{slot}:{alt}#{fill}"), secs));
    }
}

/// The slot a patience timer (`O1/Delivery:Shipment#1`, the number counting
/// the unit's fills) is for, if it should move on now: its owner's
/// position, the slot, the alternative it waited on and the next one. It
/// moves on only while that alternative is still current and its part has
/// not reached the outside world (appendix D.3.3).
pub fn patience_due<'p>(prog: &'p Program, unit: &Unit, id: &str) -> Option<(Vec<usize>, &'p str, String, String)> {
    let (base, _) = id.rsplit_once('#')?;
    let (left, alt) = base.rsplit_once(':')?;
    let (owner_key, slot) = left.rsplit_once('/')?;
    // Only the latest fill of this slot counts; an older timer is stale.
    let latest = unit.timers.iter().rev().find(|(t, _)| t.starts_with(&format!("{left}:")))?;
    if latest.0 != id {
        return None;
    }
    let at = positions(&unit.root).into_iter().find(|at| node_key(&unit.label, &unit.root, at) == owner_key)?;
    let n = node_at(&unit.root, &at);
    if n.slots.get(slot).map(String::as_str) != Some(alt) {
        return None;
    }
    if current(n, alt).is_some_and(|i| !untouched(&n.parts[i])) {
        return None;
    }
    let s = prog.slot(slot)?;
    let next = choose(s, Some(alt))?;
    Some((at, s.name.as_str(), alt.to_string(), next))
}

/// Moves a slot on when its patience ran out: an untouched part is withdrawn.
fn patience(prog: &Program, u: &mut Unit, id: &str, out: &mut Out) -> bool {
    let Some((at, slot, alt, next)) = patience_due(prog, u, id) else { return false };
    let secs = prog.slot(slot).and_then(|s| s.patience[s.alternatives.iter().position(|a| *a == alt)?]).unwrap_or(0);
    let key = node_key(&u.label, &u.root, &at);
    if let Some(i) = current(node_at(&u.root, &at), &alt) {
        give_back(&u.label, &u.root, &child(&at, i), out);
    }
    let n = node_at_mut(&mut u.root, &at);
    if let Some(i) = current(n, &alt) {
        n.parts.remove(i);
    }
    n.held.remove(&alt);
    n.slots.insert(slot.to_string(), next.clone());
    u.notes.push(format!("{slot}: no {alt} after {}, trying {next}", duration(secs)));
    ask_patience(prog, u, &key, slot, &next);
    true
}

/// A part with no effect of its own is done once every active need of it is
/// done (appendix B.3.2), unless being done would break a rule: then it waits
/// on that rule. A root is never done; its goals describe it.
fn done_pass(prog: &Program, u: &mut Unit, at: &[usize]) -> bool {
    let n = node_at(&u.root, at);
    if at.is_empty() || n.status != Status::Idle || prog.effect_of(&n.kind).is_some() || !needs_done(prog, View::of(u), at) || !bound(prog, &u.root, at) {
        return false;
    }
    let (kind, waiting) = (n.kind.clone(), n.waiting.clone());
    let mut t = u.root.clone();
    node_at_mut(&mut t, at).status = Status::Done;
    match newly_broken(prog, View::of(u), &t) {
        Some(r) if waiting.as_deref() == Some(r.name.as_str()) => false,
        Some(r) => {
            node_at_mut(&mut u.root, at).waiting = Some(r.name.clone());
            u.notes.push(format!("{kind} waits: rule {}", r.name));
            true
        }
        None => {
            node_at_mut(&mut t, at).waiting = None;
            u.root = t;
            u.notes.push(format!("{kind} done"));
            true
        }
    }
}

/// Whether every active need of the node at `at` has a current part that is done.
fn needs_done(prog: &Program, v: View, at: &[usize]) -> bool {
    let n = node_at(v.root, at);
    prog.needs.iter().filter(|need| need.owner == n.kind).all(|need| {
        let active = need.cond.as_ref().is_none_or(|c| eval(prog, v, at, c) == T::Yes);
        let want = if prog.slot(&need.part).is_some() { n.slots.get(&need.part).cloned() } else { Some(need.part.clone()) };
        !active || want.and_then(|w| current(n, &w)).is_some_and(|i| n.parts[i].status == Status::Done)
    })
}

/// Sends a ready part's request, unless a rule would break. Then the part
/// waits on that rule, and every later step of the unit checks it again
/// (appendix B.3.5).
fn request_pass(prog: &Program, u: &mut Unit, at: &[usize], out: &mut Out) -> bool {
    let n = node_at(&u.root, at);
    let Some(effect) = prog.effect_of(&n.kind) else { return false };
    if n.status != Status::Idle || !needs_done(prog, View::of(u), at) || !bound(prog, &u.root, at) {
        return false;
    }
    let (kind, waiting) = (n.kind.clone(), n.waiting.clone());
    let mut t = u.root.clone();
    node_at_mut(&mut t, at).status = Status::Requested;
    match broken_rule(prog, View::of(u).with(&t)) {
        Some(r) if waiting.as_deref() == Some(r.name.as_str()) => false,
        Some(r) => {
            node_at_mut(&mut u.root, at).waiting = Some(r.name.clone());
            u.notes.push(format!("{kind} waits: rule {}", r.name));
            true
        }
        None => {
            let n = node_at_mut(&mut t, at);
            n.waiting = None;
            n.sends = 1;
            u.root = t;
            let a = ask_for(&u.label, View::of(u), at, effect);
            let sends: Vec<String> = a.sends.iter().map(|(k, v)| format!("{k} {}", v.show(&prog.currency))).collect();
            u.notes.push(format!("{kind} → {} via {} ({})", effect.verb, effect.adapter, sends.join(", ")));
            out.asks.push(a);
            true
        }
    }
}

/// Records each goal with a deadline the first time the root matches its
/// subject, so the host starts its timer (appendix C.3.4), and notes each
/// goal the first time it is met: late, if it already raised an alert.
fn goals(prog: &Program, u: &mut Unit) {
    for (i, g) in prog.goals.iter().enumerate() {
        let i = i as u32;
        if g.kind != u.root.kind {
            continue;
        }
        let applies = g.filter.iter().all(|f| eval(prog, View::of(u), &[], f) == T::Yes);
        if applies && g.within_secs.is_some() && !u.matched.contains(&i) {
            u.matched.push(i);
        }
        if !u.met.contains(&i) && applies && g.states.iter().all(|s| state_at(prog, View::of(u), &[], s)) {
            u.met.push(i);
            let late = if u.alerts.contains(&i) { " late" } else { "" };
            u.notes.push(format!("goal met{late}: {}", g.source));
        }
    }
}

/// Whether the unit's node at `at` is in `state` (appendix B.3): `done`,
/// `failed`, a Bool attribute, its effect's marks or outcomes, or a defined state.
pub fn in_state(prog: &Program, u: &Unit, at: &[usize], state: &str) -> bool {
    state_at(prog, View::of(u), at, state)
}

fn state_at(prog: &Program, v: View, at: &[usize], state: &str) -> bool {
    let n = node_at(v.root, at);
    match state {
        "done" => return n.status == Status::Done,
        "failed" => return n.status == Status::Failed,
        _ => {}
    }
    if let Some(Val::Bool(b)) = n.attrs.get(state) {
        return *b;
    }
    if let Some(e) = prog.effect_of(&n.kind) {
        if e.marks.as_deref() == Some(state) {
            return n.status != Status::Idle;
        }
        if e.success == state || e.failures.iter().any(|f| f == state) || state == TIMED_OUT {
            return n.outcome.as_deref() == Some(state);
        }
    }
    prog.defines.iter().any(|d| d.kind == n.kind && d.state == state && eval(prog, v, at, &d.cond) == T::Yes)
}

/// Whether a condition is known to hold at the unit's node at `at`.
pub fn holds(prog: &Program, u: &Unit, at: &[usize], c: &Cond) -> bool {
    eval(prog, View::of(u), at, c) == T::Yes
}

/// The first rule the unit's state breaks, if any. A rule applies to a node
/// only when its filter is known to hold.
pub fn violated<'p>(prog: &'p Program, u: &Unit) -> Option<&'p Rule> {
    broken_rule(prog, View::of(u))
}

fn broken_rule<'p>(prog: &'p Program, v: View) -> Option<&'p Rule> {
    prog.rules.iter().find(|r| broken(prog, v, r))
}

/// The first rule that a change from `before` to `after` breaks: one that
/// `after` breaks and `before` does not. A rule already broken while parts
/// are still being created (such as `every Order has a Shipment`) does not
/// hold back the change that mends it.
fn newly_broken<'p>(prog: &'p Program, before: View, after: &Node) -> Option<&'p Rule> {
    prog.rules.iter().find(|r| broken(prog, before.with(after), r) && !broken(prog, before, r))
}

fn broken(prog: &Program, v: View, r: &Rule) -> bool {
    positions(v.root).iter().any(|at| {
        node_at(v.root, at).kind == r.subject
            && r.filter.iter().all(|f| eval(prog, v, at, f) == T::Yes)
            && match &r.body {
                Body::Every(c) => eval(prog, v, at, c) == T::No,
                Body::No { pred, unless } => {
                    eval(prog, v, at, pred) == T::Yes && unless.as_ref().is_none_or(|u| eval(prog, v, at, u) != T::Yes)
                }
            }
    })
}

/// Three-valued truth. A value not given yet makes a comparison unknown
/// (appendix B.3.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum T {
    Yes,
    No,
    Unknown,
}

fn truth(b: bool) -> T {
    if b { T::Yes } else { T::No }
}

fn eval(prog: &Program, v: View, at: &[usize], c: &Cond) -> T {
    match c {
        Cond::Cmp { left, op, right } => {
            let (Some(l), Some(r)) = (num(prog, v, at, left), num(prog, v, at, right)) else { return T::Unknown };
            compare(&l, &r, *op).map_or(T::No, truth)
        }
        Cond::State { node, state, negated } => {
            let yes = match locate(v, at, &node.steps) {
                Some(Place::Here(n)) => state_at(prog, v, &n, state),
                // A shared entity's only states are its Bool attributes (appendix F.2.3).
                Some(Place::There(l)) => v.facts.get(&l).is_some_and(|f| f.attrs.get(state) == Some(&Val::Bool(true))),
                None => false,
            };
            truth(yes != *negated)
        }
        Cond::Has { node, part, state, negated } => {
            let exists = here(v, at, &node.steps).is_some_and(|n| {
                node_at(v.root, &n).parts.iter().enumerate().any(|(i, p)| {
                    p.kind == *part && state.as_ref().is_none_or(|s| state_at(prog, v, &child(&n, i), s))
                })
            });
            truth(exists != *negated)
        }
        Cond::Contains { set, item, negated } => {
            let (Some(Val::Set(ls)), Some(Val::Ref(l))) = (num(prog, v, at, set), num(prog, v, at, item)) else { return T::Unknown };
            truth(ls.contains(&l) != *negated)
        }
        Cond::And(cs) => {
            let ts: Vec<T> = cs.iter().map(|c| eval(prog, v, at, c)).collect();
            if ts.contains(&T::No) {
                T::No
            } else if ts.contains(&T::Unknown) {
                T::Unknown
            } else {
                T::Yes
            }
        }
        Cond::Or(cs) => {
            let ts: Vec<T> = cs.iter().map(|c| eval(prog, v, at, c)).collect();
            if ts.contains(&T::Yes) {
                T::Yes
            } else if ts.contains(&T::Unknown) {
                T::Unknown
            } else {
                T::No
            }
        }
    }
}

/// A calculation's value at the node at `at`, or `None` while a value it
/// needs is unknown, or on overflow or division by zero (appendix E.2.3).
fn num(prog: &Program, v: View, at: &[usize], n: &Num) -> Option<Val> {
    match n {
        Num::Path(p) => value(v, at, p),
        Num::Lit(l) => Some(Val::from_lit(l)),
        Num::Bin(x, op, y) => arith(num(prog, v, at, x)?, *op, num(prog, v, at, y)?),
        Num::Sum { of, kind, calc, filter, ty } => {
            let mut total = zero(*ty);
            for pos in relation(prog, v, at, of, kind, filter.as_deref())? {
                total = arith(total, Arith::Add, num(prog, v, &pos, calc)?)?;
            }
            Some(total)
        }
        Num::Count { of, kind, filter } => Some(Val::Count(relation(prog, v, at, of, kind, filter.as_deref())?.len() as i64)),
        Num::Taken { of, name, ty } => match locate(v, at, &of.steps)? {
            Place::Here(p) if p.is_empty() => Some(v.taken.get(name).cloned().unwrap_or_else(|| zero(*ty))),
            _ => None,
        },
    }
}

/// Nothing, as an amount of type `ty`.
fn zero(ty: Ty) -> Val {
    match ty {
        Ty::Money => Val::Money(0),
        Ty::Duration => Val::Duration(0),
        _ => Val::Count(0),
    }
}

/// Every part of `kind` under the node at `of`, all attempts, that the
/// filter is known to hold for (appendix E.2.1).
fn relation(prog: &Program, v: View, at: &[usize], of: &Path, kind: &str, filter: Option<&Cond>) -> Option<Vec<Vec<usize>>> {
    let owner = here(v, at, &of.steps)?;
    let n = node_at(v.root, &owner);
    Some(
        (0..n.parts.len())
            .filter(|&i| n.parts[i].kind == kind)
            .map(|i| child(&owner, i))
            .filter(|pos| filter.is_none_or(|f| eval(prog, v, pos, f) == T::Yes))
            .collect(),
    )
}

/// Checked arithmetic on amounts and counts; division rounds toward zero.
fn arith(x: Val, op: Arith, y: Val) -> Option<Val> {
    let f = |a: i64, b: i64| match op {
        Arith::Add => a.checked_add(b),
        Arith::Sub => a.checked_sub(b),
        Arith::Mul => a.checked_mul(b),
        Arith::Div => a.checked_div(b),
    };
    match (x, y) {
        (Val::Money(a), Val::Money(b)) if matches!(op, Arith::Add | Arith::Sub) => f(a, b).map(Val::Money),
        (Val::Money(a), Val::Count(b)) if matches!(op, Arith::Mul | Arith::Div) => f(a, b).map(Val::Money),
        (Val::Count(a), Val::Money(b)) if op == Arith::Mul => f(a, b).map(Val::Money),
        (Val::Count(a), Val::Count(b)) => f(a, b).map(Val::Count),
        (Val::Time(a), Val::Time(b)) if op == Arith::Sub => f(a, b).map(Val::Duration),
        (Val::Time(a), Val::Duration(b)) if matches!(op, Arith::Add | Arith::Sub) => f(a, b).map(Val::Time),
        (Val::Duration(a), Val::Time(b)) if op == Arith::Add => f(a, b).map(Val::Time),
        (Val::Duration(a), Val::Duration(b)) if matches!(op, Arith::Add | Arith::Sub) => f(a, b).map(Val::Duration),
        (Val::Duration(a), Val::Count(b)) if matches!(op, Arith::Mul | Arith::Div) => f(a, b).map(Val::Duration),
        (Val::Count(a), Val::Duration(b)) if op == Arith::Mul => f(a, b).map(Val::Duration),
        _ => None,
    }
}

/// Compares two values of the same type; `None` when the types differ.
fn compare(l: &Val, r: &Val, op: Op) -> Option<bool> {
    let ord = match (l, r) {
        (Val::Money(a), Val::Money(b)) | (Val::Count(a), Val::Count(b)) => a.cmp(b),
        (Val::Time(a), Val::Time(b)) | (Val::Duration(a), Val::Duration(b)) => a.cmp(b),
        (Val::Text(a), Val::Text(b)) | (Val::Ref(a), Val::Ref(b)) => a.cmp(b),
        (Val::Bool(a), Val::Bool(b)) => a.cmp(b),
        _ => return None,
    };
    Some(match op {
        Op::Eq => ord.is_eq(),
        Op::Ne => ord.is_ne(),
        Op::Lt => ord.is_lt(),
        Op::Le => ord.is_le(),
        Op::Gt => ord.is_gt(),
        Op::Ge => ord.is_ge(),
    })
}

/// The node at a position: indexes into `parts`, starting at the root.
pub fn node_at<'a>(root: &'a Node, at: &[usize]) -> &'a Node {
    at.iter().fold(root, |n, &i| &n.parts[i])
}

fn node_at_mut<'a>(root: &'a mut Node, at: &[usize]) -> &'a mut Node {
    at.iter().fold(root, |n, &i| &mut n.parts[i])
}

/// Every position in the unit, owners before their parts.
fn positions(root: &Node) -> Vec<Vec<usize>> {
    fn walk(n: &Node, at: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
        out.push(at.clone());
        for (i, p) in n.parts.iter().enumerate() {
            at.push(i);
            walk(p, at, out);
            at.pop();
        }
    }
    let mut out = vec![];
    walk(root, &mut vec![], &mut out);
    out
}

/// The current attempt of a part kind: the last one created.
fn current(n: &Node, kind: &str) -> Option<usize> {
    n.parts.iter().rposition(|p| p.kind == kind)
}

fn child(at: &[usize], i: usize) -> Vec<usize> {
    let mut pos = at.to_vec();
    pos.push(i);
    pos
}

/// Nothing in it has been requested from the outside world.
fn untouched(n: &Node) -> bool {
    n.status == Status::Idle && n.parts.iter().all(untouched)
}

/// Follows parts, owners and references. `None` if a part is missing, a
/// reference is not set or not carried, or the walk goes above the root.
fn locate(v: View, from: &[usize], steps: &[Step]) -> Option<Place> {
    let mut place = Place::Here(from.to_vec());
    for s in steps {
        place = match (place, s) {
            (Place::Here(mut at), Step::Owner) => {
                at.pop()?;
                Place::Here(at)
            }
            (Place::Here(mut at), Step::Part(k)) => {
                at.push(current(node_at(v.root, &at), k)?);
                Place::Here(at)
            }
            (Place::Here(mut at), Step::Slot(s)) => {
                let n = node_at(v.root, &at);
                at.push(current(n, n.slots.get(s)?)?);
                Place::Here(at)
            }
            (Place::Here(at), Step::Ref(a)) => match node_at(v.root, &at).attrs.get(a) {
                Some(Val::Ref(l)) => Place::There(l.clone()),
                _ => return None,
            },
            (Place::There(l), Step::Ref(a)) => match v.facts.get(&l)?.attrs.get(a) {
                Some(Val::Ref(next)) => Place::There(next.clone()),
                _ => return None,
            },
            (Place::Here(at), Step::Bound(k)) => match node_at(v.root, &at).binds.get(k) {
                Some(Bind::Granted { to, .. }) => Place::There(to.clone()),
                _ => return None,
            },
            (_, Step::Cand) => Place::There(v.cand?.to_string()),
            (_, Step::Elem) => Place::Here(v.elem?.to_vec()),
            (Place::There(_), _) | (_, Step::Attr(_)) => return None,
        };
        if let Place::There(l) = &place {
            v.facts.get(l)?;
        }
    }
    Some(place)
}

/// A walk that must end at a node of this unit.
fn here(v: View, from: &[usize], steps: &[Step]) -> Option<Vec<usize>> {
    match locate(v, from, steps)? {
        Place::Here(at) => Some(at),
        Place::There(_) => None,
    }
}

/// The value a path ends at, if it has one yet.
fn value(v: View, from: &[usize], path: &Path) -> Option<Val> {
    let (last, rest) = path.steps.split_last()?;
    let Step::Attr(name) = last else { return None };
    match locate(v, from, rest)? {
        Place::Here(at) if name == CREATED => Some(Val::Time(node_at(v.root, &at).created)),
        Place::Here(at) => node_at(v.root, &at).attrs.get(name).cloned(),
        Place::There(l) => {
            let f = v.facts.get(&l)?;
            if name == CREATED { Some(Val::Time(f.created)) } else { f.attrs.get(name).cloned() }
        }
    }
}

fn ask_for(label: &str, v: View, at: &[usize], e: &Effect) -> Ask {
    let sends = e.sends.iter().filter_map(|(name, path)| value(v, at, path).map(|x| (name.clone(), x))).collect();
    Ask {
        adapter: e.adapter.clone(),
        verb: e.verb.clone(),
        kind: e.kind.clone(),
        label: label.to_string(),
        key: request_key(label, v.root, at),
        sends,
    }
}

/// A node's key: the unit's label for the root, its request key for a part.
fn node_key(label: &str, root: &Node, at: &[usize]) -> String {
    if at.is_empty() { label.to_string() } else { request_key(label, root, at) }
}

/// `O1/CustomsForm#1/Declaration#1`: the unit, then each part from the root
/// down with its attempt, so the parts of a replaced owner get new keys.
pub fn request_key(label: &str, root: &Node, at: &[usize]) -> String {
    let mut steps = vec![];
    let mut n = root;
    for &i in at {
        n = &n.parts[i];
        steps.push(format!("{}#{}", n.kind, n.attempt));
    }
    format!("{label}/{}", steps.join("/"))
}
