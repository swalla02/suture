//! Slice 16: events with data, and the caller in an event's reactions
//! (spec appendix Q).

use suture_lang::ir::Program;
use suture_lang::scenario::parse_scenarios;
use suture_runtime::scenario::run;
use suture_runtime::state::Val;

const SRC: &str = "module t version 1, currency EUR
role customer is a Customer
role support
kind Customer { name: Text }
kind Order    { customer: Customer, total: Money, refunded: Money = 0 EUR, why: Text = \"\", by: Text = \"\", cancelled: Bool = no }
admit cancel Order by customer with reason: Text where the Order's customer is the caller
admit refund Order by support with amount: Money, note: Text where the amount is at most its total
when a customer cancels an Order: it is cancelled and its why becomes the reason and its by becomes the caller's name
when a support refunds an Order: its refunded becomes the amount and its why becomes the note
rule cap: every Order's refunded is at most 80 EUR
";

fn prog() -> Program {
    suture_lang::compile(&[("t.st".to_string(), SRC.to_string())]).unwrap_or_else(|e| panic!("{e:?}"))
}

fn scenario(steps: &str) -> suture_runtime::scenario::Report {
    let src = format!("scenario \"t\":\n  given  a Customer Ada with name Ada\n  and    a Customer Bob with name Bob\n  and    an Order O1 with customer Ada and total 100 EUR\n{steps}");
    let sc = &parse_scenarios("t.scenario", &src).unwrap_or_else(|e| panic!("{e}"))[0];
    run(&prog(), sc)
}

fn attr(r: &suture_runtime::scenario::Report, a: &str) -> Val {
    r.host.unit("O1").unwrap().root.attrs[a].clone()
}

#[test]
fn an_event_carries_its_data_and_its_caller_to_the_reaction() {
    let r = scenario("  when   customer Ada cancels O1 with reason \"too late\"\n");
    assert!(r.passed(), "{:?}\n{}", r.failures, r.host.trace.join("\n"));
    assert_eq!(attr(&r, "why"), Val::Text("too late".into()));
    assert_eq!(attr(&r, "by"), Val::Text("Ada".into()));
    assert_eq!(attr(&r, "cancelled"), Val::Bool(true));
    assert!(r.host.trace.iter().any(|l| l.contains("customer Ada cancels it with reason \"too late\"")), "{}", r.host.trace.join("\n"));
}

#[test]
fn the_admit_reads_the_data_and_rules_still_refuse() {
    let r = scenario(
        "  when   support Sam refunds O1 with amount 120 EUR and note big\n         is refused at the gate\n\
         \x20 and    support Sam refunds O1 with amount 90 EUR and note big\n         is refused by rule cap\n\
         \x20 and    support Sam refunds O1 with amount 30 EUR and note ok\n",
    );
    assert!(r.passed(), "{:?}\n{}", r.failures, r.host.trace.join("\n"));
    assert_eq!(attr(&r, "refunded"), Val::Money(3000));
    assert_eq!(attr(&r, "why"), Val::Text("ok".into()));
}

#[test]
fn missing_extra_or_mistyped_data_is_refused() {
    let r = scenario(
        "  when   customer Ada cancels O1\n         is refused at the gate\n\
         \x20 and    customer Ada cancels O1 with reason late and mood bad\n         is refused at the gate\n\
         \x20 and    support Sam refunds O1 with amount 3 and note x\n         is refused at the gate\n",
    );
    assert!(r.passed(), "{:?}\n{}", r.failures, r.host.trace.join("\n"));
    assert_eq!(attr(&r, "cancelled"), Val::Bool(false));
}

#[test]
fn bad_event_data_does_not_compile() {
    let bad = |line: &str| {
        let src = SRC.replace("rule cap: every Order's refunded is at most 80 EUR\n", line);
        suture_lang::compile(&[("t.st".to_string(), src)]).map(|_| ()).unwrap_err()
    };
    let e = bad("admit cancel Order by support with reason: Count\n");
    assert!(e.iter().any(|e| e.msg.contains("gives this event other data")), "{e:?}");
    let e = bad("admit pause Order by support with total: Money\nwhen a support pauses an Order: it is cancelled\n");
    assert!(e.iter().any(|e| e.msg.contains("`total` is already an attribute of an Order")), "{e:?}");
    let e = bad("admit pause Order by support with who: Customer\nwhen a support pauses an Order: it is cancelled\n");
    assert!(e.iter().any(|e| e.msg.contains("an event carries no references")), "{e:?}");
    let e = bad("admit pause Order by support\nwhen a support pauses an Order: its by becomes the caller's name\n");
    assert!(e.iter().any(|e| e.msg.contains("has a role with no kind")), "{e:?}");
    let e = bad("admit pause Order by customer\nwhen a customer pauses an Order: the caller is cancelled\n");
    assert!(e.iter().any(|e| e.msg.contains("a reaction changes only its own unit")), "{e:?}");
}
