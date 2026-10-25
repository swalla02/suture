//! The interpreter (spec appendix B.3): one step of a unit for one message.
//! It is a pure function of the program, the unit and the message, so
//! replaying the journal reproduces every step.

use suture_lang::ir::Mark;
use std::collections::BTreeMap;

use suture_lang::ir::{Action, Arith, Body, Cond, Criterion, Decision, Effect, Num, Op, Otherwise, Path, Program, Rule, Slot, Source, Step, Ty, CREATED, PASSES, TIMED_OUT};

use crate::choice::order;
use crate::state::{period_key, periods_in, span_key, take_text, work_span, work_span_key, time_text, SPAN_DAYS, Ask, Bind, Decide, Fact, Facts, Fetch, Msg, Node, Status, Unit, Val};

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
    /// The period a take falls in, for the totals per period (appendix F.2.12).
    pub period: Option<&'a str>,
    /// The group a take falls in, for `no two … have the same …` (appendix AP.2.2).
    pub group: Option<&'a str>,
    /// In an `admit` condition: the caller's unit (appendix G.2.3).
    pub caller: Option<&'a str>,
    /// In an `admit` condition or an event's reaction: the data the event
    /// carries (appendix Q.2.2).
    pub data: &'a [(String, Val)],
    /// When a step checks a rule before acting: the clock. Sourced values
    /// are then known only while fresh, and strict rules count what they
    /// cannot know against the action (appendix I.2.3–I.2.4).
    pub now: Option<i64>,
    /// While a rule is checked: its `with a at most … old` bounds.
    pub bounds: &'a [(String, u64)],
    /// The version the unit was created under; `None` for a tree being
    /// built, which is of the current version (appendix M.2.3).
    pub version: Option<u32>,
}

impl<'a> View<'a> {
    pub fn of(u: &'a Unit) -> View<'a> {
        View { root: &u.root, facts: &u.facts, taken: &u.taken, cand: None, elem: None, period: None, group: None, caller: None, data: &[], now: None, bounds: &[], version: Some(u.version) }
    }

    /// The view a step checks its rules with before acting at `now`.
    fn acting(self, now: i64) -> View<'a> {
        View { now: Some(now), ..self }
    }

    /// A tree being built, with its facts and nothing taken yet.
    fn bare(root: &'a Node, facts: &'a Facts) -> View<'a> {
        View { root, facts, taken: &NO_TOTALS, cand: None, elem: None, period: None, group: None, caller: None, data: &[], now: None, bounds: &[], version: None }
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
    let unit = build_unit(prog, label, kind, given, items, Facts::new(), 0)?;
    refused_at_creation(prog, &unit, 0)?;
    Ok(unit)
}

/// The unit as it would be created, before any rule is checked: with the
/// elements of its lists (appendix E.2.6), each given as the list's name and
/// the element's attributes, the facts of the shared entities it refers to,
/// and the clock. Formulas are computed after defaults and given values, in
/// whatever order they read each other.
pub fn build_unit(
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
    // After the given values are checked, so a wrong one is named first.
    required(prog, kind, given, items)?;
    // Formulas may read each other and the owner in any order: compute
    // until nothing changes. Each attribute is set at most once.
    while (0..root.parts.len()).fold(formulas(prog, &mut root, &facts, &[]), |changed, i| formulas(prog, &mut root, &facts, &[i]) || changed) {}
    Ok(Unit {
        label: label.to_string(),
        root,
        notes: vec![],
        met: vec![],
        matched: vec![],
        alerts: vec![],
        timers: vec![],
        facts,
        taken: BTreeMap::new(),
        version: prog.version,
    })
}

/// A creation that its first step would leave breaking a rule is refused.
pub fn refused_at_creation(prog: &Program, unit: &Unit, now: i64) -> Result<(), String> {
    match violated(prog, &step_at(prog, unit, &Msg::Create, now).unit) {
        Some(r) => Err(format!("refused by rule {}: {}", r.name, r.paraphrase)),
        None => Ok(()),
    }
}

/// Whether an `admit` condition holds at the root of `u`, with the caller's
/// facts carried into it (appendix G.2.3). An unknown value does not admit.
pub fn admits(prog: &Program, u: &Unit, caller: Option<&str>, data: &[(String, Val)], at: &[usize], c: &Cond) -> bool {
    matches!(eval(prog, View { caller, data, ..View::of(u) }, at, c), T::Yes)
}

/// The unit's one part of `kind`, the current attempt, that an event goes
/// to (appendix R.2.2).
pub fn part_at(u: &Unit, kind: &str) -> Result<Vec<usize>, String> {
    let found: Vec<Vec<usize>> = positions(&u.root)
        .into_iter()
        .filter(|at| {
            let n = node_at(&u.root, at);
            n.kind == kind && !at.is_empty() && live(&u.root, at)
        })
        .collect();
    match found.as_slice() {
        [at] => Ok(at.clone()),
        [] => Err(format!("{} has no {kind}", u.label)),
        _ => Err(format!("{} has {} {kind} parts; an event goes to one", u.label, found.len())),
    }
}

/// A value for a state attribute must be one of its states (appendix Z.2.4).
pub fn state_value(kind: &str, a: &suture_lang::ir::Attr, v: &Val) -> Result<(), String> {
    match v {
        Val::Text(x) if !a.states.is_empty() && !a.states.contains(x) => Err(format!("{kind}.{} is one of {}, not {x}", a.name, a.states.join(", "))),
        _ => Ok(()),
    }
}

/// An attribute with no default, formula, source or effect answer that sets it must be given when its
/// entity, or an element of its lists, is made (appendix AE.2.1); an
/// element's reference to its owner is not given.
pub fn required(prog: &Program, kind: &str, given: &[(String, Val)], items: &[(String, Vec<(String, Val)>)]) -> Result<(), String> {
    missing(prog, kind, given, None)?;
    for (list, attrs) in items {
        if let Some(l) = prog.lists.iter().find(|l| l.owner == kind && l.name == *list) {
            missing(prog, &l.element, attrs, Some(kind))?;
        }
    }
    Ok(())
}

fn missing(prog: &Program, kind: &str, given: &[(String, Val)], owner: Option<&str>) -> Result<(), String> {
    let Some(k) = prog.kind(kind) else { return Ok(()) };
    let mut missing: Vec<&str> = k
        .attrs
        .iter()
        .filter(|a| a.default.is_none() && a.formula.is_none() && !given.iter().any(|(n, _)| *n == a.name))
        .filter(|a| !is_answered(prog, kind, &a.name))
        .filter(|a| !(a.ty == Ty::Ref && owner.is_some() && a.of.as_deref() == owner))
        .map(|a| a.name.as_str())
        .collect();
    missing.sort_unstable();
    match missing.as_slice() {
        [] => Ok(()),
        [one] => Err(format!("a new {kind} needs its {one}: it has no default")),
        [first @ .., last] => Err(format!("a new {kind} needs its {} and {last}: they have no default", first.join(", "))),
    }
}

/// Sets given attributes, which must exist, have the right type and not be formulas.
fn give(prog: &Program, kind: &str, n: &mut Node, given: &[(String, Val)]) -> Result<(), String> {
    let k = prog.kind(kind).ok_or_else(|| format!("there is no kind {kind}"))?;
    for (name, v) in given {
        let a = k.attr(name).ok_or_else(|| format!("{kind} has no attribute {name}"))?;
        if a.formula.is_some() {
            return Err(format!("{kind}.{name} is computed by its formula; it is not given"));
        }
        if prog.source_of(kind, name).is_some() {
            return Err(format!("{kind}.{name} comes from its source; it is not given"));
        }
        if v.ty() != a.ty {
            let want = a.of.as_ref().map_or_else(|| format!("{:?}", a.ty), |k| format!("{:?} of {k}", a.ty));
            return Err(format!("{kind}.{name} is {want}, not {}", v.show(&prog.currency)));
        }
        state_value(kind, a, v)?;
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

/// A unit carried into a new version (appendix M.2.5): attributes the new
/// version removes are dropped, added ones take their defaults, and the
/// goals already met or alerted keep that, by their text. The step then
/// settles it under the new version.
fn upgrade(prog: &Program, u: &mut Unit, goals: &[Option<u32>], facts: &Facts) {
    for (l, f) in facts {
        u.facts.insert(l.clone(), f.clone());
    }
    let dropped = carry_over(prog, u);
    let map = |v: &[u32]| v.iter().filter_map(|g| goals.get(*g as usize).copied().flatten()).collect::<Vec<u32>>();
    (u.met, u.matched, u.alerts) = (map(&u.met), map(&u.matched), map(&u.alerts));
    let dropped = if dropped.is_empty() { String::new() } else { format!(", dropped {}", dropped.join(", ")) };
    u.notes.push(format!("now under version {}{dropped}", prog.version));
}

/// A unit's attributes under a new version: the removed ones dropped, the
/// added ones given their defaults, then their formulas, over the facts it
/// carries. Returns what was dropped, as `Kind's attr`, once each.
pub fn carry_over(prog: &Program, u: &mut Unit) -> Vec<String> {
    let mut dropped: Vec<String> = vec![];
    for f in u.facts.values_mut() {
        carry_attrs(prog, &f.kind, &mut f.attrs, &mut vec![]);
    }
    let all = positions(&u.root);
    for at in &all {
        let n = node_at_mut(&mut u.root, at);
        let kind = n.kind.clone();
        carry_attrs(prog, &kind, &mut n.attrs, &mut dropped);
    }
    // Every position each round: formulas may read each other in any order.
    recompute(prog, u);
    dropped
}

fn carry_attrs(prog: &Program, kind: &str, attrs: &mut BTreeMap<String, Val>, dropped: &mut Vec<String>) {
    let Some(k) = prog.kind(kind) else { return };
    attrs.retain(|a, _| {
        let keep = k.attr(a).is_some();
        let name = format!("{}'s {a}", k.name);
        if !keep && !dropped.contains(&name) {
            dropped.push(name);
        }
        keep
    });
    for a in &k.attrs {
        if let (false, Some(d)) = (attrs.contains_key(&a.name), &a.default) {
            attrs.insert(a.name.clone(), Val::from_lit(d));
        }
    }
}

/// The breaks a deploy of `prog` would make in this live unit, which was
/// running under `old` (appendix M.2.4): parts or attributes the new
/// version cannot carry, then the rules it would break once carried over.
pub fn upgrade_breaks(old: &Program, prog: &Program, unit: &Unit, goals: &[Option<u32>], facts: &Facts, now: i64) -> Vec<String> {
    let mut out = vec![];
    for at in positions(&unit.root) {
        let n = node_at(&unit.root, &at);
        let Some(k) = prog.kind(&n.kind) else {
            out.push(format!("kind {} is removed", n.kind));
            continue;
        };
        for a in &k.attrs {
            let was = old.kind(&n.kind).and_then(|o| o.attr(&a.name));
            match (was, n.attrs.get(&a.name)) {
                (Some(w), Some(_)) if w.ty != a.ty || w.of != a.of => {
                    let ty = |t: Ty, of: &Option<String>| of.clone().unwrap_or_else(|| format!("{t:?}"));
                    out.push(format!("{}'s {} changes from {} to {}", k.name, a.name, ty(w.ty, &w.of), ty(a.ty, &a.of)))
                }
                (_, Some(Val::Text(v))) if !a.states.is_empty() && !a.states.contains(v) => {
                    out.push(format!("{}'s {} is {v}, which this version no longer has: {}", k.name, a.name, a.states.join(", ")))
                }
                (Some(w), Some(_)) if (w.mark == Some(Mark::Secret)) != (a.mark == Some(Mark::Secret)) => {
                    let how = if a.mark == Some(Mark::Secret) { "becomes secret" } else { "stops being secret" };
                    out.push(format!("{}'s {} {how}, and live values are held as they were", k.name, a.name))
                }
                (None, None) if a.default.is_none() && a.formula.is_none() && !is_answered(prog, &k.name, &a.name) => {
                    out.push(format!("{}'s {} is added without a default", k.name, a.name))
                }
                _ => {}
            }
        }
    }
    out.sort();
    out.dedup();
    if out.is_empty() {
        let after = step_at(prog, unit, &Msg::Upgrade { goals: goals.to_vec(), facts: facts.clone() }, now).unit;
        if let Some(r) = violated(prog, &after) {
            out.push(format!("rule {} would break: {}", r.name, r.paraphrase));
        }
    }
    out
}

/// Whether an effect's answer or a source sets the attribute, rather than
/// whoever creates the entity.
fn is_answered(prog: &Program, kind: &str, attr: &str) -> bool {
    prog.effect_of(kind).is_some_and(|e| e.data.iter().any(|(n, _)| n == attr)) || prog.source_of(kind, attr).is_some()
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
        fetched: Default::default(),
        tried: Default::default(),
        choosing: Default::default(),
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
    crate::state::Work::step();
    let mut u = unit.clone();
    u.notes.clear();
    let mut out = Out::default();
    match msg {
        Msg::Create => {
            let attrs: Vec<String> = u.root.attrs.iter().map(|(k, v)| format!("{k} {}", shown(prog, k, v))).collect();
            u.notes.push(format!("placed: {}", attrs.join(", ")));
            passing(prog, &mut u, None, now);
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
        Msg::Answer { key, data, .. } if is_choice(key) => {
            if let Err(note) = chosen(prog, &mut u, key, data, &mut out) {
                return Outcome::kept(unit, note, Out::default());
            }
        }
        Msg::Answer { key, outcome, .. } if is_fetch(key) && outcome == TIMED_OUT => {
            if let Err(note) = fetch_deadline(prog, &mut u, key, now, &mut out) {
                return Outcome::kept(unit, note, Out::default());
            }
        }
        Msg::Answer { key, outcome, data } if is_fetch(key) => {
            if let Err(note) = fetch_answer(prog, &mut u, key, outcome, data, now, &mut out) {
                return Outcome::kept(unit, note, out);
            }
        }
        Msg::Due { id } if id.contains(PASS_MARK) => passed(prog, &mut u, id),
        Msg::Due { id } => fetch_due(&mut u, id),
        Msg::Upgrade { goals, facts } => upgrade(prog, &mut u, goals, facts),
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
            let refuse = |why: String| {
                let refused = Msg::Refused { key: key.clone(), by: u.label.clone(), rule: String::new() };
                Outcome::kept(unit, format!("refused {key}: {why}"), Out { sends: vec![(from.clone(), refused)], ..Out::default() })
            };
            // A take per period falls in one period, or in each a span covers (appendix AD.2.2).
            let periods = match taken.as_ref().and_then(|(name, _)| name.split_once('@')) {
                Some((_, at)) => match periods_in(at) {
                    Ok(ps) => ps.into_iter().map(Some).collect(),
                    Err(e) => return refuse(e),
                },
                None => vec![None],
            };
            if let Some((name, amount)) = taken {
                let base = name.split_once('@').map_or(name.as_str(), |(b, _)| b);
                let shares = name.split_once('@').and_then(|(_, at)| work_span(at));
                if shares.as_ref().is_some_and(|s| s.len() != periods.len()) {
                    return refuse(format!("the take {} has a share for each of {} periods, not {}", take_text(name), shares.as_ref().map_or(0, Vec::len), periods.len()));
                }
                for (i, p) in periods.iter().enumerate() {
                    let total = p.as_ref().map_or_else(|| name.clone(), |p| format!("{base}@{p}"));
                    let before = totals.get(&total).cloned().unwrap_or_else(|| zero(amount.ty()));
                    let amount = shares.as_ref().and_then(|s| s.get(i)).map_or_else(|| amount.clone(), |s| Val::Duration(*s));
                    let Some(after) = arith(before, Arith::Add, amount) else { return refuse(format!("the total {total} would overflow")) };
                    totals.insert(total, after);
                }
            }
            let group = taken.as_ref().and_then(|(name, _)| name.split_once('#')).map(|(_, g)| g);
            for period in &periods {
                if let Some(r) = broken_rule(prog, View { taken: &totals, period: period.as_deref(), group, ..View::of(&u) }) {
                    let on = match (period, taken.as_ref().is_some_and(|(n, _)| n.contains("@span "))) {
                        (Some(p), true) => format!(" on {p}"),
                        _ => String::new(),
                    };
                    let refused = Msg::Refused { key: key.clone(), by: u.label.clone(), rule: r.name.clone() };
                    return Outcome::kept(unit, format!("refused {key}: rule {}{on}: {}", r.name, r.paraphrase), Out { sends: vec![(from.clone(), refused)], ..Out::default() });
                }
            }
            u.taken = totals;
            u.notes.push(match taken {
                Some((name, amount)) => format!("granted {key}: {} {} taken", amount.show(&prog.currency), take_text(name)),
                None => format!("granted {key}"),
            });
            out.sends.push((from.clone(), Msg::Granted { key: key.clone(), by: u.label.clone(), taken: taken.clone(), facts: own_facts(prog, &u) }));
            return Outcome::new(u, out);
        }
        Msg::Release { from, key, taken } => {
            if let Some((name, amount)) = taken {
                let totals = match name.split_once('@') {
                    Some((base, at)) => periods_in(at).unwrap_or_default().into_iter().map(|p| format!("{base}@{p}")).collect(),
                    None => vec![name.clone()],
                };
                let shares = name.split_once('@').and_then(|(_, at)| work_span(at));
                for (i, total) in totals.into_iter().enumerate() {
                    let before = u.taken.get(&total).cloned().unwrap_or_else(|| zero(amount.ty()));
                    let amount = shares.as_ref().and_then(|s| s.get(i)).map_or_else(|| amount.clone(), |s| Val::Duration(*s));
                    if let Some(after) = arith(before, Arith::Sub, amount) {
                        u.taken.insert(total, after);
                    }
                }
                u.notes.push(format!("{from} gave back {} {} ({key})", amount.show(&prog.currency), take_text(name)));
            } else {
                u.notes.push(format!("{from} gave back {key}"));
            }
            return Outcome::new(u, out);
        }
        Msg::Granted { key, by, taken, facts } => granted(prog, &mut u, key, by, taken, facts, &mut out),
        Msg::Change { attr, value, by } => {
            u.notes.push(format!("{by} changes {attr} to {}", shown(prog, attr, value)));
            // The same value again moves nothing, so it passes nothing again.
            if u.root.attrs.insert(attr.clone(), value.clone()).as_ref() != Some(value) {
                passing(prog, &mut u, Some(attr), now);
            }
        }
        Msg::Refresh { facts } => {
            // Only the facts it carries. A refresh is a fact and is never
            // refused: the host checked the change against every reader first.
            for (l, f) in facts {
                if let Some(old) = u.facts.get_mut(l) {
                    *old = f.clone();
                }
            }
            u.notes.push(format!("refreshed {}", facts.keys().cloned().collect::<Vec<_>>().join(", ")));
        }
        Msg::Event { verb, by, part, data, facts } => {
            let at = if part.is_empty() { Ok(vec![]) } else { part_at(&u, part) };
            let Ok(at) = at else { return Outcome::kept(unit, format!("ignored {by} {verb} its {part}: it has none, or several"), out) };
            let with: Vec<String> = data.iter().map(|(n, v)| format!("{n} {}", shown(prog, n, v))).collect();
            let with = if with.is_empty() { String::new() } else { format!(" with {}", with.join(", ")) };
            let it = if part.is_empty() { "it".to_string() } else { format!("its {part}") };
            u.notes.push(format!("{by} {verb} {it}{with}"));
            let kind = node_at(&u.root, &at).kind.clone();
            let mut notes = vec![];
            let (role, name) = by.split_once(' ').unwrap_or((by.as_str(), ""));
            // The caller's facts are read in this step only, and not kept.
            let mut seen = unit.facts.clone();
            seen.extend(facts.iter().map(|(l, f)| (l.clone(), f.clone())));
            let caller = facts.contains_key(name).then_some(name);
            react(prog, &mut u.root, &seen, &at, &kind, verb, Some((role, caller, data)), &mut notes);
            u.notes.extend(notes);
        }
        Msg::Refused { key, by, rule } => {
            let Some((at, id)) = find_bind(&u, key) else { return Outcome::kept(unit, format!("ignored a refusal for {key}: it is gone"), out) };
            let n = node_at_mut(&mut u.root, &at);
            // A woken binding gathered again from scratch (appendix F.2.9), so
            // only this refusal counts.
            let idle = n.status == Status::Idle || is_held(prog, &id);
            let mut skip = match (n.binds.get(&id).cloned(), idle) {
                (Some(Bind::Asked { skip }), true) => skip,
                (Some(Bind::Waiting { .. }), true) => vec![],
                _ => return Outcome::kept(unit, format!("ignored a refusal for {key}: not asked"), out),
            };
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
                u.notes.push(format!("{kind} waits for {}", wanted(&id)));
            }
            return Outcome::new(u, out);
        }
    }
    settle(prog, &mut u, &mut out, now);
    goals(prog, &mut u);
    if let (Some(r), Msg::Answer { key, outcome, .. }) = (violated(prog, &u).filter(|_| !matches!(msg, Msg::Answer { key, .. } if is_fetch(key))), msg) {
        // A consequence no pass guards, such as a withdrawn part, would
        // break a rule: refuse the answer and ask again (appendix B.3.4).
        // A deadline is not an answer, so nothing is asked again for it.
        let again = open_request(&unit.root, &unit.label, key)
            .filter(|_| outcome != TIMED_OUT)
            .and_then(|at| prog.effect_of(&node_at(&unit.root, &at).kind).map(|e| ask_for(&unit.label, View::of(unit), &at, e)));
        let out = Out { asks: again.into_iter().collect(), ..Out::default() };
        return Outcome::kept(unit, format!("refused by rule {}: {}", r.name, r.paraphrase), out);
    }
    if let (Some(r), Msg::Event { by, verb, part, .. }) = (newly_violated(prog, unit, &u), msg) {
        let it = if part.is_empty() { "it".to_string() } else { format!("its {part}") };
        // The event's step would break a rule: it is refused, and its caller
        // is told which rule (appendix G.2.4).
        return Outcome::kept(unit, format!("refused {by} {verb} {it}: refused by rule {}: {}", r.name, r.paraphrase), Out::default());
    }
    if let (Some(r), Msg::Change { by, attr, .. }) = (newly_violated(prog, unit, &u), msg) {
        return Outcome::kept(unit, format!("refused {by} changes {attr}: refused by rule {}: {}", r.name, r.paraphrase), Out::default());
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
    // A held contribution, and a `no two` count moving to a new group, may
    // be asked for at any status (appendices AA.2.1, AX.2.2).
    if (n.status != Status::Idle && !is_held(prog, &id) && !is_unique(prog, &id)) || !matches!(n.binds.get(&id), Some(Bind::Asked { .. } | Bind::Waiting { .. })) {
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
    match id.split_once('(') {
        Some((_, total)) => u.notes.push(format!("{kind} counts toward {by}'s total {}", total.trim_end_matches(')'))),
        None => u.notes.push(format!("{kind} gets {} {by}{what}", target_of(&id))),
    }
}

/// What a waiting binding waits for: `a Stock`, or room in a total.
fn wanted(id: &str) -> String {
    match id.split_once('(') {
        Some(_) => format!("room in {}", toward(id)),
        None => format!("a {}", target_of(id)),
    }
}

/// `a Policy's total payout`, `the total amount`: the total a binding counts
/// toward, or `a Stock`.
pub fn toward(id: &str) -> String {
    match id.split_once('(') {
        Some((t, total)) if t.starts_with("all ") => format!("the total {}", total.trim_end_matches(')')),
        Some((t, total)) => format!("a {t}'s total {}", total.trim_end_matches(')')),
        None => format!("a {}", target_of(id)),
    }
}

/// `line`, for the element of the `per` binding of `kind` to `target`.
fn element_name(prog: &Program, kind: &str, target: &str) -> String {
    let b = prog.bindings.iter().find(|b| b.subject == kind && b.target == target);
    b.and_then(|b| b.per.as_ref()).and_then(|l| prog.lists.iter().find(|x| x.name == *l)).map_or_else(|| "element".into(), |l| l.singular())
}

/// The kind a binding id binds: `Stock` for `Stock.0`, `Customer` for
/// `Customer(amount of Refunds)`.
fn target_of(id: &str) -> String {
    id.split(['.', '(']).next().unwrap_or(id).to_string()
}

/// A unit as other units carry it: its own fact, and the facts it carries.
fn own_facts(prog: &Program, u: &Unit) -> Facts {
    let mut facts = u.facts.clone();
    facts.insert(u.label.clone(), fact_of(prog, u));
    facts
}

/// A unit's own fact, as the units that refer to it carry it: with the
/// totals across units they read (appendix AA.2.3).
pub fn fact_of(prog: &Program, u: &Unit) -> Fact {
    let read = |name: &String| prog.bindings.iter().any(|b| b.shared && b.target == u.root.kind && b.taking.as_ref().is_some_and(|t| t.name == *name));
    let taken = u.taken.iter().filter(|(n, _)| read(n)).map(|(n, v)| (n.clone(), v.clone())).collect();
    Fact { kind: u.root.kind.clone(), created: u.root.created, attrs: u.root.attrs.clone(), taken }
}

/// The part and binding id a binding key names, if the part still has it.
fn find_bind(u: &Unit, key: &str) -> Option<(Vec<usize>, String)> {
    let (node, id) = key.rsplit_once('@')?;
    let at = positions(&u.root).into_iter().find(|at| request_key(&u.label, &u.root, at) == node)?;
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
            out.push((b.id.clone(), i, None));
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
            out.push((format!("{}.{j}", b.id), i, Some((j as u32, child(owner, idx)))));
        }
    }
    out
}

/// Whether every binding of the node at `at`, and of each of its owners, is
/// granted. A contribution held while a filter holds counts only while it
/// is asked for (appendix AA.2.1), or while its filter holds and its amount
/// is not known yet (appendix AG.2.2).
fn bound(prog: &Program, u: &Unit, at: &[usize]) -> bool {
    (0..=at.len()).all(|d| {
        let n = node_at(&u.root, &at[..d]);
        bind_slots(prog, &u.root, &at[..d]).iter().all(|(id, i, _)| match n.binds.get(id) {
            Some(Bind::Granted { .. }) => true,
            None => prog.bindings[*i].held.is_some() && !unasked(prog, u, &at[..d]).iter().any(|(x, _)| x == id),
            _ => false,
        })
    })
}

/// Whether the binding `id` adds to a total only while a filter holds.
fn is_held(prog: &Program, id: &str) -> bool {
    prog.bindings.iter().any(|b| b.id == id && b.held.is_some())
}

fn is_unique(prog: &Program, id: &str) -> bool {
    prog.bindings.iter().any(|b| b.id == id && b.unique)
}

/// Adds the node's contribution to a filtered total across units when its
/// filter comes to hold, and gives it back when the filter stops holding
/// (appendix AA.2.1). Makes at most one change.
fn held_pass(prog: &Program, u: &mut Unit, at: &[usize], out: &mut Out) -> bool {
    for (id, i, _) in bind_slots(prog, &u.root, at) {
        let Some(filter) = &prog.bindings[i].held else { continue };
        // While the filter is unknown, the contribution stays as it is.
        let holds = match eval(prog, View::of(u), at, filter) {
            T::Yes => true,
            T::No => false,
            T::Unknown => continue,
        };
        let kind = node_at(&u.root, at).kind.clone();
        let total = id.split_once('(').map_or(id.as_str(), |(_, t)| t.trim_end_matches(')')).to_string();
        match (holds, node_at(&u.root, at).binds.get(&id).cloned()) {
            (true, None) => {
                let Some(d) = decide_for(prog, u, at, &id, vec![]) else { continue };
                node_at_mut(&mut u.root, at).binds.insert(id, Bind::Asked { skip: vec![] });
                out.decides.push(d);
                return true;
            }
            (false, Some(b)) => {
                if let Bind::Granted { to, taken } = &b {
                    let key = format!("{}@{id}", request_key(&u.label, &u.root, at));
                    out.sends.push((to.clone(), Msg::Release { from: u.label.clone(), key, taken: taken.clone() }));
                    u.notes.push(format!("{kind} no longer counts toward {to}'s total {total}"));
                } else {
                    u.notes.push(format!("{kind} no longer asks to count toward the total {total}"));
                }
                node_at_mut(&mut u.root, at).binds.remove(&id);
                return true;
            }
            _ => {}
        }
    }
    false
}

/// Moves a `no two` count to its new group when a value it compares
/// changes: the old take is given back and the new one asked for, and the host refuses the input if
/// the new one would break the rule (appendix AX.2.2). Makes at most one change.
fn regroup_pass(prog: &Program, u: &mut Unit, at: &[usize], out: &mut Out) -> bool {
    for (id, i, _) in bind_slots(prog, &u.root, at) {
        if !prog.bindings[i].unique {
            continue;
        }
        let Some(Bind::Granted { to, taken: Some((name, amount)) }) = node_at(&u.root, at).binds.get(&id).cloned() else { continue };
        // A reference never changes by a caller (appendix G.2.5), so the
        // count stays with the same entity; only its group moves.
        let Some(d) = decide_for(prog, u, at, &id, vec![]) else { continue };
        if d.taken.as_ref().is_some_and(|(n, _)| *n == name) {
            continue;
        }
        let key = format!("{}@{id}", request_key(&u.label, &u.root, at));
        out.sends.push((to.clone(), Msg::Release { from: u.label.clone(), key, taken: Some((name, amount)) }));
        let kind = node_at(&u.root, at).kind.clone();
        u.notes.push(format!("{kind} moves its count from {to}"));
        node_at_mut(&mut u.root, at).binds.insert(id, Bind::Asked { skip: vec![] });
        out.decides.push(d);
        return true;
    }
    false
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
            let name = match (&t.period, &t.span) {
                (Some(per), Some((start, end))) => {
                    let time = |n: &Num| match num(prog, v, at, n) {
                        Some(Val::Time(t)) => Some(t),
                        _ => None,
                    };
                    let (from, to) = (time(start)?, time(end)?);
                    let key = match t.work {
                        Some(cal) => work_span_key(per, from, to, &shares(prog.calendars.get(cal as usize)?, per, from, to)),
                        None => span_key(per, from, to),
                    };
                    format!("{}@{key}", t.name)
                }
                (Some(per), None) => format!("{}@{}", t.name, period_key(per, node_at(&u.root, at).created)),
                (None, _) if !t.group.is_empty() => {
                    let vals = t.group.iter().map(|n| Some((num(prog, v, at, n)?, is_personal(prog, n)))).collect::<Option<Vec<_>>>()?;
                    format!("{}#{}", t.name, group_key(prog, &vals))
                }
                (None, _) => t.name.clone(),
            };
            Some((name, num(prog, v, at, &t.amount)?))
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
    if node_at(&u.root, at).status != Status::Idle {
        return false;
    }
    for (id, i, _) in bind_slots(prog, &u.root, at) {
        if node_at(&u.root, at).binds.contains_key(&id) || prog.bindings[i].held.is_some() {
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

/// The kind of the part that makes a binding's choice, and its attributes.
pub fn chooser(prog: &Program, u: &Unit, d: &Decide) -> Option<(String, Vec<(String, Val)>)> {
    let (at, _) = decide_place(prog, u, d)?;
    let n = node_at(&u.root, &at);
    Some((n.kind.clone(), n.attrs.clone().into_iter().collect()))
}

fn decide_place(prog: &Program, u: &Unit, d: &Decide) -> Option<(Vec<usize>, Option<Vec<usize>>)> {
    let at = positions(&u.root).into_iter().find(|at| request_key(&u.label, &u.root, at) == d.node)?;
    let (_, _, elem) = bind_slots(prog, &u.root, &at).into_iter().find(|(id, ..)| *id == d.bind)?;
    Some((at, elem.map(|(_, p)| p)))
}

/// Whether the shared entity `c` would grant the binding `d` its take now
/// (appendix S.2.1).
pub fn would_grant(prog: &Program, c: &Unit, d: &Decide, now: i64) -> bool {
    let take = Msg::Take { from: d.label.clone(), key: d.key(), taken: d.taken.clone() };
    !step_at(prog, c, &take, now).sends.iter().any(|(_, m)| matches!(m, Msg::Refused { .. }))
}

/// The options of a binding's choice (appendix J.2.4): of the candidates, in
/// creation order, the first `candidates` that meet the binding's filter,
/// best first under its `prefer`. With `grants`, only those it says would
/// grant the take count; it is asked only of those that meet the filter.
pub fn options(prog: &Program, u: &Unit, d: &Decide, candidates: impl Iterator<Item = Unit>, grants: Option<&dyn Fn(&Unit) -> bool>) -> Vec<Unit> {
    let Some(b) = prog.bindings.get(d.binding as usize) else { return vec![] };
    let Some((at, elem)) = decide_place(prog, u, d) else { return vec![] };
    let mut gathered = vec![];
    for c in candidates {
        if gathered.len() >= prog.candidates as usize {
            break;
        }
        let mut facts = u.facts.clone();
        facts.extend(own_facts(prog, &c));
        let v = View { facts: &facts, cand: Some(&c.label), elem: elem.as_deref(), ..View::of(u) };
        if b.filter.iter().all(|f| eval(prog, v, &at, f) == T::Yes) && grants.is_none_or(|g| g(&c)) {
            let scores = b.prefer.iter().map(|cr| score(prog, v, &at, cr)).collect::<Vec<_>>();
            let margins = b.prefer.iter().map(|cr| match cr {
                Criterion::Near { within, .. } => match num(prog, v, &at, within) {
                    Some(Val::Money(x) | Val::Count(x) | Val::Time(x) | Val::Duration(x)) => Some(i128::from(x).abs()),
                    _ => None,
                },
                _ => None,
            }).collect::<Vec<_>>();
            gathered.push((c, scores, margins));
        }
    }
    // `within`: those near the best of their tier, the candidates tied on
    // every criterion before it, rank as that best (appendix BN.2.3); the
    // margin is read at that best candidate.
    for (j, cr) in b.prefer.iter().enumerate() {
        if !matches!(cr, Criterion::Near { .. }) {
            continue;
        }
        let mut best: std::collections::BTreeMap<Vec<(u8, i128)>, (i128, i128)> = Default::default();
        for g in gathered.iter().filter(|g| g.1[j].0 == 0) {
            let (s, m) = (g.1[j].1, g.2[j].unwrap_or(0));
            best.entry(g.1[..j].to_vec()).and_modify(|e| if s < e.0 { *e = (s, m) }).or_insert((s, m));
        }
        for g in gathered.iter_mut().filter(|g| g.1[j].0 == 0) {
            if let Some(&(top, margin)) = best.get(&g.1[..j]) && g.1[j].1 <= top + margin {
                g.1[j].1 = top;
            }
        }
    }
    let gathered: Vec<_> = gathered.into_iter().map(|(c, s, _)| (c, s)).collect();
    let (mut slots, scores): (Vec<_>, Vec<_>) = gathered.into_iter().map(|(c, s)| (Some(c), s)).unzip();
    order(&scores).into_iter().filter_map(|i| slots[i].take()).collect()
}

/// The `decide` that handles a binding's choice, if any (appendix J.2.3):
/// of those that apply, the one that names the choice point, then the one
/// scoped nearest the choosing part, then the one with a `whose` that holds.
pub fn handler<'p>(prog: &'p Program, u: &Unit, d: &Decide) -> Option<&'p Decision> {
    let b = prog.bindings.get(d.binding as usize).filter(|b| !b.key_attr.is_empty())?;
    let (at, _) = decide_place(prog, u, d)?;
    handler_at(prog, u, &at, &b.target)
}

/// The `decide` that handles the choice point `point` (a binding's target, a
/// slot, or `Order's credit`) made by the node at `at` (appendix K.2.1).
pub fn handler_at<'p>(prog: &'p Program, u: &Unit, at: &[usize], point: &str) -> Option<&'p Decision> {
    // The choosing part, its owner, …, the root.
    let chain: Vec<Vec<usize>> = (0..=at.len()).rev().map(|n| at[..n].to_vec()).collect();
    let v = View::of(u);
    prog.decisions
        .iter()
        .filter(|x| x.point.as_ref().is_none_or(|p| p == point))
        .filter_map(|x| {
            let (near, pos) = match &x.scope {
                None => (0, vec![]),
                Some(k) => {
                    let i = chain.iter().position(|pos| node_at(&u.root, pos).kind == *k)?;
                    (chain.len() - i, chain[i].clone())
                }
            };
            if x.cond.as_ref().is_some_and(|c| eval(prog, v, &pos, c) != T::Yes) {
                return None;
            }
            Some(((x.point.is_some(), near, x.cond.is_some()), x))
        })
        .max_by_key(|(rank, _)| *rank)
        .map(|(_, x)| x)
}

/// A candidate's rank on one criterion; lower comes first, and an unknown value last.
fn score(prog: &Program, v: View, at: &[usize], c: &Criterion) -> (u8, i128) {
    let amount = |n: &Num| match num(prog, v, at, n) {
        Some(Val::Money(x) | Val::Count(x) | Val::Time(x) | Val::Duration(x)) => Some(i128::from(x)),
        _ => None,
    };
    match c {
        Criterion::Lowest(n) => amount(n).map_or((1, 0), |x| (0, x)),
        Criterion::Highest(n) | Criterion::Near { highest: true, n, .. } => amount(n).map_or((1, 0), |x| (0, -x)),
        Criterion::Near { n, .. } => amount(n).map_or((1, 0), |x| (0, x)),
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
            Some((_, ty)) if *ty == v.ty() => {
                if let Some(Err(why)) = prog.kind(&kind).and_then(|k| k.attr(name)).map(|a| state_value(&kind, a, v)) {
                    return Err(refuse(why));
                }
            }
            Some((_, ty)) => return Err(refuse(format!("{name} must be {ty:?}, not {}", shown(prog, name, v)))),
            None => return Err(refuse(format!("a {kind} answer has no field {name}"))),
        }
    }
    if success
        && let Some((name, _)) = effect.data.iter().find(|(n, _)| !data.iter().any(|(d, _)| d == n)) {
        return Err(refuse(format!("the answer is missing {name}")));
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
    let shown: Vec<String> = data.iter().map(|(k, v)| format!("{k} {}", shown(prog, k, v))).collect();
    let mut notes =
        vec![if shown.is_empty() { format!("{kind} {outcome}") } else { format!("{kind} {outcome}: {}", shown.join(", ")) }];
    react(prog, &mut t, &u.facts, &at, &kind, outcome, None, &mut notes);
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
    react(prog, &mut t, &u.facts, &at, &kind, TIMED_OUT, None, &mut notes);
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
    if u.met.contains(&goal) || u.alerts.contains(&goal) || goal_nodes(prog, u, g).is_empty() {
        return;
    }
    u.alerts.push(goal);
    u.notes.push(format!("alert: not met within {}: {}", duration(g.within_secs.unwrap_or(0)), g.source));
    for at in positions(&u.root) {
        let n = node_at(&u.root, &at);
        for attr in n.fetched.iter().filter(|(_, f)| f.failed).map(|(a, _)| a) {
            let adapters: Vec<&str> = prog.sources_of(&n.kind, attr).iter().map(|s| s.adapter.as_str()).collect();
            let adapter = if adapters.is_empty() { "its source".to_string() } else { adapters.join(" or ") };
            u.notes.push(format!("  because {attr} could not be sourced from {adapter}"));
        }
        for (part, rule) in &n.held {
            u.notes.push(format!("  because {part} waits: rule {rule}"));
        }
        for (id, b) in &n.binds {
            if matches!(b, Bind::Waiting { .. }) {
                u.notes.push(format!("  because {} waits for {}", n.kind, wanted(id)));
            }
        }
        for line in unasked_lines(prog, u, &at) {
            u.notes.push(format!("  because {} {line}", n.kind));
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
        .find(|(n, _)| secs >= *n && secs.is_multiple_of(*n))
        .map_or((secs, "second"), |(n, unit)| (secs / n, unit));
    format!("{n} {unit}{}", if n == 1 { "" } else { "s" })
}

/// Applies the `when` reactions to an outcome (spec §3.3).
/// An admitted event's caller: its role, its unit if the role has a kind,
/// and the data the event carries.
type Sent<'a> = (&'a str, Option<&'a str>, &'a [(String, Val)]);

/// The caller's unit and the data, for an event's actions (appendix Q.2.3).
type Given<'a> = (Option<&'a str>, &'a [(String, Val)]);

/// Applies the reactions to an outcome, or with the caller's `role` to an
/// admitted event: only those that name that role (appendix G.2.4).
#[allow(clippy::too_many_arguments)]
fn react(prog: &Program, root: &mut Node, facts: &Facts, at: &[usize], kind: &str, outcome: &str, event: Option<Sent>, notes: &mut Vec<String>) {
    let fits = |r: &&suture_lang::ir::Reaction| match event {
        None => r.by.is_empty(),
        Some((x, ..)) => r.by.iter().any(|b| b == x),
    };
    for r in prog.reactions.iter().filter(|r| r.kind == kind && r.outcome == outcome).filter(fits) {
        apply(prog, root, facts, at, &r.actions, event.map(|(_, c, d)| (c, d)), notes);
    }
}

/// Applies actions at the node at `at`: a reaction's, or a source's
/// `otherwise` (appendix I.2.6).
fn apply(prog: &Program, root: &mut Node, facts: &Facts, at: &[usize], actions: &[Action], event: Option<Given>, notes: &mut Vec<String>) {
    let (caller, data) = event.unwrap_or((None, &[]));
    {
        for a in actions {
            let Some(target) = here(View::bare(root, facts), at, &a.target.steps) else { continue };
            let Some(calc) = &a.becomes else {
                let n = node_at_mut(root, &target);
                n.attrs.insert(a.attr.clone(), Val::Bool(a.value));
                notes.push(format!("{} is {}{}", n.kind, if a.value { "" } else { "not " }, a.attr));
                continue;
            };
            let value = match num(prog, View { caller, data, ..View::bare(root, facts) }, at, calc) {
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
            if let Some(Err(why)) = prog.kind(&n.kind).and_then(|k| k.attr(&a.attr)).map(|x| state_value(&n.kind, x, &value)) {
                notes.push(format!("{} not set: {why}", a.attr));
                continue;
            }
            notes.push(format!("{}'s {} becomes {}", n.kind, a.attr, shown(prog, &a.attr, &value)));
            n.attrs.insert(a.attr.clone(), value);
        }
    }
}

fn settle(prog: &Program, u: &mut Unit, out: &mut Out, now: i64) {
    for _ in 0..SETTLE_ROUNDS {
        let changed = positions(&u.root).iter().any(|at| {
            needs_pass(prog, u, at, now, out)
                || bind_pass(prog, u, at, out)
                || held_pass(prog, u, at, out)
                || regroup_pass(prog, u, at, out)
                || done_pass(prog, u, at, now, out)
                || request_pass(prog, u, at, now, out)
                || sources_pass(prog, u, at, now, out)
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
        Add(Box<Node>, String),
        Remove(usize, String),
        Hold(String, String),
        Release(String),
        Move(String, String),
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
                    // A model is choosing its alternative: nothing is created meanwhile.
                    None if active && choosing(n, &key, &s.name) => continue,
                    None if active => {
                        if !options_of(n, s).is_empty() {
                            change = Some(Change::Move(s.name.clone(), String::new()));
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
                    if let Some(s) = slot.filter(|s| !options_of(n, s).is_empty()) {
                        change = Some(Change::Move(s.name.clone(), format!("{want} failed, ")));
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
                let next = slot.filter(|s| !options_of(n, s).is_empty());
                change = match newly_broken(prog, View::of(u).acting(now), &t) {
                    // Refused on facts that never change: move on at once (appendix F.2.15),
                    // once they are known (appendix AG.2.3).
                    Some(r) if r.fixed && next.is_some() && surely_broken(prog, View::of(u).with(&t), r) => {
                        let s = next.expect("checked above");
                        Some(Change::Move(s.name.clone(), format!("{want} refused by rule {}, ", r.name)))
                    }
                    Some(r) if n.held.get(&want) == Some(&r.name) => None,
                    Some(r) => Some(Change::Hold(want.clone(), r.name.clone())),
                    None => Some(Change::Add(Box::new(part), note)),
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
            n.parts.push(*part);
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
        Some(Change::Move(slot, why)) => move_slot(prog, u, at, &slot, &why, out),
        Some(Change::Unfill(slot)) => {
            n.slots.remove(&slot);
            n.tried.remove(&slot);
            let open = format!("{key}?{slot}#");
            n.choosing.retain(|k, _| !k.starts_with(&open));
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
pub fn patience_due<'p>(prog: &'p Program, unit: &Unit, id: &str) -> Option<(Vec<usize>, &'p str, String)> {
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
    options_of(n, s).first()?;
    Some((at, s.name.as_str(), alt.to_string()))
}

/// Moves a slot on when its patience ran out: an untouched part is withdrawn.
fn patience(prog: &Program, u: &mut Unit, id: &str, out: &mut Out) -> bool {
    let Some((at, slot, alt)) = patience_due(prog, u, id) else { return false };
    let secs = prog.slot(slot).and_then(|s| s.patience[s.alternatives.iter().position(|a| *a == alt)?]).unwrap_or(0);
    if let Some(i) = current(node_at(&u.root, &at), &alt) {
        give_back(&u.label, &u.root, &child(&at, i), out);
    }
    let n = node_at_mut(&mut u.root, &at);
    if let Some(i) = current(n, &alt) {
        n.parts.remove(i);
    }
    n.held.remove(&alt);
    move_slot(prog, u, &at, slot, &format!("no {alt} after {}, ", duration(secs)), out)
}

/// The alternatives a slot may still move to, in `prefer` order: those not
/// tried since it was last filled afresh (appendix K.2.3).
fn options_of(n: &Node, s: &Slot) -> Vec<String> {
    let tried = n.tried.get(&s.name).map(Vec::as_slice).unwrap_or_default();
    s.alternatives.iter().filter(|a| !tried.contains(a)).cloned().collect()
}

/// Whether a model is choosing the slot of the node keyed `key`.
fn choosing(n: &Node, key: &str, slot: &str) -> bool {
    let open = format!("{key}?{slot}#");
    n.choosing.keys().any(|k| k.starts_with(&open))
}

/// Fills a slot of the node at `at` with its next alternative, or puts the
/// choice to the model that handles it when there is more than one
/// (appendix K.2.2–K.2.3). `why` starts its note.
fn move_slot(prog: &Program, u: &mut Unit, at: &[usize], slot: &str, why: &str, out: &mut Out) -> bool {
    let Some(s) = prog.slot(slot) else { return false };
    let options = options_of(node_at(&u.root, at), s);
    if options.is_empty() {
        return false;
    }
    let model = handler_at(prog, u, at, slot).and_then(|h| asked_by(prog, h));
    match model {
        Some((model, within, route)) if options.len() > 1 => {
            let key = node_key(&u.label, &u.root, at);
            let n = node_at_mut(&mut u.root, at);
            let ask_key = format!("{key}?{slot}#{}", n.tried.get(slot).map_or(0, Vec::len) + 1);
            n.slots.remove(slot);
            n.choosing.insert(ask_key.clone(), options);
            u.notes.push(format!("{slot}: {why}asking model \"{model}\""));
            out.asks.push(model_ask(&u.label, &model, slot, ask_key, within, route));
        }
        _ => fill(prog, u, at, slot, &options[0], why),
    }
    true
}

fn fill(prog: &Program, u: &mut Unit, at: &[usize], slot: &str, alt: &str, why: &str) {
    let key = node_key(&u.label, &u.root, at);
    let n = node_at_mut(&mut u.root, at);
    n.slots.insert(slot.to_string(), alt.to_string());
    n.tried.entry(slot.to_string()).or_default().push(alt.to_string());
    u.notes.push(format!("{slot}: {why}trying {alt}"));
    ask_patience(prog, u, &key, slot, alt);
}

/// The model a choice `h` handles is put to first, its deadline, and, for a
/// routed or bounded choice, the decision's index, by which the host finds
/// its candidates and bounds (appendix AJ.2.2, AK.2.2). `None` for `prefer`.
fn asked_by(prog: &Program, h: &suture_lang::ir::Decision) -> Option<(String, u64, Option<usize>)> {
    let named = h.route.is_some() || h.sure.is_some() || h.otherwise.is_some();
    let route = named.then(|| prog.decisions.iter().position(|d| std::ptr::eq(d, h))).flatten();
    Some((h.asked()?.to_string(), h.within_ms, route))
}

/// A choice put to a model (appendix K.2.3): the host asks it at once, within
/// its deadline, and answers with the pick, or with `prefer`'s. A routed one
/// names its decision (appendix AJ.2.2).
fn model_ask(label: &str, model: &str, point: &str, key: String, within_ms: u64, route: Option<usize>) -> Ask {
    let mut sends = vec![("within".into(), Val::Count(within_ms as i64))];
    if let Some(i) = route {
        sends.push(("decision".into(), Val::Count(i as i64)));
    }
    Ask { adapter: model.to_string(), verb: MODEL_VERB.into(), kind: point.to_string(), label: label.to_string(), key, sends }
}

/// The verb of a request to a model.
pub const MODEL_VERB: &str = "decide";

/// The verb of a fetch a model answers by reading the value (appendix AF).
pub const READ_VERB: &str = "read";

/// Whether a request key is a choice put to a model: `O1?Delivery#1`, `O1~credit#2?source`.
pub fn is_choice(key: &str) -> bool {
    key.contains('?')
}

/// A choice put to a model, as its unit holds it: the kind of the node that
/// chooses, its attributes, and the options in `prefer` order.
pub type Pending = (String, Vec<(String, Val)>, Vec<String>);

/// The choice put to a model under `key`; see [`Pending`].
pub fn choice_of(u: &Unit, key: &str) -> Option<Pending> {
    let at = positions(&u.root).into_iter().find(|at| node_at(&u.root, at).choosing.contains_key(key))?;
    let n = node_at(&u.root, &at);
    Some((n.kind.clone(), n.attrs.clone().into_iter().collect(), n.choosing[key].clone()))
}

/// A model's answer, or `prefer`'s in its place, as the host injected it:
/// the pick fills the slot, or names the source to ask. A pick that is not
/// among the options takes the first.
fn chosen(prog: &Program, u: &mut Unit, key: &str, data: &[(String, Val)], out: &mut Out) -> Result<(), String> {
    let Some(at) = positions(&u.root).into_iter().find(|at| node_at(&u.root, at).choosing.contains_key(key)) else {
        return Err(format!("ignored a pick for {key}: nothing is being chosen"));
    };
    let options = node_at_mut(&mut u.root, &at).choosing.remove(key).expect("found above");
    let pick = match data {
        [(name, Val::Text(p))] if name == "pick" && options.contains(p) => p.clone(),
        _ => options[0].clone(),
    };
    if let Some(fetch) = key.strip_suffix("?source") {
        let attr = fetch.rsplit_once('~').and_then(|(_, r)| r.rsplit_once('#')).map(|(a, _)| a.to_string()).unwrap_or_default();
        ask_source(prog, u, &at, &attr, &pick, "", out);
        return Ok(());
    }
    let slot = key.rsplit_once('?').and_then(|(_, r)| r.rsplit_once('#')).map(|(s, _)| s.to_string()).unwrap_or_default();
    fill(prog, u, &at, &slot, &pick, "");
    Ok(())
}

/// A part with no effect of its own is done once every active need of it is
/// done (appendix B.3.2), unless being done would break a rule: then it waits
/// on that rule. A root is never done; its goals describe it.
fn done_pass(prog: &Program, u: &mut Unit, at: &[usize], now: i64, out: &mut Out) -> bool {
    let n = node_at(&u.root, at);
    if at.is_empty() || n.status != Status::Idle || prog.effect_of(&n.kind).is_some() || !needs_done(prog, View::of(u), at) || !bound(prog, u, at) {
        return false;
    }
    let (kind, waiting) = (n.kind.clone(), n.waiting.clone());
    let mut t = u.root.clone();
    node_at_mut(&mut t, at).status = Status::Done;
    if refused_for_good(prog, u, at, &t, now, out) {
        return true;
    }
    match newly_broken(prog, View::of(u).acting(now), &t) {
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

/// When going ahead with the part at `at` (as `after`) newly breaks a rule on
/// facts that never change, and the part is its slot's current alternative
/// and nothing of it went out,
/// the part is withdrawn and the slot moves on, as it does when such a rule
/// refuses the part itself (appendix Y.2.1). Returns whether it moved on.
fn refused_for_good(prog: &Program, u: &mut Unit, at: &[usize], after: &Node, now: i64, out: &mut Out) -> bool {
    let Some((_, owner)) = at.split_last() else { return false };
    let before = View::of(u).acting(now);
    let known = |r: &Rule| surely_broken(prog, View::of(u).with(after), r);
    let Some(r) = prog.rules.iter().find(|r| r.fixed && broken(prog, before.with(after), r) && !broken(prog, before, r) && known(r)) else { return false };
    let o = node_at(&u.root, owner);
    let kind = node_at(&u.root, at).kind.clone();
    let Some(s) = prog.slots.iter().find(|s| s.owner == o.kind && o.slots.get(&s.name) == Some(&kind)) else { return false };
    // One whose own parts went out is never given up, as for patience.
    if options_of(o, s).is_empty() || current(o, &kind).map(|i| child(owner, i)).as_deref() != Some(at) || !untouched(node_at(&u.root, at)) {
        return false;
    }
    let (slot, rule) = (s.name.clone(), r.name.clone());
    give_back(&u.label, &u.root, at, out);
    let i = *at.last().expect("a part");
    let o = node_at_mut(&mut u.root, owner);
    o.parts.remove(i);
    o.held.remove(&kind);
    move_slot(prog, u, owner, &slot, &format!("{kind} refused by rule {rule}, "), out)
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
fn request_pass(prog: &Program, u: &mut Unit, at: &[usize], now: i64, out: &mut Out) -> bool {
    let n = node_at(&u.root, at);
    let Some(effect) = prog.effect_of(&n.kind) else { return false };
    if n.status != Status::Idle || !needs_done(prog, View::of(u), at) || !bound(prog, u, at) {
        return false;
    }
    let (kind, waiting) = (n.kind.clone(), n.waiting.clone());
    let mut t = u.root.clone();
    node_at_mut(&mut t, at).status = Status::Requested;
    if refused_for_good(prog, u, at, &t, now, out) {
        return true;
    }
    match broken_rule(prog, View::of(u).with(&t).acting(now)) {
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
            let sends: Vec<String> = a.sends.iter().map(|(k, v)| format!("{k} {}", shown(prog, k, v))).collect();
            u.notes.push(format!("{kind} → {} via {} ({})", effect.verb, effect.adapter, sends.join(", ")));
            out.asks.push(a);
            true
        }
    }
}

/// Whether a request key is a source's fetch: `O1~credit#2` (appendix I.2.2).
pub fn is_fetch(key: &str) -> bool {
    key.contains('~')
}

/// `O1~credit#2`, `O1/Check#1~score#1`: the node, the attribute and the fetch's number.
fn fetch_key(label: &str, root: &Node, at: &[usize], attr: &str, n: u32) -> String {
    format!("{}~{attr}#{n}", node_key(label, root, at))
}

/// The source the open or last fetch of `attr` asked: the first, before any.
fn via<'p>(prog: &'p Program, kind: &str, attr: &str, f: &Fetch) -> &'p Source {
    let all = prog.sources_of(kind, attr);
    let first = prog.source_of(kind, attr).expect("a sourced attribute has a source");
    all.into_iter().find(|s| s.adapter == f.via).unwrap_or(first)
}

/// Starts the fetch of `attr` at `at` with a source not yet tried in it
/// (appendix K.2.4): the only one left, the first under `prefer`, or the
/// one its model picks. `why` starts its note. False if none is left.
fn pick_source(prog: &Program, u: &mut Unit, at: &[usize], attr: &str, why: &str, out: &mut Out) -> bool {
    let n = node_at(&u.root, at);
    let kind = n.kind.clone();
    let f = n.fetched.get(attr).cloned().unwrap_or_default();
    let options: Vec<String> = prog.sources_of(&kind, attr).into_iter().map(|s| s.adapter.clone()).filter(|a| !f.tried.contains(a)).collect();
    if options.is_empty() {
        return false;
    }
    let point = format!("{kind}'s {attr}");
    let model = handler_at(prog, u, at, &point).and_then(|h| asked_by(prog, h));
    match model {
        Some((model, within, route)) if options.len() > 1 => {
            let key = format!("{}?source", fetch_key(&u.label, &u.root, at, attr, f.n));
            node_at_mut(&mut u.root, at).choosing.insert(key.clone(), options);
            u.notes.push(format!("{attr}: {why}asking model \"{model}\" for a source"));
            out.asks.push(model_ask(&u.label, &model, &point, key, within, route));
        }
        _ => ask_source(prog, u, at, attr, &options[0], why, out),
    }
    true
}

/// Sends the open fetch of `attr` at `at` to the source `adapter`.
fn ask_source(prog: &Program, u: &mut Unit, at: &[usize], attr: &str, adapter: &str, why: &str, out: &mut Out) {
    let kind = node_at(&u.root, at).kind.clone();
    let Some(src) = prog.sources_of(&kind, attr).into_iter().find(|s| s.adapter == adapter) else { return };
    let f = node_at_mut(&mut u.root, at).fetched.entry(attr.to_string()).or_default();
    let again = if f.at.is_some() && f.tried.is_empty() { " again" } else { "" };
    f.via = adapter.to_string();
    f.sends = 1;
    let a = fetch_ask(&u.label, View::of(u), at, src);
    let sends: Vec<String> = a.sends.iter().map(|(k, v)| format!("{k} {}", shown(prog, k, v))).collect();
    let who = if src.model { format!("model \"{adapter}\" to read it") } else { adapter.to_string() };
    u.notes.push(format!("{attr}: {why}asking {who}{again} ({})", sends.join(", ")));
    out.asks.push(a);
}

fn fetch_ask(label: &str, v: View, at: &[usize], src: &Source) -> Ask {
    let n = node_at(v.root, at).fetched.get(&src.attr).map_or(0, |f| f.n);
    let sends = src.sends.iter().filter_map(|(name, path)| value(v, at, path).map(|x| (name.clone(), x))).collect();
    Ask {
        adapter: src.adapter.clone(),
        verb: if src.model { READ_VERB.into() } else { "source".into() },
        kind: src.request_kind(),
        label: label.to_string(),
        key: fetch_key(label, v.root, at, &src.attr, n),
        sends,
    }
}

/// The node and attribute of the fetch numbered as `key` says, open or not.
fn find_fetch(u: &Unit, key: &str, open: bool) -> Option<(Vec<usize>, String)> {
    positions(&u.root).into_iter().find_map(|at| {
        let n = node_at(&u.root, &at);
        let hit = n.fetched.iter().find(|(a, f)| (!open || f.sends > 0) && fetch_key(&u.label, &u.root, &at, a, f.n) == key);
        hit.map(|(a, _)| (at.clone(), a.clone()))
    })
}

/// The bindings of the node at `at` that cannot be asked yet because their
/// amount reads a sourced value not in yet (appendix AG.2.2), as (id, index).
pub fn unasked(prog: &Program, u: &Unit, at: &[usize]) -> Vec<(String, usize)> {
    let n = node_at(&u.root, at);
    bind_slots(prog, &u.root, at)
        .into_iter()
        .filter(|(id, i, _)| !n.binds.contains_key(id) && !prog.bindings[*i].sources.is_empty())
        .filter(|(_, i, _)| match &prog.bindings[*i].held {
            Some(c) => matches!(eval(prog, View::of(u), at, c), T::Yes),
            None => n.status == Status::Idle,
        })
        .filter(|(id, ..)| decide_for(prog, u, at, id, vec![]).is_none())
        .map(|(id, i, _)| (id, i))
        .collect()
}

/// `waits for its amount, to count toward a Policy's total payout`, for each
/// binding at `at` that cannot be asked yet.
pub fn unasked_lines(prog: &Program, u: &Unit, at: &[usize]) -> Vec<String> {
    unasked(prog, u, at)
        .into_iter()
        .map(|(id, i)| {
            let attrs: Vec<&str> = prog.bindings[i].sources.iter().map(|(_, a)| a.as_str()).collect();
            format!("waits for its {}, to count toward {}", attrs.join(" and "), toward(&id))
        })
        .collect()
}

/// Fetches what a waiting part's strict rules need (appendix I.2.5): each
/// sourced value a rule it waits on reads that is not fresh for that rule
/// and has no open or failed fetch, and each one a binding's amount waits
/// for (appendix AG.2.2). Makes at most one change.
fn sources_pass(prog: &Program, u: &mut Unit, at: &[usize], now: i64, out: &mut Out) -> bool {
    for (id, i) in unasked(prog, u, at) {
        for (kind, attr) in &prog.bindings[i].sources {
            for pos in positions(&u.root) {
                let node = node_at(&u.root, &pos);
                if node.kind != *kind || node.status == Status::Failed || node.attrs.contains_key(attr) {
                    continue;
                }
                let f = node.fetched.get(attr).cloned().unwrap_or_default();
                let picking = format!("{}?source", fetch_key(&u.label, &u.root, &pos, attr, f.n));
                if f.sends > 0 || f.failed || node.choosing.contains_key(&picking) {
                    continue;
                }
                let fetch = node_at_mut(&mut u.root, &pos).fetched.entry(attr.clone()).or_default();
                fetch.n += 1;
                fetch.tried.clear();
                pick_source(prog, u, &pos, attr, &format!("for {}, ", toward(&id)), out);
                return true;
            }
        }
    }
    let n = node_at(&u.root, at);
    let mut waits: Vec<String> = n.held.values().cloned().collect();
    if n.status == Status::Idle {
        waits.extend(n.waiting.clone());
    }
    for name in waits {
        let Some(r) = prog.rules.iter().find(|r| r.name == name) else { continue };
        for (kind, attr) in &r.sources {
            if prog.source_of(kind, attr).is_none() {
                continue;
            }
            let bound = r.fresh.iter().find(|(a, _)| a == attr).map(|(_, secs)| *secs);
            for pos in positions(&u.root) {
                let node = node_at(&u.root, &pos);
                // A failed attempt is kept for the record; only live nodes fetch.
                if node.kind != *kind || node.status == Status::Failed {
                    continue;
                }
                let f = node.fetched.get(attr).cloned().unwrap_or_default();
                let picking = format!("{}?source", fetch_key(&u.label, &u.root, &pos, attr, f.n));
                if f.sends > 0 || f.failed || f.fresh(now, bound) || node.choosing.contains_key(&picking) {
                    continue;
                }
                let last = via(prog, kind, attr, &f);
                let why = match (f.at, bound) {
                    (None, _) => String::new(),
                    (Some(_), Some(b)) if f.until.is_none_or(|until| now < until) => format!("older than {} for rule {}: ", duration(b), r.name),
                    (Some(_), _) => format!("older than {}: ", duration(last.good_for.unwrap_or(0))),
                };
                let fetch = node_at_mut(&mut u.root, &pos).fetched.entry(attr.clone()).or_default();
                fetch.n += 1;
                fetch.tried.clear();
                pick_source(prog, u, &pos, attr, &why, out);
                return true;
            }
        }
    }
    false
}

/// Whether a live part of the unit waits on a rule that reads `kind`'s
/// `attr` (appendix W.2.1): what `sources_pass` fetches for.
fn needed(prog: &Program, u: &Unit, kind: &str, attr: &str) -> bool {
    positions(&u.root).into_iter().any(|at| {
        let n = node_at(&u.root, &at);
        let idle = n.status == Status::Idle;
        let waits = n.held.values().chain(n.waiting.iter().filter(|_| idle));
        n.status != Status::Failed && waits.into_iter().any(|name| prog.rules.iter().any(|r| r.name == *name && r.sources.iter().any(|(k, a)| k == kind && a == attr)))
            || unasked(prog, u, &at).into_iter().any(|(_, i)| prog.bindings[i].sources.iter().any(|(k, a)| k == kind && a == attr))
    })
}

/// Computes the formulas a new value lets the unit know (appendix AG.2.1).
fn recompute(prog: &Program, u: &mut Unit) {
    let all = positions(&u.root);
    while all.iter().map(|at| formulas(prog, &mut u.root, &u.facts, at)).filter(|changed| *changed).count() > 0 {}
}

/// A fetch's answer (appendix I.2.2): the value is recorded as fetched now,
/// or the fetch fails. A malformed answer is refused and asked again.
fn fetch_answer(prog: &Program, u: &mut Unit, key: &str, outcome: &str, data: &[(String, Val)], now: i64, out: &mut Out) -> Result<(), String> {
    let Some((at, attr)) = find_fetch(u, key, true) else { return Err(format!("ignored an answer for {key}: no open fetch")) };
    let n = node_at(&u.root, &at);
    let src = via(prog, &n.kind, &attr, &n.fetched[&attr]);
    let mut refuse = |why: String| {
        out.asks.push(fetch_ask(&u.label, View::of(u), &at, src));
        Err(format!("refused an answer for {key}: {why}"))
    };
    if outcome != src.success {
        if src.failures.iter().any(|f| f == outcome) {
            fail(prog, u, &at, src, key, outcome, now, out);
            return Ok(());
        }
        return refuse(format!("a source of {attr} reports `{}` or fails with {}, not `{outcome}`", src.success, src.failures.join(", ")));
    }
    let value = match data {
        [(name, v)] if *name == attr && v.ty() == src.ty => v.clone(),
        [(name, v)] if *name == attr => return refuse(format!("{attr} must be {:?}, not {}", src.ty, v.show(&prog.currency))),
        _ => return refuse(format!("a `{}` answer carries {attr} and nothing else", src.success)),
    };
    let shown = shown(prog, &attr, &value);
    let n = node_at_mut(&mut u.root, &at);
    n.attrs.insert(attr.clone(), value);
    let f = n.fetched.entry(attr.clone()).or_default();
    *f = Fetch { sends: 0, at: Some(now), until: src.good_for.map(|g| now + g as i64), failed: false, fallback: false, tried: vec![], ..f.clone() };
    recompute(prog, u);
    let from = if src.model { format!("read by model \"{}\"", src.adapter) } else { format!("from {}", src.adapter) };
    match src.good_for {
        Some(g) => {
            u.notes.push(format!("{attr}: {shown} {from}, trusted for {}", duration(g)));
            fetch_timer(u, key, "stale", g);
        }
        None => u.notes.push(format!("{attr}: {shown} {from}")),
    }
    Ok(())
}

/// Wants the timer `O1~credit#2:stale`. The timers of the attribute's earlier
/// fetches are already started and now fire for a fetch that is over, so
/// they are dropped: a value asked again for weeks keeps one.
fn fetch_timer(u: &mut Unit, key: &str, what: &str, secs: u64) {
    let fetches = key.rsplit_once('#').map_or(key, |(k, _)| k);
    u.timers.retain(|(id, _)| id.rsplit_once('#').is_none_or(|(k, _)| k != fetches) || !is_fetch(id));
    u.timers.push((format!("{key}:{what}"), secs));
}

/// A fetch's deadline: it is asked again with the same key, or after the
/// last send it fails.
fn fetch_deadline(prog: &Program, u: &mut Unit, key: &str, now: i64, out: &mut Out) -> Result<(), String> {
    let Some((at, attr)) = find_fetch(u, key, true) else { return Err(format!("ignored a deadline for {key}: no open fetch")) };
    let n = node_at(&u.root, &at);
    let src = via(prog, &n.kind, &attr, &n.fetched[&attr]);
    if !needed(prog, u, &n.kind, &attr) {
        // Nothing waits on it any more, say its part was withdrawn: no more
        // sends, and a late answer finds no open fetch.
        let f = node_at_mut(&mut u.root, &at).fetched.entry(attr.clone()).or_default();
        f.sends = 0;
        f.n += 1;
        u.notes.push(format!("{attr}: no step waits on it any more: stopped asking {}", src.adapter));
        return Ok(());
    }
    let f = node_at_mut(&mut u.root, &at).fetched.entry(attr.clone()).or_default();
    if f.sends < prog.attempts {
        f.sends += 1;
        let sends = f.sends;
        u.notes.push(format!("{attr}: no answer within {}: asking again (send {sends} of {})", duration(src.within_secs), prog.attempts));
        out.asks.push(fetch_ask(&u.label, View::of(u), &at, src));
        return Ok(());
    }
    fail(prog, u, &at, src, key, &format!("timed out after {} sends", prog.attempts), now, out);
    Ok(())
}

/// A fetch failed: it moves on to a source not yet tried (appendix K.2.4),
/// or, with none left, the attribute's `otherwise` applies (appendix I.2.6).
#[allow(clippy::too_many_arguments)]
fn fail(prog: &Program, u: &mut Unit, at: &[usize], src: &Source, key: &str, why: &str, now: i64, out: &mut Out) {
    let attr = &src.attr;
    let f = node_at_mut(&mut u.root, at).fetched.entry(attr.clone()).or_default();
    f.sends = 0;
    f.tried.push(src.adapter.clone());
    let tried = f.tried.clone();
    if prog.sources_of(&src.kind, attr).into_iter().any(|s| !tried.contains(&s.adapter)) {
        // A new fetch number, so a late answer from this source is ignored.
        f.n += 1;
        pick_source(prog, u, at, attr, &format!("{} failed ({why}), ", src.adapter), out);
        return;
    }
    f.failed = true;
    let failed = if src.model { format!("{attr}: model \"{}\" did not read it", src.adapter) } else { format!("{attr}: {} failed ({why})", src.adapter) };
    // The `otherwise`, and the `good for` of the source that gives it.
    let otherwise = prog.otherwise_of(&src.kind, attr);
    let src = prog.sources_of(&src.kind, attr).into_iter().find(|s| s.otherwise == *otherwise).unwrap_or(src);
    match otherwise {
        Otherwise::Hold => u.notes.push(format!("{failed}; the steps its rules guard wait")),
        Otherwise::Use(l) => {
            let v = Val::from_lit(l);
            u.notes.push(format!("{failed}: using {} instead", shown(prog, attr, &v)));
            let n = node_at_mut(&mut u.root, at);
            n.attrs.insert(attr.clone(), v);
            let f = n.fetched.entry(attr.clone()).or_default();
            *f = Fetch { sends: 0, at: Some(now), until: src.good_for.map(|g| now + g as i64), failed: false, fallback: true, tried: vec![], ..f.clone() };
            recompute(prog, u);
            if let Some(g) = src.good_for {
                fetch_timer(u, key, "stale", g);
            }
        }
        Otherwise::Again(secs) => {
            u.notes.push(format!("{failed}: asking again in {}", duration(*secs)));
            fetch_timer(u, key, "again", *secs);
        }
        Otherwise::Act(actions) => {
            u.notes.push(failed);
            let mut t = u.root.clone();
            let mut notes = vec![];
            apply(prog, &mut t, &u.facts, at, actions, None, &mut notes);
            match newly_broken(prog, View::of(u), &t) {
                Some(r) => {
                    // The recovery cannot apply, so the next waiting step fetches again.
                    u.notes.push(format!("reaction held: rule {}: {}", r.name, r.paraphrase));
                    if let Some(f) = node_at_mut(&mut u.root, at).fetched.get_mut(attr) {
                        f.failed = false;
                    }
                }
                None => {
                    u.root = t;
                    u.notes.extend(notes);
                }
            }
        }
    }
}

/// Whether a source's timer still has a reason: the fetch it names is still
/// the latest, and for `:again` it is still failed. The host skips the rest.
pub fn fetch_due_matters(u: &Unit, id: &str) -> bool {
    let (key, again) = match (id.strip_suffix(":again"), id.strip_suffix(":stale")) {
        (Some(k), _) => (k, true),
        (_, Some(k)) => (k, false),
        _ => return false,
    };
    find_fetch(u, key, false).is_some_and(|(at, attr)| node_at(&u.root, &at).fetched.get(&attr).is_some_and(|f| f.sends == 0 && f.failed == again))
}

/// What marks a timer for a time that passes: `L1!passes:due@1767225600`.
pub const PASS_MARK: &str = "!passes:";

/// Arms the times of the unit that pass (appendix AQ.2.2–AQ.2.3): each one
/// already past passes now; each later one is a timer. With `only`, just
/// that attribute, which a change has just set.
fn passing(prog: &Program, u: &mut Unit, only: Option<&str>, now: i64) {
    let kind = u.root.kind.clone();
    for r in prog.reactions.iter().filter(|r| r.kind == kind) {
        let Some(attr) = r.outcome.strip_suffix(PASSES) else { continue };
        if only.is_some_and(|o| o != attr) {
            continue;
        }
        let Some(Val::Time(t)) = u.root.attrs.get(attr).cloned() else { continue };
        if t <= now {
            u.notes.push(format!("its {attr} has passed ({})", time_text(t)));
            pass(prog, u, &r.actions);
        } else {
            let id = format!("{}{PASS_MARK}{attr}@{t}", u.label);
            if !u.timers.iter().any(|(x, _)| *x == id) {
                u.timers.push((id, (t - now) as u64));
            }
        }
    }
}

/// Whether the timer `id` still stands for a time that passes: the
/// attribute holds the time it was armed for (appendix AQ.2.3).
pub fn pass_matters(u: &Unit, id: &str) -> Option<(String, i64)> {
    let (_, rest) = id.split_once(PASS_MARK)?;
    let (attr, t) = rest.rsplit_once('@')?;
    let t: i64 = t.parse().ok()?;
    (u.root.attrs.get(attr) == Some(&Val::Time(t))).then(|| (attr.to_string(), t))
}

/// A time passed: the reactions to it apply at the root.
fn passed(prog: &Program, u: &mut Unit, id: &str) {
    let Some((attr, t)) = pass_matters(u, id) else { return };
    u.notes.push(format!("its {attr} passes ({})", time_text(t)));
    let outcome = format!("{attr}{PASSES}");
    let kind = u.root.kind.clone();
    for r in prog.reactions.iter().filter(|r| r.kind == kind && r.outcome == outcome) {
        pass(prog, u, &r.actions);
    }
}

/// A time passing is a fact, as a time-out is: it is always recorded, and a
/// reaction that would break a rule is held back (appendix AQ.2.1).
fn pass(prog: &Program, u: &mut Unit, actions: &[Action]) {
    let mut t = u.root.clone();
    let mut notes = vec![];
    apply(prog, &mut t, &u.facts, &[], actions, None, &mut notes);
    match newly_broken(prog, View::of(u), &t) {
        Some(r) => u.notes.push(format!("reaction held: rule {}: {}", r.name, r.paraphrase)),
        None => {
            u.root = t;
            u.notes.extend(notes);
        }
    }
}

/// A source's timer (appendix I.2.5–I.2.6): after `ask again after`, the
/// failed fetch may fetch again. Either way the step settles, so a part that
/// still waits fetches what it needs.
fn fetch_due(u: &mut Unit, id: &str) {
    let Some(key) = id.strip_suffix(":again") else { return };
    if let Some((at, attr)) = find_fetch(u, key, false)
        && let Some(f) = node_at_mut(&mut u.root, &at).fetched.get_mut(&attr) {
        f.failed = false;
    }
}

/// Records each goal with a deadline the first time the root matches its
/// subject, so the host starts its timer (appendix C.3.4), and notes each
/// goal the first time it is met: late, if it already raised an alert.
/// The nodes a goal is about in this unit: the root, or for a goal about a
/// part kind, the current attempt of each such part, whose filter holds
/// (appendix R.2.1).
pub fn goal_nodes(prog: &Program, u: &Unit, g: &suture_lang::ir::Goal) -> Vec<Vec<usize>> {
    positions(&u.root)
        .into_iter()
        .filter(|at| {
            let n = node_at(&u.root, at);
            n.kind == g.kind && live(&u.root, at)
        })
        .filter(|at| g.filter.iter().all(|f| eval(prog, View::of(u), at, f) == T::Yes))
        .collect()
}

/// Whether a goal applies to the unit: some node it is about is in it.
pub fn goal_applies(prog: &Program, u: &Unit, goal: u32) -> bool {
    prog.goals.get(goal as usize).is_some_and(|g| !goal_nodes(prog, u, g).is_empty())
}

fn goals(prog: &Program, u: &mut Unit) {
    for (i, g) in prog.goals.iter().enumerate() {
        let i = i as u32;
        let nodes = goal_nodes(prog, u, g);
        let applies = !nodes.is_empty();
        if applies && g.within_secs.is_some() && !u.matched.contains(&i) {
            u.matched.push(i);
        }
        if !u.met.contains(&i) && applies && nodes.iter().all(|at| g.states.iter().all(|s| state_at(prog, View::of(u), at, s))) {
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
    if let Some(a) = prog.kind(&n.kind).and_then(|k| k.state_attr(state)) {
        return matches!(n.attrs.get(&a.name), Some(Val::Text(x)) if x == state);
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

/// A value as the trace and explanations show it: a secret as `(secret)`,
/// and the value of an attribute some kind marks personal as `(personal)`
/// (appendix N.2.5). Marks are matched by the attribute's name, so this
/// masks more than it needs to, never less.
/// Whether `n` reads a personal attribute.
fn is_personal(prog: &Program, n: &Num) -> bool {
    matches!(n, Num::Path(p) if matches!(p.steps.last(), Some(Step::Attr(a)) if prog.kinds.iter().any(|k| k.attr(a).is_some_and(|x| x.mark == Some(Mark::Personal)))))
}

pub fn shown(prog: &Program, attr: &str, v: &Val) -> String {
    let personal = prog.kinds.iter().any(|k| k.attr(attr).is_some_and(|a| a.mark == Some(suture_lang::ir::Mark::Personal)));
    if personal && !matches!(v, Val::Secret(_)) { "(personal)".into() } else { v.show(&prog.currency) }
}


/// A rule `after` breaks that `before` did not.
pub fn newly_violated<'p>(prog: &'p Program, before: &Unit, after: &Unit) -> Option<&'p Rule> {
    prog.rules.iter().find(|r| broken(prog, View::of(after), r) && !broken(prog, View::of(before), r))
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

/// Whether a rule is broken. A strict rule (one that reads sourced facts),
/// checked before an action, counts what it cannot know against the action
/// (appendix I.2.4): a filter that might hold applies, an `every` that might
/// fail fails, and a `no … has` that might be present is present.
fn broken(prog: &Program, v: View, r: &Rule) -> bool {
    if !applies_to(r, v.root, v.version) {
        return false;
    }
    let strict = v.now.is_some() && !r.sources.is_empty();
    let v = View { bounds: &r.fresh, ..v };
    let may = |t: T| t == T::Yes || (strict && t == T::Unknown);
    positions(v.root).iter().any(|at| {
        node_at(v.root, at).kind == r.subject
            && r.filter.iter().all(|f| may(eval(prog, v, at, f)))
            && match &r.body {
                Body::Every(c) => {
                    let t = eval(prog, v, at, c);
                    t == T::No || (strict && t == T::Unknown)
                }
                Body::No { pred, unless } => may(eval(prog, v, at, pred)) && unless.as_ref().is_none_or(|u| eval(prog, v, at, u) != T::Yes),
            }
    })
}

/// Whether a rule is broken on what is known: nothing unknown counts against
/// it, so a fixed rule refuses for good only once its facts are in
/// (appendix AG.2.3).
fn surely_broken(prog: &Program, v: View, r: &Rule) -> bool {
    if !applies_to(r, v.root, v.version) {
        return false;
    }
    positions(v.root).iter().any(|at| {
        node_at(v.root, at).kind == r.subject
            && r.filter.iter().all(|f| eval(prog, v, at, f) == T::Yes)
            && match &r.body {
                Body::Every(c) => eval(prog, v, at, c) == T::No,
                Body::No { pred, unless } => eval(prog, v, at, pred) == T::Yes && unless.as_ref().is_none_or(|u| eval(prog, v, at, u) == T::No),
            }
    })
}

/// Whether a rule holds on a unit at all: `apply rule R to Ks created from
/// version n` leaves out the units of root kind K created before n
/// (appendix M.2.3).
pub fn applies_to(r: &Rule, root: &Node, version: Option<u32>) -> bool {
    match (&r.applies, version) {
        (Some(a), Some(v)) => root.kind != a.kind || v >= a.from,
        _ => true,
    }
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
                // A shared entity's only states are its Bool attributes and
                // the states of its state attributes (appendix F.2.3, Z.2.2).
                Some(Place::There(l)) => v.facts.get(&l).is_some_and(|f| match prog.kind(&f.kind).and_then(|k| k.state_attr(state)) {
                    Some(a) => matches!(f.attrs.get(&a.name), Some(Val::Text(x)) if x == state),
                    None => f.attrs.get(state) == Some(&Val::Bool(true)),
                }),
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
        Num::Bin(l, Arith::Div, z) if matches!(l.as_ref(), Num::Bin(_, Arith::Mul, _)) => {
            let Num::Bin(x, _, y) = l.as_ref() else { unreachable!("matched above") };
            match (num(prog, v, at, x)?, num(prog, v, at, y)?, num(prog, v, at, z)?) {
                (Val::Money(a), Val::Money(b), Val::Money(c)) => share(a, b, c).map(Val::Money),
                (a, b, c) => arith(arith(a, Arith::Mul, b)?, Arith::Div, c),
            }
        }
        Num::Bin(x, Arith::Working(i), y) => match (num(prog, v, at, x)?, num(prog, v, at, y)?) {
            (Val::Time(a), Val::Time(b)) => Some(Val::Duration(working(prog.calendars.get(*i as usize)?, a, b))),
            _ => None,
        },
        Num::Bin(x, op, y) => arith(num(prog, v, at, x)?, *op, num(prog, v, at, y)?),
        Num::Flag(c) => match eval(prog, v, at, c) {
            T::Yes => Some(Val::Count(1)),
            T::No => Some(Val::Count(0)),
            T::Unknown => None,
        },
        Num::Sum { of, kind, calc, filter, ty } => {
            let mut total = zero(*ty);
            for pos in relation(prog, v, at, of, kind, filter.as_deref())? {
                total = arith(total, Arith::Add, num(prog, v, &pos, calc)?)?;
            }
            Some(total)
        }
        Num::Count { of, kind, filter } => Some(Val::Count(relation(prog, v, at, of, kind, filter.as_deref())?.len() as i64)),
        Num::Taken { of, name, run: true, .. } => match locate(v, at, &of.steps)? {
            Place::Here(p) if p.is_empty() => Some(Val::Count(run_through(v.taken, name, v.period?)?)),
            _ => None,
        },
        Num::Taken { of, name, ty, period, group, .. } => match locate(v, at, &of.steps)? {
            Place::Here(p) if p.is_empty() => {
                let name = match period {
                    Some(_) => format!("{name}@{}", v.period?),
                    None if *group => format!("{name}#{}", v.group?),
                    None => name.clone(),
                };
                Some(v.taken.get(&name).cloned().unwrap_or_else(|| zero(*ty)))
            }
            // Another unit's total, as carried and refreshed (appendix AA.2.3).
            Place::There(l) => v.facts.get(&l).map(|f| f.taken.get(name).cloned().unwrap_or_else(|| zero(*ty))),
            _ => None,
        },
    }
}

/// The group of a take for `no two … have the same …`, from the entity's
/// values: `"Bo"`, `Ann, 2026-03-01`. It never holds `@` or `#`, which
/// mark a total's period and group (appendix AP.2.2). A personal value is
/// kept as a hash of it, so traces and read-outs never show it
/// (appendix BB.2.5).
fn group_key(prog: &Program, vals: &[(Val, bool)]) -> String {
    let shown: Vec<String> = vals
        .iter()
        .map(|(x, personal)| match x {
            // Keyed, so the name cannot be checked against guesses (appendix BI.2.1).
            x if *personal => {
                let text = x.show(&prog.currency);
                let hash = match &prog.personal_key {
                    Some(k) => crate::media::hmac_sha256(&k.0, text.as_bytes()),
                    None => crate::media::sha256(text.as_bytes()),
                };
                format!("(personal {})", &hash[..16])
            }
            Val::Text(s) => format!("{s:?}"),
            x => x.show(&prog.currency),
        })
        .collect();
    shown.join(", ").replace('%', "%25").replace('@', "%40").replace('#', "%23")
}

/// How many days in a row, through the day `day` (`2026-01-05`), have a
/// running total `name` per day above nothing; none when that day's is
/// nothing (appendix AH.2.1). It looks at most a span's longest either way.
fn run_through(taken: &BTreeMap<String, Val>, name: &str, day: &str) -> Option<i64> {
    let start = suture_lang::sentence::iso_time(day)?;
    let held = |d: i64| taken.get(&format!("{name}@{}", period_key("day", start + d * 86_400))).is_some_and(|v| *v != Val::Count(0));
    if !held(0) {
        return Some(0);
    }
    let before = (1..SPAN_DAYS).take_while(|d| held(-d)).count() as i64;
    let after = (1..SPAN_DAYS).take_while(|d| held(*d)).count() as i64;
    Some(before + 1 + after)
}

/// `a * b / c` over amounts, in cents, rounded half away from zero
/// (appendix AB.2.1). A share of nothing is nothing, even of a zero `c`;
/// `None` when only `c` is zero, or the result overflows.
fn share(a: i64, b: i64, c: i64) -> Option<i64> {
    let (n, d) = (i128::from(a) * i128::from(b), i128::from(c));
    if n == 0 {
        return Some(0);
    }
    if d == 0 {
        return None;
    }
    let q = (2 * n.abs() + d.abs()) / (2 * d.abs());
    let q = if (n < 0) != (d < 0) { -q } else { q };
    i64::try_from(q).ok()
}

/// Nothing, as an amount of type `ty`.
pub(crate) fn zero(ty: Ty) -> Val {
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

/// Monday to Friday from day `a` to day `b` since 1970-01-01, both counted;
/// 0 when `b` is before `a` (appendix AW.2.1).
fn weekdays(a: i64, b: i64) -> i64 {
    // Days since a Monday up to day `d`, both counted, that are weekdays;
    // 1970-01-01 was a Thursday, so day 0 is 3 days after a Monday.
    let upto = |d: i64| {
        let n = d + 3; // day 0 of this count is a Monday
        let (weeks, rest) = (n.div_euclid(7), n.rem_euclid(7));
        weeks * 5 + (rest + 1).min(5)
    };
    if b < a { 0 } else { upto(b) - upto(a - 1) }
}

/// The time calendar `c` works in each period from `from` to `to`, in the
/// order `periods_in` gives them (appendix BM.2.1).
fn shares(c: &suture_lang::ir::Calendar, per: &str, from: i64, to: i64) -> Vec<i64> {
    const DAY: i64 = 86_400;
    let end = if to.rem_euclid(DAY) == 0 { to.saturating_add(DAY) } else { to };
    let mut out: Vec<(String, i64)> = vec![];
    for d in from.div_euclid(DAY)..=to.div_euclid(DAY) {
        let k = period_key(per, d * DAY);
        let share = between(c, from.max(d * DAY), end.min((d + 1) * DAY));
        match out.last_mut() {
            Some((last, s)) if *last == k => *s += share,
            _ => out.push((k, share)),
        }
    }
    out.into_iter().map(|(_, s)| s).collect()
}

/// The time calendar `c` works from `a` to `b`. An end at midnight takes
/// that whole day, as `the days from` counts it; none when `b` is before `a`
/// (appendix BL.2.2).
pub fn working(c: &suture_lang::ir::Calendar, a: i64, b: i64) -> i64 {
    const DAY: i64 = 86_400;
    between(c, a, if b.rem_euclid(DAY) == 0 { b.saturating_add(DAY) } else { b })
}

/// The time calendar `c` works from `a` up to, not including, `b`.
fn between(c: &suture_lang::ir::Calendar, a: i64, b: i64) -> i64 {
    const DAY: i64 = 86_400;
    if b <= a {
        return 0;
    }
    let off = |d: i64| c.holidays.binary_search(&d).is_ok();
    // Day 0 of the runtime clock, 2026-01-01, was a Thursday.
    let works = |d: i64| c.days[(d + 3).rem_euclid(7) as usize] && !off(d);
    let overlap = |d: i64| match works(d) {
        true => (b.min(d * DAY + c.end) - a.max(d * DAY + c.start)).max(0),
        false => 0,
    };
    let (first, last) = (a.div_euclid(DAY), (b - 1).div_euclid(DAY));
    if first == last {
        return overlap(first);
    }
    // The whole days between: whole weeks at once, then what is left, then
    // the days off that fall on working days.
    let (from, to) = (first + 1, last - 1);
    let mut whole = 0;
    if to >= from {
        let n = to - from + 1;
        whole = n / 7 * c.days.iter().filter(|d| **d).count() as i64;
        whole += (0..n % 7).filter(|k| c.days[(to - k + 3).rem_euclid(7) as usize]).count() as i64;
        let lo = c.holidays.partition_point(|h| *h < from);
        let hi = c.holidays.partition_point(|h| *h <= to);
        whole -= c.holidays[lo..hi].iter().filter(|d| c.days[(**d + 3).rem_euclid(7) as usize]).count() as i64;
    }
    overlap(first) + overlap(last) + whole * (c.end - c.start)
}

/// Checked arithmetic on amounts and counts; division rounds toward zero.
fn arith(x: Val, op: Arith, y: Val) -> Option<Val> {
    let f = |a: i64, b: i64| match op {
        Arith::Add => a.checked_add(b),
        Arith::Sub => a.checked_sub(b),
        Arith::Mul => a.checked_mul(b),
        Arith::Div => a.checked_div(b),
        Arith::Days | Arith::Weekdays | Arith::Working(_) => None,
    };
    match (x, y) {
        // Calendar days in UTC, both counted: none or fewer when `b` is before `a` (appendix AN.2.2).
        (Val::Time(a), Val::Time(b)) if op == Arith::Days => Some(Val::Count(b.div_euclid(86_400) - a.div_euclid(86_400) + 1)),
        (Val::Time(a), Val::Time(b)) if op == Arith::Weekdays => Some(Val::Count(weekdays(a.div_euclid(86_400), b.div_euclid(86_400)))),
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
pub(crate) fn positions(root: &Node) -> Vec<Vec<usize>> {
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

/// Whether every node on the walk to `at` is the current attempt of its
/// kind, so the node is not left over from a replaced owner.
fn live(root: &Node, at: &[usize]) -> bool {
    (0..at.len()).all(|k| current(node_at(root, &at[..k]), &node_at(root, &at[..=k]).kind) == Some(at[k]))
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
            (_, Step::Caller) => Place::There(v.caller?.to_string()),
            (_, Step::Elem) => Place::Here(v.elem?.to_vec()),
            (_, Step::Data(_)) => return None,
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
    if path.steps == [Step::Caller] {
        return v.caller.map(|c| Val::Ref(c.to_string()));
    }
    if let [Step::Data(n)] = path.steps.as_slice() {
        return v.data.iter().find(|(d, _)| d == n).map(|(_, x)| x.clone());
    }
    let (last, rest) = path.steps.split_last()?;
    let Step::Attr(name) = last else { return None };
    match locate(v, from, rest)? {
        Place::Here(at) if name == CREATED => Some(Val::Time(node_at(v.root, &at).created)),
        Place::Here(at) => {
            let n = node_at(v.root, &at);
            if let (Some(now), Some(f)) = (v.now, n.fetched.get(name)) {
                let bound = v.bounds.iter().find(|(a, _)| a == name).map(|(_, secs)| *secs);
                if !f.fresh(now, bound) {
                    return None;
                }
            }
            n.attrs.get(name).cloned()
        }
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
