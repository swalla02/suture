//! The choice point (spec §5.7, appendix D.3.4). Every pick of a slot's
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
