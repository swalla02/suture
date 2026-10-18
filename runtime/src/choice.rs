//! The choice point (spec §5.7, appendix D.3.4, F.2.7, J). A slot's
//! alternative is picked here by `prefer`. Shared entities are ranked here by
//! `prefer`; a `decide … with model` may pick another of the ranked options
//! (appendix J), and the rules hold whatever is picked, because every take
//! is checked where it lands.

use std::collections::{BTreeMap, VecDeque};

use suture_lang::ir::Slot;

use crate::state::Val;

/// The alternative to try: the first one in `prefer` order, or the one after
/// `current`. `None` when no alternative is left.
pub fn choose(slot: &Slot, current: Option<&str>) -> Option<String> {
    let next = match current {
        None => 0,
        Some(c) => slot.alternatives.iter().position(|a| a == c)? + 1,
    };
    slot.alternatives.get(next).cloned()
}

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

/// What a model is given (appendix J.2.4).
#[derive(Clone, Debug)]
pub struct Choice {
    /// The choice point: the `uses` target.
    pub point: String,
    /// The unit that chooses, and the binding's key there.
    pub unit: String,
    pub key: String,
    /// The options, best first under `prefer`.
    pub options: Vec<Option_>,
}

/// One option: a candidate's name and attributes.
#[derive(Clone, Debug)]
pub struct Option_ {
    pub label: String,
    pub attrs: Vec<(String, Val)>,
}

/// A decision model: it picks one option by name, or declines.
pub type Model = Box<dyn FnMut(&Choice) -> Option<String>>;

/// The host's models by `name@version` (appendix J.2.7): functions, and
/// scripted stand-ins whose picks a scenario queues.
#[derive(Default)]
pub struct Models {
    funcs: BTreeMap<String, Model>,
    scripted: BTreeMap<String, VecDeque<String>>,
}

/// What a model did with a choice.
pub enum Answer {
    Picked(String),
    Declined,
    Unknown,
}

impl Models {
    pub fn register(&mut self, name: &str, f: Model) {
        self.funcs.insert(name.to_string(), f);
    }

    /// Queues picks for a stand-in: each choice it handles takes the next.
    pub fn script(&mut self, name: &str, picks: impl IntoIterator<Item = String>) {
        self.scripted.entry(name.to_string()).or_default().extend(picks);
    }

    pub fn ask(&mut self, name: &str, c: &Choice) -> Answer {
        if let Some(f) = self.funcs.get_mut(name) {
            return f(c).map_or(Answer::Declined, Answer::Picked);
        }
        match self.scripted.get_mut(name) {
            Some(q) => q.pop_front().map_or(Answer::Declined, Answer::Picked),
            None => Answer::Unknown,
        }
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
