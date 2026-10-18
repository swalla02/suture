//! The choice point (spec §5.7, appendix D.3.4, F.2.7, J, K). Shared
//! entities are ranked here by `prefer`; a `decide … with model` may pick
//! another of the ranked options, a slot's next alternative or the source to
//! ask (appendices J, K), and the rules hold whatever is picked, because
//! every pick is checked where it lands.

use std::collections::{BTreeMap, VecDeque};
use std::time::{Duration, Instant};

use crate::state::Val;

/// The candidate to pick among shared entities (appendix F.2.7): the one
/// whose scores, criterion by criterion, are lowest; ties go to the one
/// gathered first, which is creation order. `None` when there is none.
pub fn pick(scores: &[Vec<(u8, i128)>]) -> Option<usize> {
    (0..scores.len()).min_by(|&a, &b| scores[a].cmp(&scores[b]))
}

/// The candidates' order under `prefer`: best first, ties in the order gathered.
pub fn order(scores: &[Vec<(u8, i128)>]) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..scores.len()).collect();
    idx.sort_by(|&a, &b| scores[a].cmp(&scores[b]));
    idx
}

/// What a model is given (appendix J.2.4, K.2.6).
#[derive(Clone, Debug)]
pub struct Choice {
    /// The choice point: a `uses` target, a slot, or `Order's credit`.
    pub point: String,
    /// The unit that chooses, and the request's key there.
    pub unit: String,
    pub key: String,
    /// The kind of the entity that chooses, and its facts.
    pub chooser: String,
    pub facts: Vec<(String, Val)>,
    /// The options, best first under `prefer`.
    pub options: Vec<Option_>,
    /// The module's currency, to show amounts.
    pub currency: String,
}

/// One option: a candidate's name and attributes, an alternative's kind, or
/// a source's adapter.
#[derive(Clone, Debug)]
pub struct Option_ {
    pub label: String,
    pub attrs: Vec<(String, Val)>,
}

/// A decision model: it picks one option by name, or declines.
pub type Model = Box<dyn FnMut(&Choice) -> Option<String>>;

/// Anything that answers choices: a function, a Jev backend, a custom one
/// (appendix K.2.6). `within` is its deadline; an answer after it is not used.
pub trait Backend {
    fn decide(&mut self, c: &Choice, within: Duration) -> Answer;
}

struct Func(Model);

impl Backend for Func {
    fn decide(&mut self, c: &Choice, _: Duration) -> Answer {
        (self.0)(c).map_or(Answer::Declined, Answer::Picked)
    }
}

/// A scripted stand-in's next answer (appendix K.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Scripted {
    Pick(String),
    Decline,
    Late,
}

/// The host's models by `name@version` (appendix J.2.7): backends, and
/// scripted stand-ins whose answers a scenario queues.
#[derive(Default)]
pub struct Models {
    backends: BTreeMap<String, Box<dyn Backend>>,
    scripted: BTreeMap<String, VecDeque<Scripted>>,
}

/// What a model did with a choice.
#[derive(Clone, Debug, PartialEq)]
pub enum Answer {
    Picked(String),
    Declined,
    Unknown,
    /// It did not answer within its deadline.
    Late,
    /// Its confidence was below the backend's minimum.
    Unsure(f64),
    /// The backend failed: no answer, an error, or an answer it cannot read.
    Failed(String),
}

impl Models {
    pub fn register(&mut self, name: &str, f: Model) {
        self.backends.insert(name.to_string(), Box::new(Func(f)));
    }

    pub fn register_backend(&mut self, name: &str, b: Box<dyn Backend>) {
        self.backends.insert(name.to_string(), b);
    }

    /// Queues picks for a stand-in: each choice it handles takes the next.
    pub fn script(&mut self, name: &str, picks: impl IntoIterator<Item = String>) {
        self.answers(name, picks.into_iter().map(Scripted::Pick));
    }

    /// Queues a stand-in's answers: picks, declines and late answers.
    pub fn answers(&mut self, name: &str, answers: impl IntoIterator<Item = Scripted>) {
        self.scripted.entry(name.to_string()).or_default().extend(answers);
    }

    /// Asks the model `name`, which has `within_ms` to answer. A scripted
    /// answer still queued comes before the backend of that name.
    pub fn ask(&mut self, name: &str, c: &Choice, within_ms: u64) -> Answer {
        let within = Duration::from_millis(within_ms);
        match self.scripted.get_mut(name).and_then(VecDeque::pop_front) {
            Some(Scripted::Pick(p)) => return Answer::Picked(p),
            Some(Scripted::Late) => return Answer::Late,
            Some(Scripted::Decline) => return Answer::Declined,
            None => {}
        }
        if let Some(b) = self.backends.get_mut(name) {
            let start = Instant::now();
            let a = b.decide(c, within);
            return if start.elapsed() > within && matches!(a, Answer::Picked(_)) { Answer::Late } else { a };
        }
        if self.scripted.contains_key(name) { Answer::Declined } else { Answer::Unknown }
    }
}

#[cfg(test)]
mod tests {
    use super::pick;

    #[test]
    fn picks_by_score_then_creation_order() {
        assert_eq!(pick(&[]), None);
        assert_eq!(pick(&[vec![(0, 5)], vec![(0, 3)], vec![(0, 3)]]), Some(1));
        assert_eq!(pick(&[vec![(1, 0)], vec![(0, 9)]]), Some(1), "an unknown value comes last");
        assert_eq!(pick(&[vec![], vec![]]), Some(0));
    }
}
