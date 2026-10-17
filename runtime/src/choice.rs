//! The choice point (spec §5.7, appendix D.3.4, F.2.7). Every pick of a slot's
//! alternative goes through here. Slice 3 picks by `prefer`; a `decide …
//! with model` backend replaces this function later, and the rules hold
//! whatever it picks, because the interpreter checks every change it makes.

use suture_lang::ir::Slot;

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
