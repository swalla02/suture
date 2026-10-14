mod common;

use common::*;
use suture::*;

#[test]
fn new_effects_are_taken_once_in_hole_order() {
    let o = orders();
    let mut m = Machine::new(o.prog.clone());
    place(&mut m, &o, 0, 3);
    let taken = m.take_new_effects();
    let holes: Vec<HoleId> = taken.iter().map(|e| e.hole).collect();
    let mut sorted = holes.clone();
    sorted.sort();
    assert_eq!(holes, sorted);
    assert_eq!(taken.len(), 3);
    assert!(taken.iter().all(|e| e.kind == o.capture));
    assert!(m.take_new_effects().is_empty(), "already taken");
    assert_eq!(m.outbox(), taken, "taken effects stay pending until answered");
    m.check_invariants().unwrap();
}

#[test]
fn an_effect_answered_before_it_is_taken_is_skipped() {
    let o = orders();
    let mut m = Machine::new(o.prog.clone());
    place(&mut m, &o, 0, 3);
    let first = m.outbox()[0].hole;
    answer(&mut m, &o, first);
    let before = m.stats().effects_examined;
    let taken = m.take_new_effects();
    assert_eq!(taken.len(), 2);
    assert!(taken.iter().all(|e| e.hole != first));
    assert_eq!(m.stats().effects_examined - before, 3, "the stale entry is examined, then dropped");
    m.check_invariants().unwrap();
}

#[test]
fn outbox_lists_pending_effects_in_hole_order() {
    let o = orders();
    let mut m = Machine::new(o.prog.clone());
    place(&mut m, &o, 0, 50);
    for e in m.outbox().into_iter().step_by(7) {
        answer(&mut m, &o, e.hole);
    }
    let scanned: Vec<HoleId> = m
        .graph()
        .holes_sorted()
        .into_iter()
        .filter(|(_, h)| matches!(h.origin, HoleOrigin::Effect { .. }))
        .map(|(id, _)| id)
        .collect();
    let listed: Vec<HoleId> = m.outbox().iter().map(|e| e.hole).collect();
    assert_eq!(listed, scanned);
    assert_eq!(listed.len(), 50 - 8);
    m.check_invariants().unwrap();
}

#[test]
fn recovery_queues_every_pending_effect_again() {
    let o = orders();
    let mut m = Machine::new(o.prog.clone());
    place(&mut m, &o, 0, 3);
    let pending = m.take_new_effects();
    answer(&mut m, &o, pending[0].hole);
    m.commit().unwrap();
    let mut r = Machine::recover(o.prog.clone(), m.into_journal()).unwrap();
    let again = r.take_new_effects();
    assert_eq!(again, pending[1..].to_vec(), "the host must dispatch these again after a crash");
    assert_eq!(r.outbox(), again);
    r.check_invariants().unwrap();
}

#[test]
fn a_host_that_never_takes_keeps_the_queue_bounded() {
    let o = orders();
    let mut m = Machine::new(o.prog.clone());
    for id in 0..5_000 {
        place(&mut m, &o, id, 1);
        let hole = m.outbox()[0].hole;
        answer(&mut m, &o, hole);
    }
    assert!(m.outbox().is_empty());
    let before = m.stats().effects_examined;
    assert!(m.take_new_effects().is_empty());
    assert!(m.stats().effects_examined - before < 2_000, "the queue must not keep answered effects");
}
