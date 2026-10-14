//! The interpreter (spec appendix B.3): one step of a unit for one message.
//! It is a pure function of the program, the unit and the message, so
//! replaying the journal reproduces every step.

use suture_lang::ir::{Body, Cond, Effect, Op, Operand, Path, Program, Rule, Step};

use crate::state::{Ask, Msg, Node, Status, Unit, Val};

/// The most attempts of a part before its need gives up (spec §3.2).
pub const ATTEMPTS: u32 = 3;

/// A bound on settling. A unit's parts are bounded, so a valid program
/// settles long before this.
const SETTLE_ROUNDS: usize = 256;

/// The unit after a step, and the requests the step sends out.
pub struct Outcome {
    pub unit: Unit,
    pub asks: Vec<Ask>,
}

/// A new unit for a root entity: its defaults, then the given attributes.
pub fn new_unit(prog: &Program, label: &str, kind: &str, given: &[(String, Val)]) -> Result<Unit, String> {
    let k = prog.kind(kind).ok_or_else(|| format!("there is no kind {kind}"))?;
    if let Some(o) = &k.owner {
        return Err(format!("{kind} is a part of {}; its owner's needs create it", o.kind));
    }
    let mut root = blank(prog, kind, 1);
    for (name, v) in given {
        let a = k.attr(name).ok_or_else(|| format!("{kind} has no attribute {name}"))?;
        if v.ty() != a.ty {
            return Err(format!("{kind}.{name} is {:?}, not {:?}", a.ty, v.ty()));
        }
        root.attrs.insert(name.clone(), v.clone());
    }
    Ok(Unit { label: label.to_string(), root, notes: vec![], met: vec![] })
}

fn blank(prog: &Program, kind: &str, attempt: u32) -> Node {
    let attrs = prog
        .kind(kind)
        .map(|k| k.attrs.iter().filter_map(|a| a.default.as_ref().map(|d| (a.name.clone(), Val::from_lit(d)))).collect())
        .unwrap_or_default();
    Node { kind: kind.to_string(), attrs, status: Status::Idle, outcome: None, attempt, waiting: None, parts: vec![] }
}

/// One step: apply the message, then settle the unit (create, replace and
/// withdraw parts, finish parts, send the requests every rule allows), then
/// note the goals that became met.
pub fn step(prog: &Program, unit: &Unit, msg: &Msg) -> Outcome {
    let mut u = unit.clone();
    u.notes.clear();
    let mut asks = vec![];
    match msg {
        Msg::Create => {
            let attrs: Vec<String> = u.root.attrs.iter().map(|(k, v)| format!("{k} {}", v.show(&prog.currency))).collect();
            u.notes.push(format!("placed: {}", attrs.join(", ")));
        }
        Msg::Answer { key, outcome, data } => {
            if let Err(refusal) = answer(prog, &mut u, key, outcome, data) {
                let mut kept = unit.clone();
                kept.notes = vec![refusal.note];
                asks.extend(refusal.again);
                return Outcome { unit: kept, asks };
            }
        }
    }
    settle(prog, &mut u, &mut asks);
    goals(prog, &mut u);
    Outcome { unit: u, asks }
}

/// Why an answer was not recorded, and the request to send again, if any.
struct Refusal {
    note: String,
    again: Option<Ask>,
}

fn answer(prog: &Program, u: &mut Unit, key: &str, outcome: &str, data: &[(String, Val)]) -> Result<(), Refusal> {
    let open = positions(&u.root)
        .into_iter()
        .find(|at| node_at(&u.root, at).status == Status::Requested && request_key(&u.label, &u.root, at) == key);
    let Some(at) = open else {
        return Err(Refusal { note: format!("ignored an answer for {key}: no open request"), again: None });
    };
    let kind = node_at(&u.root, &at).kind.clone();
    let effect = prog.effect_of(&kind).expect("a requested part has an effect");
    let again = Some(ask_for(&u.label, &u.root, &at, effect));
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
    react(prog, &mut t, &at, &kind, outcome, &mut notes);
    if let Some(r) = violated(prog, &t) {
        return Err(Refusal { note: format!("refused by rule {}: {}", r.name, r.paraphrase), again });
    }
    u.root = t;
    u.notes.extend(notes);
    Ok(())
}

/// Applies the `when` reactions to an outcome (spec §3.3).
fn react(prog: &Program, root: &mut Node, at: &[usize], kind: &str, outcome: &str, notes: &mut Vec<String>) {
    for r in prog.reactions.iter().filter(|r| r.kind == kind && r.outcome == outcome) {
        for a in &r.actions {
            let Some(target) = locate(root, at, &a.target.steps) else { continue };
            let n = node_at_mut(root, &target);
            n.attrs.insert(a.attr.clone(), Val::Bool(a.value));
            notes.push(format!("{} is {}{}", n.kind, if a.value { "" } else { "not " }, a.attr));
        }
    }
}

fn settle(prog: &Program, u: &mut Unit, asks: &mut Vec<Ask>) {
    for _ in 0..SETTLE_ROUNDS {
        let changed = positions(&u.root)
            .iter()
            .any(|at| needs_pass(prog, u, at) || done_pass(prog, u, at) || request_pass(prog, u, at, asks));
        if !changed {
            return;
        }
    }
    u.notes.push("stopped settling: the program keeps changing this unit".into());
}

/// Creates a part an active need lacks, replaces one that failed, or
/// withdraws one whose need is no longer active and that has not reached the
/// outside world (spec §3.2, D9). Makes at most one change.
fn needs_pass(prog: &Program, u: &mut Unit, at: &[usize]) -> bool {
    enum Change {
        Add(Node, String),
        Remove(usize, String),
    }
    let change = {
        let n = node_at(&u.root, at);
        if n.status == Status::Failed {
            return false;
        }
        let mut change = None;
        for need in prog.needs.iter().filter(|need| need.owner == n.kind) {
            let active = need.cond.as_ref().is_none_or(|c| eval(prog, &u.root, at, c) == T::Yes);
            match current(n, &need.part).map(|i| (i, &n.parts[i])) {
                None if active => change = Some(Change::Add(blank(prog, &need.part, 1), format!("needs {}", need.part))),
                Some((_, p)) if active && p.status == Status::Failed && p.attempt < ATTEMPTS => {
                    let note = format!("{} {}: attempt {}", need.part, p.outcome.as_deref().unwrap_or("failed"), p.attempt + 1);
                    change = Some(Change::Add(blank(prog, &need.part, p.attempt + 1), note));
                }
                Some((i, p)) if !active && untouched(p) => {
                    change = Some(Change::Remove(i, format!("{} withdrawn", need.part)));
                }
                _ => {}
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
            n.parts.push(part);
            u.notes.push(note);
            true
        }
        Some(Change::Remove(i, note)) => {
            n.parts.remove(i);
            u.notes.push(note);
            true
        }
        None => false,
    }
}

/// A part with no effect of its own is done once every active need of it is
/// done (appendix B.3.2). A root is never done; its goals describe it.
fn done_pass(prog: &Program, u: &mut Unit, at: &[usize]) -> bool {
    let n = node_at(&u.root, at);
    if at.is_empty() || n.status != Status::Idle || prog.effect_of(&n.kind).is_some() || !needs_done(prog, &u.root, at) {
        return false;
    }
    let kind = n.kind.clone();
    node_at_mut(&mut u.root, at).status = Status::Done;
    u.notes.push(format!("{kind} done"));
    true
}

/// Whether every active need of the node at `at` has a current part that is done.
fn needs_done(prog: &Program, root: &Node, at: &[usize]) -> bool {
    let n = node_at(root, at);
    prog.needs.iter().filter(|need| need.owner == n.kind).all(|need| {
        let active = need.cond.as_ref().is_none_or(|c| eval(prog, root, at, c) == T::Yes);
        !active || current(n, &need.part).is_some_and(|i| n.parts[i].status == Status::Done)
    })
}

/// Sends a ready part's request, unless a rule would break. Then the part
/// waits on that rule, and every later step of the unit checks it again
/// (appendix B.3.5).
fn request_pass(prog: &Program, u: &mut Unit, at: &[usize], asks: &mut Vec<Ask>) -> bool {
    let n = node_at(&u.root, at);
    let Some(effect) = prog.effect_of(&n.kind) else { return false };
    if n.status != Status::Idle || !needs_done(prog, &u.root, at) {
        return false;
    }
    let (kind, waiting) = (n.kind.clone(), n.waiting.clone());
    let mut t = u.root.clone();
    node_at_mut(&mut t, at).status = Status::Requested;
    match violated(prog, &t) {
        Some(r) if waiting.as_deref() == Some(r.name.as_str()) => false,
        Some(r) => {
            node_at_mut(&mut u.root, at).waiting = Some(r.name.clone());
            u.notes.push(format!("{kind} waits: rule {}", r.name));
            true
        }
        None => {
            node_at_mut(&mut t, at).waiting = None;
            u.root = t;
            let a = ask_for(&u.label, &u.root, at, effect);
            let sends: Vec<String> = a.sends.iter().map(|(k, v)| format!("{k} {}", v.show(&prog.currency))).collect();
            u.notes.push(format!("{kind} → {} via {} ({})", effect.verb, effect.adapter, sends.join(", ")));
            asks.push(a);
            true
        }
    }
}

/// Notes each goal of the root the first time it is met.
fn goals(prog: &Program, u: &mut Unit) {
    for (i, g) in prog.goals.iter().enumerate() {
        let i = i as u32;
        if g.kind != u.root.kind || u.met.contains(&i) {
            continue;
        }
        let applies = g.filter.iter().all(|f| eval(prog, &u.root, &[], f) == T::Yes);
        if applies && g.states.iter().all(|s| in_state(prog, &u.root, &[], s)) {
            u.met.push(i);
            u.notes.push(format!("goal met: {}", g.source));
        }
    }
}

/// Whether the node at `at` is in `state` (appendix B.3): `done`, `failed`, a
/// Bool attribute, its effect's marks or outcomes, or a defined state.
pub fn in_state(prog: &Program, root: &Node, at: &[usize], state: &str) -> bool {
    let n = node_at(root, at);
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
        if e.success == state || e.failures.iter().any(|f| f == state) {
            return n.outcome.as_deref() == Some(state);
        }
    }
    prog.defines.iter().any(|d| d.kind == n.kind && d.state == state && eval(prog, root, at, &d.cond) == T::Yes)
}

/// The first rule the unit's state breaks, if any. A rule applies to a node
/// only when its filter is known to hold.
pub fn violated<'p>(prog: &'p Program, root: &Node) -> Option<&'p Rule> {
    let all = positions(root);
    prog.rules.iter().find(|r| {
        all.iter().any(|at| {
            node_at(root, at).kind == r.subject
                && r.filter.iter().all(|f| eval(prog, root, at, f) == T::Yes)
                && match &r.body {
                    Body::Every(c) => eval(prog, root, at, c) == T::No,
                    Body::No { pred, unless } => {
                        eval(prog, root, at, pred) == T::Yes && unless.as_ref().is_none_or(|u| eval(prog, root, at, u) != T::Yes)
                    }
                }
        })
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

fn eval(prog: &Program, root: &Node, at: &[usize], c: &Cond) -> T {
    match c {
        Cond::Cmp { left, op, right } => {
            let Some(l) = value(root, at, left) else { return T::Unknown };
            let r = match right {
                Operand::Lit(lit) => Val::from_lit(lit),
                Operand::Path(p) => match value(root, at, p) {
                    Some(v) => v.clone(),
                    None => return T::Unknown,
                },
            };
            compare(l, &r, *op).map_or(T::No, truth)
        }
        Cond::State { node, state, negated } => {
            let yes = locate(root, at, &node.steps).is_some_and(|n| in_state(prog, root, &n, state));
            truth(yes != *negated)
        }
        Cond::Has { node, part, state, negated } => {
            let exists = locate(root, at, &node.steps).is_some_and(|n| {
                node_at(root, &n).parts.iter().enumerate().any(|(i, p)| {
                    p.kind == *part && state.as_ref().is_none_or(|s| in_state(prog, root, &child(&n, i), s))
                })
            });
            truth(exists != *negated)
        }
        Cond::And(cs) => {
            let ts: Vec<T> = cs.iter().map(|c| eval(prog, root, at, c)).collect();
            if ts.contains(&T::No) {
                T::No
            } else if ts.contains(&T::Unknown) {
                T::Unknown
            } else {
                T::Yes
            }
        }
        Cond::Or(cs) => {
            let ts: Vec<T> = cs.iter().map(|c| eval(prog, root, at, c)).collect();
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

/// Compares two values of the same type; `None` when the types differ.
fn compare(l: &Val, r: &Val, op: Op) -> Option<bool> {
    let ord = match (l, r) {
        (Val::Money(a), Val::Money(b)) | (Val::Count(a), Val::Count(b)) => a.cmp(b),
        (Val::Text(a), Val::Text(b)) => a.cmp(b),
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

/// Follows parts and owners. `None` if a part is missing, or the walk goes above the root.
fn locate(root: &Node, from: &[usize], steps: &[Step]) -> Option<Vec<usize>> {
    let mut at = from.to_vec();
    for s in steps {
        match s {
            Step::Owner => {
                at.pop()?;
            }
            Step::Part(k) => {
                let i = current(node_at(root, &at), k)?;
                at.push(i);
            }
            Step::Attr(_) => return None,
        }
    }
    Some(at)
}

/// The value a path ends at, if it has one yet.
fn value<'a>(root: &'a Node, from: &[usize], path: &Path) -> Option<&'a Val> {
    let (last, rest) = path.steps.split_last()?;
    let Step::Attr(name) = last else { return None };
    node_at(root, &locate(root, from, rest)?).attrs.get(name)
}

fn ask_for(label: &str, root: &Node, at: &[usize], e: &Effect) -> Ask {
    let sends = e.sends.iter().filter_map(|(name, path)| value(root, at, path).map(|v| (name.clone(), v.clone()))).collect();
    Ask {
        adapter: e.adapter.clone(),
        verb: e.verb.clone(),
        kind: e.kind.clone(),
        label: label.to_string(),
        key: request_key(label, root, at),
        sends,
    }
}

/// `O1/CustomsForm/Declaration#1`: the unit, the kinds from the root down to
/// the part, and its attempt.
pub fn request_key(label: &str, root: &Node, at: &[usize]) -> String {
    let mut kinds = vec![];
    let mut n = root;
    for &i in at {
        n = &n.parts[i];
        kinds.push(n.kind.as_str());
    }
    format!("{label}/{}#{}", kinds.join("/"), n.attempt)
}
