//! From syntax to the compiled program (spec §4.1–§4.2, appendix B.3). It
//! resolves every name, checks types, owners and states, and attaches every
//! rule's read-back. Every check is fixed; no model decides anything (D4).

use crate::ast::{self, CmpOp, Head, Literal, Loc, NounPhrase, Source, Term, TypeName};
use crate::ir::{
    Action, Attr, Body, Cond, Define, Effect, Goal, Kind, Lit, Need, Op, Operand, OwnerRef, Path, Program, Reaction,
    Rule, Step, Ty,
};
use crate::lex::Error;
use crate::readback;

/// Checks a parsed module and compiles it, reporting every error it finds.
pub fn check(src: &Source) -> Result<Program, Vec<Error>> {
    let mut cx = Checker { errors: vec![] };
    let prog = cx.program(src);
    if cx.errors.is_empty() { Ok(prog) } else { Err(cx.errors) }
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
        TypeName::Kind(_) => None,
    }
}

/// A noun phrase as written, for messages.
fn show(np: &NounPhrase) -> String {
    let mut s = match (&np.head, np.steps.is_empty()) {
        (Head::It, true) => "it".to_string(),
        (Head::It, false) => "its".to_string(),
        (Head::The(k), _) => format!("the {k}"),
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
        };
        self.kinds(src, &mut p);
        let conds = self.owners(src, &mut p);
        self.effects(src, &mut p);
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
                attrs.push(Attr { name: a.name.clone(), ty, default });
            }
            p.kinds.push(Kind { name: k.name.clone(), attrs, owner: None });
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
                let Some(decl) = src.kinds.iter().find(|k| k.name == *part) else {
                    self.errors.push(err(&n.at, format!("unknown kind {part}")));
                    continue;
                };
                let owner_type = TypeName::Kind(n.owner.clone());
                let refs: Vec<&str> = decl.attrs.iter().filter(|a| a.ty == owner_type).map(|a| a.name.as_str()).collect();
                if refs.len() != 1 {
                    self.errors.push(err(
                        &n.at,
                        format!("{part} needs exactly one attribute of kind {} to name its owner; it has {}", n.owner, refs.len()),
                    ));
                    continue;
                }
                let kind = p.kinds.iter_mut().find(|k| k.name == *part).expect("declared kinds are compiled");
                if let Some(o) = &kind.owner {
                    if o.kind != n.owner {
                        self.errors.push(err(&n.at, format!("{part} is already a part of {}; a part has one owner", o.kind)));
                        continue;
                    }
                }
                kind.owner = Some(OwnerRef { attr: refs[0].to_string(), kind: n.owner.clone() });
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
                    None => kind.attrs.push(Attr { name: name.clone(), ty, default: None }),
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
            let mut rule = Rule {
                name: r.name.clone(),
                subject: subject.kind.clone(),
                filter,
                body,
                source: r.source.clone(),
                formula: String::new(),
                paraphrase: String::new(),
                tier: "local".into(),
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
            if e.success != r.outcome && !e.failures.contains(&r.outcome) {
                self.errors.push(err(
                    &r.at,
                    format!("a {} reports `{}` or fails with {}, not `{}`", r.kind, e.success, e.failures.join(", "), r.outcome),
                ));
                continue;
            }
            let mut actions = vec![];
            for a in &r.actions {
                match self.np(p, &r.at, &r.kind, &a.target) {
                    Some((target, End::Node(k))) => {
                        let is_bool = p.kind(&k).and_then(|x| x.attr(&a.attr)).is_some_and(|x| x.ty == Ty::Bool);
                        if is_bool {
                            actions.push(Action { target, attr: a.attr.clone(), value: a.value });
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

    /// Resolves a noun phrase from a node of kind `from` (spec B.3.8): each
    /// word is an attribute, a part, or the owner, tried in that order.
    fn np(&mut self, p: &Program, at: &Loc, from: &str, np: &NounPhrase) -> Option<(Path, End)> {
        let mut steps = vec![];
        let mut cur = from.to_string();
        if let Head::The(k) = &np.head {
            while cur != *k {
                match p.kind(&cur).and_then(|x| x.owner.as_ref()) {
                    Some(o) => {
                        steps.push(Step::Owner);
                        cur = o.kind.clone();
                    }
                    None => {
                        self.errors.push(err(at, format!("`the {k}`: a {from} is not part of a {k}")));
                        return None;
                    }
                }
            }
        }
        for (i, w) in np.steps.iter().enumerate() {
            let kind = p.kind(&cur)?;
            if let Some(a) = kind.attr(w) {
                if let Some(next) = np.steps.get(i + 1) {
                    self.errors.push(err(at, format!("`{}`: {w} is a value, so it has no `{next}`", show(np))));
                    return None;
                }
                steps.push(Step::Attr(w.clone()));
                return Some((Path { steps }, End::Value(a.ty)));
            }
            if p.needs.iter().any(|n| n.owner == cur && n.part == *w) {
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
                if !p.needs.iter().any(|n| n.owner == k && n.part == *kind) {
                    self.errors.push(err(at, format!("a {k} does not need a {kind}")));
                    return None;
                }
                if let Some(s) = state {
                    self.state(p, at, kind, s)?;
                }
                Some(Cond::Has { node, part: kind.clone(), state: state.clone(), negated: *negated })
            }
            ast::Cond::Cmp { left, op, right } => {
                let (path, end) = self.np(p, at, from, left)?;
                let ty = match end {
                    End::Value(t @ (Ty::Money | Ty::Count)) => t,
                    End::Value(t) => {
                        self.errors.push(err(
                            at,
                            format!("`{}` is {t:?}; only amounts and counts compare with at most, at least, more than or less than", show(left)),
                        ));
                        return None;
                    }
                    End::Node(k) => {
                        self.errors.push(err(at, format!("`{}` is a {k}, not an amount or a count", show(left))));
                        return None;
                    }
                };
                let right = self.operand(p, at, from, right, ty)?;
                let op = match op {
                    CmpOp::AtMost => Op::Le,
                    CmpOp::AtLeast => Op::Ge,
                    CmpOp::MoreThan => Op::Gt,
                    CmpOp::LessThan => Op::Lt,
                };
                Some(Cond::Cmp { left: path, op, right })
            }
            ast::Cond::Is { left, negated, right } => {
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
                        Some(Cond::Cmp { left: path, op: if *negated { Op::Ne } else { Op::Eq }, right })
                    }
                }
            }
        }
    }

    /// The other side of a comparison with a value of type `ty`. A bare word
    /// is text when `ty` is Text (spec B.3.7).
    fn operand(&mut self, p: &Program, at: &Loc, from: &str, t: &Term, ty: Ty) -> Option<Operand> {
        let (operand, found) = match t {
            Term::Lit(l) => {
                let (lit, t) = self.lit(at, l, &p.currency)?;
                (Operand::Lit(lit), t)
            }
            Term::Word(w) if ty == Ty::Text => (Operand::Lit(Lit::Text(w.clone())), Ty::Text),
            Term::Word(w) => {
                self.errors.push(err(at, format!("expected a {ty:?} value, found `{w}`")));
                return None;
            }
            Term::Np(np) => match self.np(p, at, from, np)? {
                (path, End::Value(t)) => (Operand::Path(path), t),
                (_, End::Node(k)) => {
                    self.errors.push(err(at, format!("`{}` is a {k}, not a value", show(np))));
                    return None;
                }
            },
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
