//! Slice 19: the deferred minors (spec appendix T).

use suture_lang::ir::Program;
use suture_lang::scenario::Caller;
use suture_runtime::host::Host;
use suture_runtime::state::Val;

fn prog(src: &str) -> Program {
    suture_lang::compile(&[("app.st".to_string(), src.to_string())]).unwrap_or_else(|e| panic!("{}", e.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("\n")))
}

const NOTES: &str = "module notes version 1, currency EUR
role staff
kind Note { body: Text }
admit write a new Note by staff
";

fn staff(verb: &str) -> Caller {
    Caller { role: "staff".into(), name: "Sam".into(), verb: verb.into() }
}

#[test]
fn a_unit_too_large_for_the_machine_is_refused_before_it_is_made() {
    let mut h = Host::new(prog(NOTES));
    let long = Val::Text("x".repeat(70_000));
    let e = h.input_new(Some(&staff("writes")), "Note", "N1", &[("body".into(), long)], &[]).unwrap_err();
    assert!(e.starts_with("refused: N1 would hold ") && e.ends_with("bytes, and a unit holds at most 65536"), "{e}");
    assert!(h.unit("N1").is_none());
    assert!(h.machine.faults().is_empty());
}

#[test]
fn a_change_that_would_make_a_unit_too_large_is_refused_and_changes_nothing() {
    let p = prog(&format!("{NOTES}admit change Note's body by staff\n"));
    let mut h = Host::new(p);
    h.input_new(Some(&staff("writes")), "Note", "N1", &[("body".into(), Val::Text("short".into()))], &[]).unwrap();
    let before = h.unit("N1");
    let e = h.input_change(Some(&staff("changes")), "N1", "body", Val::Text("x".repeat(70_000))).unwrap_err();
    assert!(e.starts_with("refused: N1 would hold "), "{e}");
    assert_eq!(h.unit("N1"), before);
    assert!(h.machine.faults().is_empty());
}

fn errors(src: &str) -> String {
    match suture_lang::compile(&[("app.st".to_string(), src.to_string())]) {
        Ok(_) => String::new(),
        Err(e) => e.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("\n"),
    }
}

const SHOP: &str = "module shop version 1, currency EUR
kind Carrier  { zone: Text, fee: Money }
kind Order    { zone: Text }
kind Shipment { order: Order }
Order needs Shipment
Shipment uses a Carrier where the Carrier's zone is the Order's zone
prefer Carrier: lowest fee
";

#[test]
fn a_second_prefer_for_the_same_choice_is_an_error() {
    let e = errors(&format!("{SHOP}prefer Carrier: highest fee\n"));
    assert!(e.contains("app.st:8: prefer Carrier is written twice; the first is at line 7"), "{e}");
}

#[test]
fn two_kinds_with_one_plural_are_an_error() {
    let e = errors("module m version 1, currency EUR\nkind Box { size: Text }\nkind Crate (plural Boxes) { size: Text }\n");
    assert!(e.contains("Crate's plural is Boxes, which the plural of Box already is"), "{e}");
    let e = errors("module m version 1, currency EUR\nkind Item (plural Box) { size: Text }\nkind Box { size: Text }\n");
    assert!(e.contains("Item's plural is Box, which is the kind Box"), "{e}");
    assert_eq!(errors("module m version 1, currency EUR\nkind Sheep (plural Sheep) { size: Text }\n"), "");
}

#[test]
fn attempts_must_be_at_least_one() {
    let e = errors("module m version 1, currency EUR, attempts 0\nkind Box { size: Text }\n");
    assert!(e.contains("`attempts` must be at least 1"), "{e}");
}

#[test]
fn a_duration_is_at_most_a_hundred_years() {
    let goal = |d: &str| errors(&format!("{SHOP}effect ship Shipment via carriers within {d}\n  sends    the order's zone\n  reports  shipped\n"));
    assert!(goal("5300 weeks").contains("`5300 weeks` is too long: a duration is at most 100 years"), "{}", goal("5300 weeks"));
    assert!(goal("99999999999999999 weeks").contains("is too long"), "an overflow is an error, not a panic");
    assert_eq!(goal("5200 weeks"), "");
    let rule = errors("module m version 1, currency EUR\nkind Box { wait: Duration }\nrule long: every Box's wait is at most 99999999999999 weeks\n");
    assert!(rule.contains("is too long: a duration is at most 100 years"), "{rule}");
}

#[test]
fn a_change_that_would_make_a_reader_too_large_is_refused_and_changes_nothing() {
    let p = prog("module library version 1, currency EUR
role librarian
kind Member { name: Text, bio: Text, suspended: Bool = no }
kind Loan   { member: Member, guarantor: Member, memo: Text, open: Bool = yes }
rule suspended: no Loan whose member is suspended is open
admit add a new Member or Loan by librarian
admit change Member's bio by librarian
");
    let mut h = Host::new(p);
    let by = |verb: &str| Caller { role: "librarian".into(), name: "Lu".into(), verb: verb.into() };
    h.input_new(Some(&by("adds")), "Member", "Ada", &[("name".into(), Val::Text("Ada".into())), ("bio".into(), Val::Text("short".into()))], &[]).unwrap();
    h.input_new(Some(&by("adds")), "Member", "Bo", &[("name".into(), Val::Text("Bo".into())), ("bio".into(), Val::Text("g".repeat(20_000)))], &[]).unwrap();
    let loan = [("member".into(), Val::Ref("Ada".into())), ("guarantor".into(), Val::Ref("Bo".into())), ("memo".into(), Val::Text("m".repeat(15_000)))];
    h.input_new(Some(&by("adds")), "Loan", "L1", &loan, &[]).unwrap();
    let (ada, loan) = (h.unit("Ada"), h.unit("L1"));
    // Ada fits with the new bio, but L1, which carries it and Bo's, would not.
    let e = h.input_change(Some(&by("changes")), "Ada", "bio", Val::Text("b".repeat(32_000))).unwrap_err();
    assert!(e.starts_with("refused: L1 would hold "), "{e}");
    assert_eq!((h.unit("Ada"), h.unit("L1")), (ada, loan));
    assert!(h.machine.faults().is_empty());
}
