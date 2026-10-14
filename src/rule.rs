//! Interaction rules and the rule table.
//!
//! A rule is code: a deterministic function of the active pair's kinds and
//! payloads that *builds* the replacement net through [`RuleCx`]. It never
//! touches the graph. The machine validates every patch it produces before
//! installation, so a buggy rule can fault a pair but cannot corrupt the graph.

use std::collections::hash_map::Entry;
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::check::NetError;
use crate::id::DigestBuilder;
use crate::net::{NetBuilder, NetData, Var};
use crate::sig::{KindId, Signature};
use crate::value::{Value, ValueError};

/// How replay treats a rule's output.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuleMode {
    /// Pure function of the pair: replay re-runs it and verifies the patch digest.
    Deterministic,
    /// Opaque (e.g. a learned law): the patch itself is journaled and replay
    /// applies the recorded patch without re-running the rule.
    Recorded,
}

pub type RuleBody = dyn Fn(&mut RuleCx<'_>) -> Result<(), RuleError> + Send + Sync;

pub struct Rule {
    pub name: String,
    pub version: u32,
    pub left: KindId,
    pub right: KindId,
    pub mode: RuleMode,
    pub may_emit: Vec<KindId>,
    pub body: Arc<RuleBody>,
}

impl Rule {
    pub fn version(&mut self, v: u32) -> &mut Self {
        self.version = v;
        self
    }
    pub fn recorded(&mut self) -> &mut Self {
        self.mode = RuleMode::Recorded;
        self
    }
    /// Grants authority to emit the given effect kinds.
    pub fn emits(&mut self, kinds: &[KindId]) -> &mut Self {
        self.may_emit.extend_from_slice(kinds);
        self
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RuleError {
    #[error("rejected: {0}")]
    Reject(String),
    #[error(transparent)]
    Net(#[from] NetError),
    #[error(transparent)]
    Value(#[from] ValueError),
}

/// The context a rule body runs in: read access to the pair, write access to
/// the patch under construction. Interface variables are numbered left
/// auxiliary ports first, then right auxiliary ports.
pub struct RuleCx<'a> {
    sig: &'a Signature,
    left_kind: KindId,
    right_kind: KindId,
    left: Value,
    right: Value,
    builder: NetBuilder<'a>,
    n_left: usize,
}

impl<'a> RuleCx<'a> {
    pub(crate) fn new(sig: &'a Signature, left_kind: KindId, left: Value, right_kind: KindId, right: Value) -> Self {
        let n_left = sig.arity(left_kind);
        let n = n_left + sig.arity(right_kind);
        RuleCx { sig, left_kind, right_kind, left, right, builder: NetBuilder::with_vars(sig, n as u32), n_left }
    }

    pub fn left(&self) -> &Value {
        &self.left
    }
    pub fn right(&self) -> &Value {
        &self.right
    }
    pub fn left_kind(&self) -> KindId {
        self.left_kind
    }
    pub fn right_kind(&self) -> KindId {
        self.right_kind
    }
    pub fn sig(&self) -> &Signature {
        self.sig
    }

    fn aux<const N: usize>(&mut self, kind: KindId, offset: usize) -> [Var; N] {
        let expected = self.sig.arity(kind);
        if expected != N {
            self.builder.fail(NetError::Arity { kind: self.sig.kind(kind).name.clone(), expected, got: N });
        }
        std::array::from_fn(|i| Var((offset + i) as u32))
    }

    /// Interface variables standing for the left agent's auxiliary ports.
    pub fn left_aux<const N: usize>(&mut self) -> [Var; N] {
        self.aux(self.left_kind, 0)
    }

    /// Interface variables standing for the right agent's auxiliary ports.
    pub fn right_aux<const N: usize>(&mut self) -> [Var; N] {
        self.aux(self.right_kind, self.n_left)
    }

    pub fn var(&mut self) -> Var {
        self.builder.var()
    }

    pub fn agent(&mut self, kind: KindId, payload: impl Into<Value>, at: Var, aux: &[Var]) {
        self.builder.agent(kind, payload, at, aux)
    }

    pub fn effect(&mut self, kind: KindId, payload: impl Into<Value>, aux: &[Var]) {
        self.builder.effect(kind, payload, aux)
    }

    pub fn link(&mut self, a: Var, b: Var) {
        self.builder.link(a, b)
    }

    pub fn reject(&self, why: impl Into<String>) -> RuleError {
        RuleError::Reject(why.into())
    }

    pub(crate) fn finish(self) -> Result<NetData, NetError> {
        self.builder.finish()
    }
}

/// Rules indexed by unordered pair of kinds; at most one rule per pair.
#[derive(Default)]
pub struct RuleTable {
    rules: Vec<Rule>,
    index: HashMap<(KindId, KindId), usize>,
    names: HashMap<String, usize>,
    forbidden: BTreeSet<(KindId, KindId)>,
    errors: Vec<String>,
}

fn key(a: KindId, b: KindId) -> (KindId, KindId) {
    if a <= b { (a, b) } else { (b, a) }
}

/// Result of [`RuleTable::coverage`]: pairs of kinds whose principal ports
/// can meet but have no rule, pairs explicitly forbidden, and rules whose
/// principal ports can never meet.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Coverage {
    pub missing: Vec<(String, String)>,
    pub forbidden: Vec<(String, String)>,
    pub never_fires: Vec<String>,
}

impl RuleTable {
    pub fn new() -> Self {
        Self::default()
    }

    /// Defines the rule for `left >< right`. Duplicate pairs or names are
    /// recorded in [`RuleTable::errors`] and reported by `Program::new`.
    pub fn def(
        &mut self,
        name: &str,
        left: KindId,
        right: KindId,
        body: impl Fn(&mut RuleCx<'_>) -> Result<(), RuleError> + Send + Sync + 'static,
    ) -> &mut Rule {
        let idx = self.rules.len();
        match self.index.entry(key(left, right)) {
            Entry::Vacant(e) => {
                e.insert(idx);
            }
            Entry::Occupied(_) => {
                self.errors.push(format!("rule {name}: kinds {left:?} and {right:?} already have a rule"));
            }
        }
        if self.names.insert(name.to_string(), idx).is_some() {
            self.errors.push(format!("duplicate rule name {name}"));
        }
        self.rules.push(Rule {
            name: name.to_string(),
            version: 1,
            left,
            right,
            mode: RuleMode::Deterministic,
            may_emit: vec![],
            body: Arc::new(body),
        });
        &mut self.rules[idx]
    }

    /// Declares that `a >< b` intentionally has no rule.
    pub fn forbid(&mut self, a: KindId, b: KindId) {
        self.forbidden.insert(key(a, b));
    }

    /// The rule for a pair, with `swapped == true` when the pair's first
    /// agent plays the rule's right-hand role.
    pub fn lookup(&self, a: KindId, b: KindId) -> Option<(usize, &Rule, bool)> {
        let idx = *self.index.get(&key(a, b))?;
        let rule = &self.rules[idx];
        Some((idx, rule, rule.left != a))
    }

    pub fn get(&self, idx: usize) -> &Rule {
        &self.rules[idx]
    }

    pub fn by_name(&self, name: &str) -> Option<(usize, &Rule)> {
        let idx = *self.names.get(name)?;
        Some((idx, &self.rules[idx]))
    }

    pub fn len(&self) -> usize {
        self.rules.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    pub fn errors(&self) -> &[String] {
        &self.errors
    }

    /// Feeds the table's structure into a program fingerprint: rule names,
    /// kind pairs, modes, effect authority and forbidden pairs. Versions and
    /// rule code are left out, because replay checks them step by step.
    pub(crate) fn encode_structure(&self, d: &mut DigestBuilder) {
        d.u64(self.rules.len() as u64);
        for r in &self.rules {
            d.str(&r.name);
            d.u32(r.left.0 as u32);
            d.u32(r.right.0 as u32);
            d.u8(matches!(r.mode, RuleMode::Recorded) as u8);
            d.u64(r.may_emit.len() as u64);
            for k in &r.may_emit {
                d.u32(k.0 as u32);
            }
        }
        d.u64(self.forbidden.len() as u64);
        for (a, b) in &self.forbidden {
            d.u32(a.0 as u32);
            d.u32(b.0 as u32);
        }
    }

    pub fn coverage(&self, sig: &Signature) -> Coverage {
        let kinds: Vec<_> = sig.kinds().collect();
        let name = |k: KindId| sig.kind(k).name.clone();
        let mut c = Coverage::default();
        for (i, (a, da)) in kinds.iter().enumerate() {
            for (b, db) in &kinds[i..] {
                if !sig.compatible(da.principal, db.principal) {
                    continue;
                }
                let k = key(*a, *b);
                if self.index.contains_key(&k) {
                    continue;
                }
                let pair = (name(k.0), name(k.1));
                if self.forbidden.contains(&k) {
                    c.forbidden.push(pair);
                } else {
                    c.missing.push(pair);
                }
            }
        }
        for r in &self.rules {
            if !sig.compatible(sig.kind(r.left).principal, sig.kind(r.right).principal) {
                c.never_fires.push(r.name.clone());
            }
        }
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::fixtures::fx;
    use crate::net::Var;
    use crate::{tup, Value};

    fn noop(_: &mut RuleCx<'_>) -> Result<(), RuleError> {
        Ok(())
    }

    #[test]
    fn lookup_orients() {
        let f = fx();
        let mut t = RuleTable::new();
        t.def("add_s", f.s, f.add, noop);
        let (_, r, swapped) = t.lookup(f.add, f.s).unwrap();
        assert_eq!(r.name, "add_s");
        assert!(swapped);
        assert!(!t.lookup(f.s, f.add).unwrap().2);
        assert!(t.lookup(f.z, f.add).is_none());
        assert_eq!(t.by_name("add_s").unwrap().0, 0);
        assert!(t.errors().is_empty());
    }

    #[test]
    fn duplicates_are_recorded() {
        let f = fx();
        let mut t = RuleTable::new();
        t.def("a", f.s, f.add, noop);
        t.def("b", f.add, f.s, noop);
        t.def("a", f.z, f.add, noop);
        assert_eq!(t.errors().len(), 2, "{:?}", t.errors());
    }

    #[test]
    fn rule_options() {
        let f = fx();
        let mut t = RuleTable::new();
        t.def("ask", f.z, f.ask, noop).version(3).recorded().emits(&[f.ask]);
        let (_, r, _) = t.lookup(f.z, f.ask).unwrap();
        assert_eq!((r.version, r.mode, r.may_emit.clone()), (3, RuleMode::Recorded, vec![f.ask]));
    }

    #[test]
    fn coverage_reports_missing_forbidden_and_never_fires() {
        let f = fx();
        let mut t = RuleTable::new();
        t.def("add_z", f.z, f.add, noop);
        t.def("add_s", f.s, f.add, noop);
        t.def("era_s", f.s, f.era, noop);
        t.def("z_s", f.z, f.s, noop); // both principals are Nat+: can never meet
        let c = t.coverage(&f.sig);
        assert!(c.missing.contains(&("Z".to_string(), "Era".to_string())));
        assert_eq!(c.missing.len(), 9 - 3);
        assert_eq!(c.never_fires, vec!["z_s".to_string()]);
        t.forbid(f.era, f.z);
        let c = t.coverage(&f.sig);
        assert!(!c.missing.contains(&("Z".to_string(), "Era".to_string())));
        assert_eq!(c.forbidden, vec![("Z".to_string(), "Era".to_string())]);
    }

    #[test]
    fn rule_cx_numbers_interface_vars_left_then_right() {
        let f = fx();
        let mut cx = RuleCx::new(&f.sig, f.s, tup![1], f.add, Value::Unit);
        let [x] = cx.left_aux();
        let [y, r] = cx.right_aux();
        assert_eq!((x, y, r), (Var(0), Var(1), Var(2)));
        assert_eq!(cx.left(), &tup![1]);
        let r1 = cx.var();
        assert_eq!(r1, Var(3));
        cx.agent(f.add, (), x, &[y, r1]);
        cx.agent(f.s, (), r, &[r1]);
        let net = cx.finish().unwrap();
        assert_eq!(net.agents.len(), 2);

        let mut cx = RuleCx::new(&f.sig, f.s, Value::Unit, f.add, Value::Unit);
        let [_a, _b] = cx.left_aux(); // S has one auxiliary port
        assert!(matches!(cx.finish(), Err(crate::NetError::Arity { .. })));
    }
}
