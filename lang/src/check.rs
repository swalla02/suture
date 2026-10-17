//! From syntax to the compiled program (spec §4.1–§4.2, appendix B.3). It
//! resolves every name, checks types, owners and states, and attaches every
//! rule's read-back. Every check is fixed; no model decides anything (D4).

use crate::ast::{self, CmpOp, Head, Literal, Loc, NounPhrase, Source, Term, TypeName};
use crate::ir::{
    Action, Arith, Attr, Body, Cond, Define, Effect, Goal, Kind, Lit, Need, Num, Op, OwnerRef, Path, Program, Reaction,
    Rule, Slot, Step, Ty, CREATED, TIMED_OUT,
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

/// `Refunds`, `Boxes`, `Branches`, `Countries`: the regular plural (spec §3.1).
fn plural_of(name: &str) -> String {
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
            let answered = p.effect_of(&at).is_some_and(|e| e.data.iter().any(|(d, _)| d == attr));
            answered.then(|| format!("{attr}, which an answer sets later"))
        }
        Num::Sum { of, kind: k, .. } | Num::Count { of, kind: k, .. } if p.lists.iter().all(|l| l.element != *k) => {
            let owner = end_kind(p, kind, &of.steps).unwrap_or_else(|| kind.to_string());
            let plural = p.kind(k).map_or_else(|| k.clone(), |x| x.plural.clone());
            Some(format!("its {plural}, which do not exist yet when {} is created", a(&owner)))
        }
        Num::Sum { .. } | Num::Count { .. } => None,
    }
}

/// The kind a walk of parts, slots and owner steps ends at, from `kind`.
fn end_kind(p: &Program, kind: &str, steps: &[Step]) -> Option<String> {
    let mut cur = kind.to_string();
    for s in steps {
        cur = match s {
            Step::Part(k) => k.clone(),
            Step::Owner => p.kind(&cur)?.owner.as_ref()?.kind.clone(),
            Step::Slot(_) | Step::Attr(_) | Step::Ref(_) => return None,
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
                '*' => format!("cannot multiply a {lt:?} by a {rt:?}"),
                _ => format!("cannot divide a {lt:?} by a {rt:?}"),
            })
        }
    })
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

/// Whether a path goes through a reference, to another unit's carried facts.
fn through_ref(path: &Path) -> bool {
    path.steps.iter().any(|s| matches!(s, Step::Ref(_)))
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
        TypeName::Kind(_) | TypeName::List(_) | TypeName::Set(_) => None,
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
        self.refs(src, &mut p);
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
                if a.name == CREATED {
                    self.errors.push(err(&k.at, format!("{}.{CREATED}: every entity has `{CREATED}`, the time it was created", k.name)));
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
                attrs.push(Attr { name: a.name.clone(), ty, of: None, default, formula: None });
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
            Literal::Duration(secs) => (Lit::Duration(*secs), Ty::Duration),
            Literal::Set(_) => {
                self.errors.push(err(at, "a set of names is written only in scenarios"));
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
                kind.attrs.push(Attr { name: a.name.clone(), ty, of: Some(target.clone()), default: None, formula: None });
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
                let Some((num, ty)) = self.num(p, &k.at, &k.name, f) else { continue };
                if ty != want {
                    self.errors.push(err(&k.at, format!("{}.{} is {want:?}, but its formula gives a {ty:?}", k.name, a.name)));
                    continue;
                }
                if let Some(why) = not_known_at_creation(p, &k.name, &num) {
                    self.errors.push(err(&k.at, format!("{}.{} reads {why}; a formula is computed when the entity is created", k.name, a.name)));
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
                    None => kind.attrs.push(Attr { name: name.clone(), ty, of: None, default: None, formula: None }),
                }
                data.push((name.clone(), ty));
            }
            let mut sends = vec![];
            for np in &e.sends {
                match self.np(p, &e.at, &e.kind, np) {
                    Some((path, End::Value(_) | End::Ref(_) | End::Set(_))) => sends.push((np.steps.last().cloned().unwrap_or_default(), path)),
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
                    Some((target, End::Node(k) | End::Ref(k))) if through_ref(&target) || matches!(target.steps.last(), Some(Step::Attr(_))) => {
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
                    Some((_, End::Value(_) | End::Set(_) | End::Ref(_))) => {
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
                (path, End::Ref(_)) => Some((Num::Path(path), Ty::Ref)),
                (path, End::Set(_)) => Some((Num::Path(path), Ty::Set)),
                (_, End::Node(k)) => {
                    self.errors.push(err(at, format!("`{}` is {}, not a value", show(np), a(&k))));
                    None
                }
            },
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
                let names = l.singulars();
                let (calc, ty) = self.over(p, at, from, &element, &names, calc)?;
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
        if let Head::Each(k) = &np.head {
            if k != from {
                self.errors.push(err(at, format!("`each {k}`: this sentence is about {}", a(from))));
                return None;
            }
        }
        if let Head::The(k) = &np.head {
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
        for (i, w) in np.steps.iter().enumerate() {
            let next = np.steps.get(i + 1);
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
                    (ty, None) => {
                        steps.push(Step::Attr(w.clone()));
                        return Some((Path { steps }, End::Value(ty)));
                    }
                }
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
        let bools: Vec<String> = p.kind(k).map(|x| x.attrs.iter().filter(|a| a.ty == Ty::Bool).map(|a| a.name.clone()).collect()).unwrap_or_default();
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
        self.errors.push(err(at, format!("a {kind} has no state `{s}`; its states are {}", states.join(", "))));
        None
    }
}
