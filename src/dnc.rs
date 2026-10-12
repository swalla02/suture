//! Decision-Native Computing: decision laws, decision backends, and the
//! resolver (`docs/dnc.md`).
//!
//! A *decision site* is an effect agent of a declared decision law. The rule
//! that creates it has already computed the site's observation and its
//! admissible alternatives from local data. The resolver asks a backend to
//! pick one alternative per site, in batches, and answers each site by
//! injecting a typed `Decided` token at its hole. An ordinary deterministic
//! rule consumes the token and enacts the selected transformation. Tokens are
//! journaled injects, so replay never calls the model again.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::id::HoleId;
use crate::machine::{EffectView, InjectError, Machine};
use crate::net::{Injection, NetBuilder};
use crate::rng::Rng;
use crate::sig::KindId;
use crate::value::{Value, ValueError, ValueType};
use crate::tup;

/// Maps a site payload to one feature vector per admissible alternative.
pub type Encoder = Arc<dyn Fn(&Value) -> Result<Vec<Vec<f64>>, String> + Send + Sync>;

/// A decision law: which effect kind is a decision site, which kind is its
/// token, and how its alternatives are presented to a model.
#[derive(Clone)]
pub struct Law {
    pub name: String,
    pub site: KindId,
    pub token: KindId,
    pub encode: Encoder,
}

impl Law {
    pub fn new(name: &str, site: KindId, token: KindId, encode: Encoder) -> Law {
        Law { name: name.to_string(), site, token, encode }
    }
}

#[derive(Clone, Debug)]
pub struct DecisionRequest {
    pub law: String,
    /// The site's outbox hole: a stable identity, usable as a trace key.
    pub site: HoleId,
    /// The site payload as built by the rule (observation and alternatives).
    pub observation: Value,
    /// One feature vector per admissible alternative.
    pub features: Vec<Vec<f64>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DecisionResult {
    pub selected: usize,
    pub scores: Option<Vec<f64>>,
    pub model: String,
    pub trace: String,
}

#[derive(Debug, thiserror::Error)]
pub enum DecisionError {
    #[error("decision service unavailable: {0}")]
    Unavailable(String),
    #[error("decision service timed out")]
    Timeout,
}

/// A model-independent decision service. One call answers a whole batch.
pub trait DecisionBackend {
    fn model(&self) -> String;
    fn decide(&mut self, batch: &[DecisionRequest]) -> Result<Vec<DecisionResult>, DecisionError>;
}

/// Payload schema of a decision token: `(selected, model, trace, scores × 1000)`.
pub fn decided_schema() -> ValueType {
    ValueType::Tuple(vec![ValueType::Int, ValueType::Str, ValueType::Str, ValueType::List(Box::new(ValueType::Int))])
}

/// The alternative a decision token selects.
pub fn selected_index(token: &Value) -> Result<usize, ValueError> {
    let i = token.int(0)?;
    usize::try_from(i).map_err(|_| ValueError { value: token.to_string(), reason: "negative selection".into() })
}

fn token_payload(r: &DecisionResult) -> Value {
    let scores = r.scores.iter().flatten().map(|s| Value::Int((s * 1000.0).round() as i64));
    tup![r.selected as i64, r.model.as_str(), r.trace.as_str(), Value::list(scores)]
}

fn argmax(scores: &[f64]) -> usize {
    let mut best = 0;
    for (i, s) in scores.iter().enumerate() {
        if *s > scores[best] {
            best = i;
        }
    }
    best
}

/// Always the first alternative: the deterministic list-order policy, and
/// the fallback for invalid or unavailable decisions.
#[derive(Clone, Copy, Debug, Default)]
pub struct FirstBackend;

impl DecisionBackend for FirstBackend {
    fn model(&self) -> String {
        "first".into()
    }
    fn decide(&mut self, batch: &[DecisionRequest]) -> Result<Vec<DecisionResult>, DecisionError> {
        Ok(batch.iter().map(|_| DecisionResult { selected: 0, scores: None, model: self.model(), trace: String::new() }).collect())
    }
}

/// Uniformly random among the admissible alternatives (seeded).
#[derive(Clone, Debug)]
pub struct RandomBackend {
    rng: Rng,
    seed: u64,
}

impl RandomBackend {
    pub fn new(seed: u64) -> Self {
        RandomBackend { rng: Rng::new(seed), seed }
    }
}

impl DecisionBackend for RandomBackend {
    fn model(&self) -> String {
        format!("random({})", self.seed)
    }
    fn decide(&mut self, batch: &[DecisionRequest]) -> Result<Vec<DecisionResult>, DecisionError> {
        let model = self.model();
        Ok(batch
            .iter()
            .map(|r| DecisionResult { selected: self.rng.index(r.features.len()), scores: None, model: model.clone(), trace: String::new() })
            .collect())
    }
}

/// A linear scoring law per decision law: `score = w · features`, argmax,
/// ties to the lowest index. Shared by every site of the law.
#[derive(Clone, Debug)]
pub struct LinearBackend {
    version: String,
    weights: BTreeMap<String, Vec<f64>>,
}

impl LinearBackend {
    pub fn new(version: &str) -> Self {
        LinearBackend { version: version.to_string(), weights: BTreeMap::new() }
    }
    pub fn with(mut self, law: &str, weights: Vec<f64>) -> Self {
        self.weights.insert(law.to_string(), weights);
        self
    }
    pub fn weights(&self, law: &str) -> Option<&[f64]> {
        self.weights.get(law).map(Vec::as_slice)
    }
}

impl DecisionBackend for LinearBackend {
    fn model(&self) -> String {
        self.version.clone()
    }
    fn decide(&mut self, batch: &[DecisionRequest]) -> Result<Vec<DecisionResult>, DecisionError> {
        Ok(batch
            .iter()
            .map(|r| {
                let w = self.weights.get(&r.law).map(Vec::as_slice).unwrap_or(&[]);
                let scores: Vec<f64> = r.features.iter().map(|f| f.iter().zip(w).map(|(x, w)| x * w).sum()).collect();
                DecisionResult { selected: argmax(&scores), scores: Some(scores), model: self.version.clone(), trace: String::new() }
            })
            .collect())
    }
}

/// Replaces each decision of `inner` by a uniformly random one with
/// probability `eps`: uncertain or contradictory decisions.
pub struct NoisyBackend<B> {
    inner: B,
    eps: f64,
    rng: Rng,
}

impl<B: DecisionBackend> NoisyBackend<B> {
    pub fn new(inner: B, eps: f64, seed: u64) -> Self {
        NoisyBackend { inner, eps, rng: Rng::new(seed) }
    }
}

impl<B: DecisionBackend> DecisionBackend for NoisyBackend<B> {
    fn model(&self) -> String {
        format!("noisy({}, {})", self.inner.model(), self.eps)
    }
    fn decide(&mut self, batch: &[DecisionRequest]) -> Result<Vec<DecisionResult>, DecisionError> {
        let mut out = self.inner.decide(batch)?;
        for (r, req) in out.iter_mut().zip(batch) {
            if self.rng.chance(self.eps) {
                r.selected = self.rng.index(req.features.len());
                r.scores = None;
            }
        }
        Ok(out)
    }
}

/// Counters for the decision layer (directive §10F: calls and their cost).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResolveStats {
    /// Sites answered.
    pub sites: u64,
    /// Sites answered by the backend (two or more alternatives).
    pub decisions: u64,
    /// Sites with a single alternative, answered without a model call.
    pub trivial: u64,
    /// Batched backend calls.
    pub calls: u64,
    /// Selections out of range, replaced by the fallback.
    pub invalid: u64,
    /// Decisions answered by the fallback because the backend failed.
    pub fallback: u64,
    /// Decisions whose score margin was below the threshold.
    pub low_confidence: u64,
    /// Sites whose payload the law could not encode.
    pub encode_errors: u64,
}

pub struct Resolver {
    laws: Vec<Law>,
    min_margin: Option<f64>,
    pub stats: ResolveStats,
}

fn fallback(reason: &str) -> DecisionResult {
    DecisionResult { selected: 0, scores: None, model: format!("fallback:{reason}"), trace: String::new() }
}

impl Resolver {
    pub fn new(laws: Vec<Law>) -> Self {
        Resolver { laws, min_margin: None, stats: ResolveStats::default() }
    }

    /// Decisions whose top score beats the runner-up by less than `margin`
    /// are answered by the fallback instead.
    pub fn with_min_margin(mut self, margin: f64) -> Self {
        self.min_margin = Some(margin);
        self
    }

    pub fn laws(&self) -> &[Law] {
        &self.laws
    }

    fn law_for(&self, kind: KindId) -> Option<&Law> {
        self.laws.iter().find(|l| l.site == kind)
    }

    /// Pending decision sites, in outbox order.
    pub fn pending(&self, m: &Machine) -> Vec<EffectView> {
        m.outbox().into_iter().filter(|e| self.law_for(e.kind).is_some()).collect()
    }

    /// Resolves every pending site with one batched backend call.
    pub fn resolve(&mut self, m: &mut Machine, backend: &mut dyn DecisionBackend) -> Result<usize, InjectError> {
        let sites = self.pending(m);
        self.resolve_sites(m, backend, &sites)
    }

    fn low_confidence(&self, r: &DecisionResult) -> bool {
        let (Some(margin), Some(scores)) = (self.min_margin, &r.scores) else { return false };
        if scores.len() < 2 {
            return false;
        }
        let mut sorted = scores.clone();
        sorted.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
        sorted[0] - sorted[1] < margin
    }

    /// Resolves `sites` (in the given order) with one batched backend call,
    /// then injects one token per site in that order.
    pub fn resolve_sites(
        &mut self,
        m: &mut Machine,
        backend: &mut dyn DecisionBackend,
        sites: &[EffectView],
    ) -> Result<usize, InjectError> {
        let mut chosen: Vec<Option<(KindId, DecisionResult)>> = vec![None; sites.len()];
        let (mut batch, mut index, mut tokens) = (vec![], vec![], vec![]);
        for (i, s) in sites.iter().enumerate() {
            let Some(law) = self.law_for(s.kind).cloned() else { continue };
            let features = match (law.encode)(&s.payload) {
                Ok(f) if !f.is_empty() => f,
                _ => {
                    self.stats.encode_errors += 1;
                    continue;
                }
            };
            self.stats.sites += 1;
            if features.len() == 1 {
                self.stats.trivial += 1;
                chosen[i] = Some((law.token, DecisionResult { model: "trivial".into(), ..fallback("") }));
                continue;
            }
            batch.push(DecisionRequest { law: law.name.clone(), site: s.hole, observation: s.payload.clone(), features });
            index.push(i);
            tokens.push(law.token);
        }
        if !batch.is_empty() {
            self.stats.calls += 1;
            self.stats.decisions += batch.len() as u64;
            match backend.decide(&batch) {
                Ok(results) if results.len() == batch.len() => {
                    for (j, r) in results.into_iter().enumerate() {
                        let r = if r.selected >= batch[j].features.len() {
                            self.stats.invalid += 1;
                            fallback("invalid")
                        } else if self.low_confidence(&r) {
                            self.stats.low_confidence += 1;
                            fallback("low-confidence")
                        } else {
                            r
                        };
                        chosen[index[j]] = Some((tokens[j], r));
                    }
                }
                _ => {
                    self.stats.fallback += batch.len() as u64;
                    for (j, &i) in index.iter().enumerate() {
                        chosen[i] = Some((tokens[j], fallback("unavailable")));
                    }
                }
            }
        }
        let mut done = 0;
        for (s, c) in sites.iter().zip(chosen) {
            let Some((token, mut r)) = c else { continue };
            r.trace = format!("{:032x}", s.hole.0);
            let mut b = NetBuilder::new(&m.program().sig);
            let v = b.var();
            b.agent(token, token_payload(&r), v, &[]);
            let inj = Injection::new(b.finish()?).at_hole(v, s.hole).key(format!("decide:{:032x}", s.hole.0));
            m.inject(inj)?;
            done += 1;
        }
        Ok(done)
    }
}
