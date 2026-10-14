//! Work-count acceptance tests (language spec §7.4). They count work and
//! never time it.

mod common;

use common::*;
use suture::*;

/// C2: taking new effects examines only the new ones, however many are pending.
#[test]
fn c2_taking_new_effects_ignores_the_backlog() {
    for pending in [0usize, 10_000, 100_000] {
        let o = orders();
        let mut m = Machine::new(o.prog.clone());
        place(&mut m, &o, 0, pending);
        assert_eq!(m.take_new_effects().len(), pending);
        let before = m.stats().effects_examined;
        place(&mut m, &o, pending as i64, 5);
        assert_eq!(m.take_new_effects().len(), 5);
        assert_eq!(m.stats().effects_examined - before, 5, "with {pending} pending");
        assert_eq!(m.outbox().len(), pending + 5);
    }
}
