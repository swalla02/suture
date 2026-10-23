//! The work one input costs, counted, as the number of units grows
//! (spec appendix BA). Each case builds a world of `n` units, then counts
//! the units read and the steps taken by one more input.

use suture_lang::ir::Program;
use suture_lang::scenario::Caller;
use suture_runtime::host::Host;
use suture_runtime::state::{Val, WORK};

const SHOP: &str = "module shop version 1, currency EUR
role staff
kind Member  { name: Text }
kind Class   { seats: Count }
kind Booking { member: Member, class: Class, people: Count, cancelled: Bool = no }
kind Payment { booking: Booking }
Booking needs Payment while it is not cancelled
effect charge Payment via payments within 1 hour
  sends    the booking's people
  reports  paid
rule fits: every Booking's people is at most 4
rule three: the number of each Member's Bookings that are not cancelled is at most 3
rule seats: the total people of each Class's Bookings that are not cancelled is at most 1000000
admit book a new Booking by staff
admit cancel Booking by staff
when a staff cancels a Booking: it is cancelled
";

fn prog() -> Program {
    suture_lang::compile(&[("t.st".to_string(), SHOP.to_string())]).unwrap_or_else(|e| panic!("{e:?}"))
}

/// A host with `n` Members and `n` Bookings spread over 10 Classes.
fn world(n: usize) -> Host {
    let mut h = Host::new(prog());
    for c in 0..10 {
        h.create("Class", &format!("C{c}"), &[("seats".into(), Val::Count(100))]).unwrap();
    }
    for i in 0..n {
        h.create("Member", &format!("M{i}"), &[("name".into(), Val::Text(format!("m{i}")))]).unwrap();
        book(&mut h, &format!("B{i}"), &format!("M{i}"), i % 10);
    }
    h.trace.clear();
    h
}

fn book(h: &mut Host, label: &str, member: &str, class: usize) {
    let given = [("member".into(), Val::Ref(member.into())), ("class".into(), Val::Ref(format!("C{class}"))), ("people".into(), Val::Count(1))];
    h.create("Booking", label, &given).unwrap();
}

/// (units read, steps) one input costs.
fn cost(h: &mut Host, input: impl FnOnce(&mut Host)) -> (u64, u64) {
    let (d0, s0) = WORK.now();
    input(h);
    let (d1, s1) = WORK.now();
    (d1 - d0, s1 - s0)
}

fn costs(input: fn(&mut Host)) -> [(u64, u64); 2] {
    [100, 1000].map(|n| {
        let mut h = world(n);
        cost(&mut h, input)
    })
}

/// One input to a world.
type Input = fn(&mut Host);

// One test, so no other test's work is counted with it. Pools whose first
// candidates are full, and totals read by every unit, are not flat yet
// (spec appendix BA.4).
#[test]
fn the_cost_of_an_input_does_not_grow_with_the_units() {
    let cases: [(&str, Input); 4] = [
        ("a new booking", |h| book(h, "X", "M1", 3)),
        ("an answer", |h| h.answer("payments", "paid", &[], Some("B1")).unwrap()),
        ("a cancel", |h| {
            let by = Caller { role: "staff".into(), name: "Sam".into(), verb: "cancels".into() };
            h.input_event(Some(&by), "B1", None, &[]).unwrap()
        }),
        ("a minute passing, with no timer due", |h| h.pass(60).unwrap()),
    ];
    for (name, input) in cases {
        let [small, large] = costs(input);
        assert_eq!(small, large, "{name}: (units read, steps) {small:?} at 100 units, {large:?} at 1000");
    }
    // Every Payment's request times out after an hour: one step per timer.
    for n in [100, 1000] {
        let mut h = world(n);
        let (_, steps) = cost(&mut h, |h| h.pass(3600).unwrap());
        assert_eq!(steps, n as u64, "{n} timers");
    }
}
