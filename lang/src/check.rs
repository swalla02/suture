//! From syntax to the compiled program (spec §4.1–§4.2, appendix B.3). It
//! resolves every name, checks types, owners and states, and attaches every
//! rule's read-back. Every check is fixed; no model decides anything (D4).

use crate::ast::{self, CmpOp, Head, Literal, Loc, NounPhrase, Source, Term, TypeName};
use crate::ir::{
    Action, Arith, Attr, Body, Cond, Define, Effect, Goal, Kind, Lit, Need, Num, Op, OwnerRef, Path, Program, Reaction,
    Rule, Slot, Step, Ty, TIMED_OUT,
};
use crate::lex::Error;
use crate::readback;

/// Checks a parsed module and compiles it, reporting every error it finds.
pub fn check(src: &Source) -> Result<Program, Vec<Error>> {
    let mut cx = Checker { errors: vec![] };
    let prog = cx.program(src);
    if cx.errors.is_empty() { Ok(prog) } else { Err(cx.errors) }
}

/// Resolves a condition about a `kind` against a compiled program, as a
/// scenario's `expect` needs (appendix E.2.9).
pub fn condition(p: &Program, kind: &str, c: &ast::Cond) -> Result<Cond, Vec<Error>> {
    let mut cx = Checker { errors: vec![] };
    let at = Loc { file: String::new(), line: 0 };
    match cx.cond(p, &at, kind, c) {
        Some(c) if cx.errors.is_empty() => Ok(c),
        _ => Err(cx.errors),
    }
}

/// `Refunds`, `Boxes`, `Branches`: the regular plural (spec §3.1).
fn plural_of(name: &str) -> String {
    let es = ["s", "x", "ch", "sh"].iter().any(|e| name.ends_with(e));
    format!("{name}{}", if es { "es" } else { "s" })
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
    }
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
            Num::Sum { .. } | Num::Count { .. } => true,
            Num::Bin(x, _, y) => num(x) || num(y),
            Num::Path(_) | Num::Lit(_) => false,
        }
    }
    fn cond(c: &Cond) -> bool {
        match c {
            Cond::Cmp { left, right, .. } => num(left) || num(right),
            Cond::And(cs) | Cond::Or(cs) => cs.iter().any(cond),
            Cond::State { .. } | Cond::Has { .. } => false,
        }
    }
    match b {
        Body::Every(c) => cond(c),
        Body::No { pred, unless } => cond(pred) || unless.as_ref().is_some_and(cond),
    }
}

/// Where a noun phrase ends: at a node of some kind, or at a value of some type.
enum End {
    Node(String),
    Value(Ty),
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
        TypeName::Kind(_) | TypeName::List(_) => None,
    }
}

/// A noun phrase as written, for messages.
fn show(np: &NounPhrase) -> String {
    let mut s = match (&np.head, np.steps.is_empty()) {
        (Head::It, true) => "it".to_string(),
        (Head::It, false) => "its".to_string(),
        (Head::The(k), _) => format!("the {k}"),
        (Head::Each(k), _) => format!("each {k}"),
    };
    for (i, w) in np.steps.iter().enumerate() {
        s.push_str(if i == 0 && matches!(np.head, Head::It) { " " } else { "'s " });
        s.push_str(w);
    }
    s
}

struct Checker {
    errors: Vec<Error>,
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
            rules: vec![],
            defines: vec![],
            goals: vec![],
            reactions: vec![],
            slots: vec![],
            lists: vec![],
            attempts: src.module.as_ref().and_then(|m| m.attempts).unwrap_or(3),
            warnings: vec![],
        };
        self.kinds(src, &mut p);
        self.slots(src, &mut p);
        self.lists(src, &mut p);
        let conds = self.owners(src, &mut p);
        self.prefers(src, &mut p);
        self.effects(src, &mut p);
        self.list_elements(src, &p);
        self.state_names(src, &p);
        self.formulas(src, &mut p);
        self.defines(src, &mut p);
        for (i, owner, at, cond) in conds {
            let c = self.cond(&p, at, &owner, cond);
            p.needs[i].cond = c;
        }
        self.rules(src, &mut p);
        self.goals(src, &mut p);
        self.reactions(src, &mut p);
        p
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
                let Some(ty) = value_type(&a.ty) else { continue }; // an owner reference: see `owners`
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
                attrs.push(Attr { name: a.name.clone(), ty, default, formula: None });
            }
            let plural = k.plural.clone().unwrap_or_else(|| plural_of(&k.name));
            p.kinds.push(Kind { name: k.name.clone(), plural, attrs, owner: None });
        }
    }

    /// A literal's value and type. Money must be in the module's currency.
    fn lit(&mut self, at: &Loc, l: &Literal, currency: &str) -> Option<(Lit, Ty)> {
        Some(match l {
            Literal::Text(s) => (Lit::Text(s.clone()), Ty::Text),
            Literal::Bool(b) => (Lit::Bool(*b), Ty::Bool),
            Literal::Count(n) => (Lit::Count(*n), Ty::Count),
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
        for k in &src.kinds {
            for a in &k.attrs {
                let TypeName::Kind(target) = &a.ty else { continue };
                let owner = p.kind(&k.name).and_then(|x| x.owner.as_ref());
                if owner.is_none_or(|o| o.kind != *target) {
                    self.errors.push(err(
                        &k.at,
                        format!("{}.{}: {target} is not the owner of {}; in slice 1 a kind refers only to its owner", k.name, a.name, k.name),
                    ));
                }
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
        if let Some(o) = &kind.owner {
            if o.kind != owner {
                self.errors.push(err(at, format!("{part} is already a part of {}; a part has one owner", o.kind)));
                return false;
            }
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
                let Some(want) = p.kind(&k.name).and_then(|x| x.attr(&a.name)).map(|x| x.ty) else { continue };
                let Some((num, ty)) = self.num(p, &k.at, &k.name, f) else { continue };
                if ty != want {
                    self.errors.push(err(&k.at, format!("{}.{} is {want:?}, but its formula gives a {ty:?}", k.name, a.name)));
                    continue;
                }
                let kind = p.kinds.iter_mut().find(|x| x.name == k.name).expect("declared kinds are compiled");
                if let Some(x) = kind.attrs.iter_mut().find(|x| x.name == a.name) {
                    x.formula = Some(num);
                }
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
        for pr in &src.prefers {
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
                match kind.attr(name).map(|a| a.ty) {
                    Some(have) if have != ty => {
                        self.errors.push(err(&e.at, format!("{}.{name} is {have:?}, but the answer gives {ty:?}", e.kind)));
                        continue;
                    }
                    Some(_) => {}
                    None => kind.attrs.push(Attr { name: name.clone(), ty, default: None, formula: None }),
                }
                data.push((name.clone(), ty));
            }
            let mut sends = vec![];
            for np in &e.sends {
                match self.np(p, &e.at, &e.kind, np) {
                    Some((path, End::Value(_))) => sends.push((np.steps.last().cloned().unwrap_or_default(), path)),
                    Some((_, End::Node(k))) => {
                        self.errors.push(err(&e.at, format!("an effect sends values; `{}` is a {k}", show(np))));
                    }
                    None => {}
                }
            }
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
            if p.kind(&subject.kind).is_none() {
                self.errors.push(err(&r.at, format!("unknown kind {}", subject.kind)));
                continue;
            }
            let Some(filter) = self.filter(p, &r.at, subject) else { continue };
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
            let tier = if body_sums(&body) { "per-group" } else { "local" };
            let mut rule = Rule {
                name: r.name.clone(),
                subject: subject.kind.clone(),
                filter,
                body,
                source: r.source.clone(),
                formula: String::new(),
                paraphrase: String::new(),
                tier: tier.into(),
            };
            rule.formula = readback::formula(p, &rule);
            rule.paraphrase = readback::paraphrase(p, &rule);
            p.rules.push(rule);
        }
    }

    fn goals(&mut self, src: &Source, p: &mut Program) {
        for g in &src.goals {
            let kind = &g.subject.kind;
            let Some(k) = p.kind(kind) else {
                self.errors.push(err(&g.at, format!("unknown kind {kind}")));
                continue;
            };
            if let Some(o) = &k.owner {
                self.errors.push(err(&g.at, format!("in slice 1 a goal is about a root kind; {kind} is a part of {}", o.kind)));
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
            let mut actions = vec![];
            for a in &r.actions {
                match self.np(p, &r.at, &r.kind, &a.target) {
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
                        actions.push(Action { target, attr: a.attr.clone(), value: true, becomes: Some(num) });
                    }
                    Some((target, End::Node(k))) => {
                        let is_bool = p.kind(&k).and_then(|x| x.attr(&a.attr)).is_some_and(|x| x.ty == Ty::Bool);
                        if is_bool {
                            actions.push(Action { target, attr: a.attr.clone(), value: a.value, becomes: None });
                        } else {
                            self.errors.push(err(&r.at, format!("a {k} has no Bool attribute `{}` to set", a.attr)));
                        }
                    }
                    Some((_, End::Value(_))) => {
                        self.errors.push(err(&r.at, format!("`{}` is a value; an action sets an attribute of a node", show(&a.target))));
                    }
                    None => {}
                }
            }
            p.reactions.push(Reaction { kind: r.kind.clone(), outcome: r.outcome.clone(), actions });
        }
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
                (_, End::Node(k)) => {
                    self.errors.push(err(at, format!("`{}` is {}, not a value", show(np), a(&k))));
                    None
                }
            },
            ast::Num::Bin(x, op, y) => {
                let (l, lt) = self.num(p, at, from, x)?;
                let (r, rt) = self.num(p, at, from, y)?;
                let (arith, ty) = match (op, lt, rt) {
                    ('+', Ty::Money, Ty::Money) | ('-', Ty::Money, Ty::Money) => (if *op == '+' { Arith::Add } else { Arith::Sub }, Ty::Money),
                    ('+', Ty::Count, Ty::Count) | ('-', Ty::Count, Ty::Count) => (if *op == '+' { Arith::Add } else { Arith::Sub }, Ty::Count),
                    ('*', Ty::Money, Ty::Count) | ('*', Ty::Count, Ty::Money) => (Arith::Mul, Ty::Money),
                    ('*', Ty::Count, Ty::Count) => (Arith::Mul, Ty::Count),
                    ('/', Ty::Money, Ty::Count) => (Arith::Div, Ty::Money),
                    ('/', Ty::Count, Ty::Count) => (Arith::Div, Ty::Count),
                    _ => {
                        let what = match op {
                            '+' => format!("cannot add a {lt:?} and a {rt:?}"),
                            '-' => format!("cannot subtract a {rt:?} from a {lt:?}"),
                            '*' => format!("cannot multiply a {lt:?} by a {rt:?}"),
                            _ => format!("cannot divide a {lt:?} by a {rt:?}"),
                        };
                        self.errors.push(err(at, format!("`{}`: {what}", show_num(n))));
                        return None;
                    }
                };
                Some((Num::Bin(Box::new(l), arith, Box::new(r)), ty))
            }
            ast::Num::Total { attr, of, filter } => {
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
                let one = l.singular();
                let (calc, ty) = self.over(p, at, from, &element, &one, calc)?;
                if !matches!(ty, Ty::Money | Ty::Count) {
                    self.errors.push(err(at, format!("`{}` is {ty:?}; only amounts and counts add up", show_num(n))));
                    return None;
                }
                Some((Num::Sum { of: path, kind: element, calc: Box::new(calc), filter: None, ty }, ty))
            }
        }
    }

    /// The calculation inside `the total of … over its lines`, evaluated at
    /// each element: `the line's price` reads the element, anything else
    /// reads the owner through the element's owner step.
    fn over(&mut self, p: &Program, at: &Loc, owner: &str, element: &str, one: &str, n: &ast::Num) -> Option<(Num, Ty)> {
        match n {
            ast::Num::Np(np) if matches!(np.head, Head::It) && np.steps.first().is_some_and(|w| w == one) => {
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
            ast::Num::Bin(x, op, y) => {
                let (l, lt) = self.over(p, at, owner, element, one, x)?;
                let (r, rt) = self.over(p, at, owner, element, one, y)?;
                let lit = |t: Ty| ast::Num::Lit(match t {
                    Ty::Money => Literal::Money { cents: 0, currency: p.currency.clone() },
                    _ => Literal::Count(0),
                });
                // Type the operation as the checker does for any calculation.
                let (_, ty) = self.num(p, at, owner, &ast::Num::Bin(Box::new(lit(lt)), *op, Box::new(lit(rt))))?;
                let arith = match op {
                    '+' => Arith::Add,
                    '-' => Arith::Sub,
                    '*' => Arith::Mul,
                    _ => Arith::Div,
                };
                Some((Num::Bin(Box::new(l), arith, Box::new(r)), ty))
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
    /// word is an attribute, a part, or the owner, tried in that order.
    fn np(&mut self, p: &Program, at: &Loc, from: &str, np: &NounPhrase) -> Option<(Path, End)> {
        let mut steps = vec![];
        let mut cur = from.to_string();
        if let Head::Each(k) = &np.head {
            if k != from {
                self.errors.push(err(at, format!("`each {k}`: this sentence is about {}", a(from))));
                return None;
            }
        }
        if let Head::The(k) = &np.head {
            while cur != *k {
                // A valid chain of owners is shorter than the number of kinds;
                // a longer walk is going round a needs loop, reported elsewhere.
                match p.kind(&cur).and_then(|x| x.owner.as_ref()) {
                    Some(o) if steps.len() < p.kinds.len() => {
                        steps.push(Step::Owner);
                        cur = o.kind.clone();
                    }
                    _ => {
                        self.errors.push(err(at, format!("`the {k}`: a {from} is not part of a {k}")));
                        return None;
                    }
                }
            }
        }
        for (i, w) in np.steps.iter().enumerate() {
            if plural_kind(p, &cur, w).is_some() {
                self.errors.push(err(
                    at,
                    format!("`{}` is a relation; count it with `the number of …` or add it up with `the total … of …`", show(np)),
                ));
                return None;
            }
            let kind = p.kind(&cur)?;
            if let Some(a) = kind.attr(w) {
                if let Some(next) = np.steps.get(i + 1) {
                    self.errors.push(err(at, format!("`{}`: {w} is a value, so it has no `{next}`", show(np))));
                    return None;
                }
                steps.push(Step::Attr(w.clone()));
                return Some((Path { steps }, End::Value(a.ty)));
            }
            if p.slot(w).is_some() && p.needs.iter().any(|n| n.owner == cur && n.part == *w) {
                if np.steps.get(i + 1).is_some() {
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
                    self.errors.push(err(at, format!("a {cur} has no attribute, part or owner called `{w}`")));
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
                let End::Node(k) = end else {
                    self.errors.push(err(at, format!("`{}` is a value; only a node has parts", show(owner))));
                    return None;
                };
                if p.slot(kind).is_some() {
                    let first = p.slot(kind).map(|s| s.alternatives[0].clone()).unwrap_or_default();
                    self.errors.push(err(at, format!("`has a {kind}`: {kind} is a slot; name an alternative (`has a {first}`) or say `its {kind} is done`")));
                    return None;
                }
                if !p.has_part(&k, kind) {
                    self.errors.push(err(at, format!("a {k} does not need a {kind}")));
                    return None;
                }
                if let Some(s) = state {
                    self.state(p, at, kind, s)?;
                }
                Some(Cond::Has { node, part: kind.clone(), state: state.clone(), negated: *negated })
            }
            ast::Cond::Cmp { left, op, right } => {
                let (num, ty) = self.num(p, at, from, left)?;
                if !matches!(ty, Ty::Money | Ty::Count) {
                    self.errors.push(err(
                        at,
                        format!("`{}` is {ty:?}; only amounts and counts compare with at most, at least, more than or less than", show_num(left)),
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
                        Some(Cond::Cmp { left: Num::Path(path), op: if *negated { Op::Ne } else { Op::Eq }, right })
                    }
                }
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
        self.errors.push(err(at, format!("a {kind} has no state `{s}`; its states are {}", states.join(", "))));
        None
    }
}
