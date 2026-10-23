//! The work one input costs, counted, as the number of units grows
//! (spec appendix BA). Each case builds a world of `n` units, then counts
//! the units read and the steps taken by one more input.

use suture_lang::ir::Program;
use suture_lang::scenario::Caller;
use suture_runtime::host::Host;
use suture_runtime::state::{Val, Work};

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
    let (d0, s0) = Work::now();
    input(h);
    let (d1, s1) = Work::now();
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

const POOL: &str = "module stock version 1, currency EUR
role staff
kind Title { name: Text, stock: Count }
kind Loan  { title: Text, copies: Count = 1 }
kind Hold  { loan: Loan }
Loan needs Hold
Hold uses a Title where the Title's name is the Loan's title, taking the Loan's copies
rule stock: the total copies taken from each Title is at most its stock
admit lend a new Loan by staff
";

#[test]
fn a_pool_whose_first_candidates_are_full_still_grants() {
    let prog = suture_lang::compile(&[("t.st".to_string(), POOL.to_string())]).unwrap_or_else(|e| panic!("{e:?}"));
    let mut h = Host::new(prog);
    // More sold-out Titles than a choice gathers, then one with stock.
    for i in 0..600 {
        h.create("Title", &format!("T{i}"), &[("name".into(), Val::Text("rust".into())), ("stock".into(), Val::Count(i64::from(i == 599)))]).unwrap();
    }
    h.create("Loan", "L1", &[("title".into(), Val::Text("rust".into()))]).unwrap();
    let hold = format!("{:?}", h.unit("L1").unwrap().root.parts[0].binds);
    assert!(hold.contains("T599"), "{hold}\n{}", h.trace.join("\n"));
    // The next Loan finds none left and waits, without filling its unit.
    h.create("Loan", "L2", &[("title".into(), Val::Text("rust".into()))]).unwrap();
    assert!(h.trace.iter().any(|l| l.contains("L2") && l.contains("wait")), "{}", h.trace.join("\n"));
}

const TALLY: &str = "module tally version 1, currency EUR
role staff
kind Vote { weight: Count }
rule cap: the total weight of all Votes is at most 1000000, checked globally
admit cast a new Vote by staff
";

#[test]
fn a_take_from_a_global_total_does_not_grow_with_its_readers() {
    let [small, large] = [100, 1000].map(|n| {
        let prog = suture_lang::compile(&[("t.st".to_string(), TALLY.to_string())]).unwrap_or_else(|e| panic!("{e:?}"));
        let mut h = Host::new(prog);
        for i in 0..n {
            h.create("Vote", &format!("V{i}"), &[("weight".into(), Val::Count(1))]).unwrap();
        }
        cost(&mut h, |h| h.create("Vote", "X", &[("weight".into(), Val::Count(1))]).unwrap())
    });
    assert_eq!(small, large, "(units read, steps) at 100 votes, at 1000");
}

#[test]
fn a_take_from_shared_stock_does_not_grow_with_its_holders() {
    let [small, large] = [100, 1000].map(|n| {
        let prog = suture_lang::compile(&[("t.st".to_string(), POOL.to_string())]).unwrap_or_else(|e| panic!("{e:?}"));
        let mut h = Host::new(prog);
        h.create("Title", "T", &[("name".into(), Val::Text("rust".into())), ("stock".into(), Val::Count(1_000_000))]).unwrap();
        for i in 0..n {
            h.create("Loan", &format!("L{i}"), &[("title".into(), Val::Text("rust".into()))]).unwrap();
        }
        cost(&mut h, |h| h.create("Loan", "X", &[("title".into(), Val::Text("rust".into()))]).unwrap())
    });
    assert_eq!(small, large, "(units read, steps) at 100 holders, at 1000");
}
