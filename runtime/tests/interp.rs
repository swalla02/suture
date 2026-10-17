use suture_lang::ir::{Program, TIMED_OUT};
use suture_runtime::interp::{in_state, new_unit, step, violated};
use suture_runtime::state::{Ask, Msg, Unit, Val};

fn shop() -> Program {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../fixtures/shop-slice4");
    suture_lang::compile_dir(std::path::Path::new(dir)).unwrap_or_else(|e| panic!("{e:?}"))
}

fn run(p: &Program, u: &Unit, m: Msg) -> (Unit, Vec<Ask>) {
    let out = step(p, u, &m);
    (out.unit, out.asks)
}

fn order(p: &Program, label: &str, country: &str, euros: i64) -> (Unit, Vec<Ask>) {
    let given = [("country".to_string(), Val::Text(country.into())), ("total".to_string(), Val::Money(euros * 100))];
    let u = new_unit(p, label, "Order", &given).unwrap();
    run(p, &u, Msg::Create)
}

fn answer(p: &Program, u: &Unit, ask: &Ask, outcome: &str, data: &[(&str, Val)]) -> (Unit, Vec<Ask>) {
    let data = data.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
    run(p, u, Msg::Answer { key: ask.key.clone(), outcome: outcome.into(), data })
}

fn ask<'a>(asks: &'a [Ask], adapter: &str) -> &'a Ask {
    asks.iter().find(|a| a.adapter == adapter).unwrap_or_else(|| panic!("no request for {adapter} in {asks:?}"))
}

fn has_note(u: &Unit, text: &str) -> bool {
    u.notes.iter().any(|n| n.contains(text))
}

fn has_part(u: &Unit, kind: &str) -> bool {
    u.root.parts.iter().any(|n| n.kind == kind)
}

fn root_is(p: &Program, u: &Unit, state: &str) -> bool {
    in_state(p, u, &[], state)
}

#[test]
fn a_french_order_grows_a_customs_form() {
    let p = shop();
    let (u, asks) = order(&p, "O1", "France", 120);
    assert!(has_part(&u, "CustomsForm"));
    assert!(has_note(&u, "needs Declaration"), "{:?}", u.notes);
    assert!(has_note(&u, "Shipment waits: rule hold"), "{:?}", u.notes);
    assert_eq!(asks.len(), 2, "{asks:?}");
    assert_eq!(ask(&asks, "forms").key, "O1/CustomsForm#1/Declaration#1");
    assert_eq!(ask(&asks, "payments").sends, vec![("total".to_string(), Val::Money(12000))]);
}

#[test]
fn a_japanese_order_has_no_form() {
    let p = shop();
    let (u, asks) = order(&p, "J1", "Japan", 80);
    assert!(!has_part(&u, "CustomsForm"));
    assert_eq!(asks.len(), 1, "{asks:?}");
    assert!(has_note(&u, "Shipment waits: rule paid"), "{:?}", u.notes);
}

#[test]
fn an_over_declared_value_is_refused_and_asked_again() {
    let p = shop();
    let (u, asks) = order(&p, "O1", "France", 120);
    let form = ask(&asks, "forms").clone();
    let data = [("value", Val::Money(15000)), ("contents", Val::Text("books".into()))];
    let (u2, again) = answer(&p, &u, &form, "filled", &data);
    assert!(has_note(&u2, "refused by rule declared"), "{:?}", u2.notes);
    assert_eq!(u2.root, u.root, "a refused answer changes nothing");
    assert_eq!(again, vec![form]);
}

#[test]
fn alcohol_opens_the_excise_section_and_the_form_releases_shipping() {
    let p = shop();
    let (u, asks) = order(&p, "O1", "France", 120);
    let (u, _) = answer(&p, &u, ask(&asks, "payments"), "captured", &[]);
    assert!(has_note(&u, "Payment captured"), "{:?}", u.notes);
    let data = [("value", Val::Money(10000)), ("contents", Val::Text("alcohol".into()))];
    let (u, asks) = answer(&p, &u, ask(&asks, "forms"), "filled", &data);
    let excise = ask(&asks, "forms").clone();
    assert_eq!(excise.key, "O1/CustomsForm#1/Excise#1");
    let (u, again) = answer(&p, &u, &excise, "filled", &[("licence", Val::Text(String::new()))]);
    assert!(has_note(&u, "refused by rule licence"), "{:?}", u.notes);
    assert_eq!(again, vec![excise.clone()]);
    let (u, asks) = answer(&p, &u, &excise, "filled", &[("licence", Val::Text("L-77".into()))]);
    assert!(has_note(&u, "CustomsForm done"), "{:?}", u.notes);
    let ship = ask(&asks, "carriers");
    assert_eq!(ship.sends, vec![("country".to_string(), Val::Text("France".into()))]);
    let (u, _) = answer(&p, &u, ship, "delivered", &[]);
    assert!(root_is(&p, &u, "paid") && root_is(&p, &u, "settled"));
    assert!(has_note(&u, "goal met"), "{:?}", u.notes);
}

#[test]
fn a_declined_payment_cancels_the_order_and_withdraws_the_shipment() {
    let p = shop();
    let (u, asks) = order(&p, "D1", "Japan", 50);
    let (u, more) = answer(&p, &u, ask(&asks, "payments"), "declined", &[]);
    assert!(root_is(&p, &u, "cancelled"));
    assert!(!root_is(&p, &u, "paid"));
    assert!(!has_part(&u, "Shipment"), "the idle shipment is withdrawn");
    assert!(has_note(&u, "Shipment withdrawn"), "{:?}", u.notes);
    assert!(more.is_empty());
}

#[test]
fn a_late_answer_is_ignored() {
    let p = shop();
    let (u, asks) = order(&p, "J1", "Japan", 80);
    let pay = ask(&asks, "payments").clone();
    let (u, _) = answer(&p, &u, &pay, "captured", &[]);
    let (u2, again) = answer(&p, &u, &pay, "captured", &[]);
    assert_eq!(u2.root, u.root);
    assert!(again.is_empty());
    assert_eq!(u2.notes, vec![format!("ignored an answer for {}: no open request", pay.key)]);
}

#[test]
fn wrongly_typed_answer_data_is_refused() {
    let p = shop();
    let (u, asks) = order(&p, "O1", "France", 120);
    let form = ask(&asks, "forms").clone();
    let data = [("value", Val::Text("abc".into())), ("contents", Val::Text("books".into()))];
    let (u2, again) = answer(&p, &u, &form, "filled", &data);
    assert_eq!(u2.root, u.root);
    assert!(has_note(&u2, "value must be Money"), "{:?}", u2.notes);
    assert_eq!(again, vec![form]);
}

#[test]
fn a_lost_parcel_is_shipped_again() {
    let p = shop();
    let (u, asks) = order(&p, "J1", "Japan", 80);
    let (u, asks) = answer(&p, &u, ask(&asks, "payments"), "captured", &[]);
    let first = ask(&asks, "carriers").clone();
    assert_eq!(first.key, "J1/Shipment#1");
    let (u, asks) = answer(&p, &u, &first, "lost", &[]);
    assert!(has_note(&u, "Shipment lost: attempt 2"), "{:?}", u.notes);
    assert_eq!(ask(&asks, "carriers").key, "J1/Shipment#2");
}

fn program(src: &str) -> Program {
    let files = vec![("t.st".to_string(), format!("module t version 1, currency EUR\n{src}"))];
    suture_lang::compile(&files).unwrap_or_else(|e| panic!("{e:?}"))
}

const PAY: &str = "kind Order { total: Money }
kind Payment { order: Order }
effect capture Payment via payments within 30 seconds
  sends    the order's total
  reports  captured | fails with declined
";

fn placed(p: &Program, euros: i64) -> (Unit, Vec<Ask>) {
    let u = new_unit(p, "O1", "Order", &[("total".to_string(), Val::Money(euros * 100))]).unwrap();
    run(p, &u, Msg::Create)
}

#[test]
fn a_part_waits_while_creating_it_would_break_a_rule() {
    let p = program(&format!(
        "{PAY}kind Shipment {{ order: Order }}
Order needs Payment, Shipment
effect send Shipment via carriers within 7 days
  sends    the order's total
  reports  delivered | fails with lost
rule paid: no Order has a Shipment unless its Payment is captured
"
    ));
    let (u, asks) = placed(&p, 50);
    assert!(violated(&p, &u).is_none());
    assert!(!has_part(&u, "Shipment"), "{:?}", u.root);
    assert!(has_note(&u, "Shipment waits: rule paid"), "{:?}", u.notes);
    let (u, asks) = answer(&p, &u, ask(&asks, "payments"), "captured", &[]);
    assert!(has_part(&u, "Shipment"), "{:?}", u.notes);
    ask(&asks, "carriers");
}

#[test]
fn a_part_waits_to_be_done_while_a_rule_forbids_it() {
    let p = program(&format!(
        "{PAY}kind Form {{ order: Order }}
kind Entry {{ form: Form }}
Order needs Payment, Form
Form needs Entry
effect ask Entry via forms within 3 days
  sends    the form's order's total
  reports  filled
rule early: no Order has a Form that is done unless its Payment is captured
"
    ));
    let (u, asks) = placed(&p, 50);
    let pay = ask(&asks, "payments").clone();
    let (u, _) = answer(&p, &u, ask(&asks, "forms"), "filled", &[]);
    assert!(violated(&p, &u).is_none());
    assert!(has_note(&u, "Entry filled"), "the outside fact is kept: {:?}", u.notes);
    assert!(has_note(&u, "Form waits: rule early"), "{:?}", u.notes);
    let (u, _) = answer(&p, &u, &pay, "captured", &[]);
    assert!(has_note(&u, "Form done"), "{:?}", u.notes);
}

#[test]
fn creating_a_unit_that_breaks_a_rule_is_refused() {
    let p = program(&format!("{PAY}Order needs Payment\nrule cap: every Order's total is at most 500 EUR\n"));
    let err = new_unit(&p, "O1", "Order", &[("total".to_string(), Val::Money(60_000))]).unwrap_err();
    assert!(err.contains("refused by rule cap"), "{err}");
}

#[test]
fn a_replaced_owner_gives_its_parts_new_request_keys() {
    let p = program(&format!(
        "{PAY}kind Shipment {{ order: Order }}
kind Label {{ shipment: Shipment }}
Order needs Payment, Shipment
Shipment needs Label
effect print Label via labels within 1 days
  sends    the shipment's order's total
  reports  printed
effect send Shipment via carriers within 7 days
  sends    the order's total
  reports  delivered | fails with lost
"
    ));
    let (u, asks) = placed(&p, 50);
    let label1 = ask(&asks, "labels").clone();
    assert_eq!(label1.key, "O1/Shipment#1/Label#1");
    let (u, asks) = answer(&p, &u, &label1, "printed", &[]);
    let (u, asks) = answer(&p, &u, ask(&asks, "carriers"), "lost", &[]);
    assert_eq!(ask(&asks, "labels").key, "O1/Shipment#2/Label#1");
    let (again, sent) = answer(&p, &u, &label1, "printed", &[]);
    assert_eq!(again.root, u.root, "a duplicate of the first label's answer must not apply to the second");
    assert!(has_note(&again, "no open request"), "{:?}", again.notes);
    assert!(sent.is_empty());
}

#[test]
fn a_withdrawal_that_would_break_a_rule_refuses_the_answer() {
    let p = program(
        "kind Order { total: Money, cancelled: Bool = no }
kind Payment { order: Order }
kind Shipment { order: Order }
Order needs Payment, Shipment while it is not cancelled
effect capture Payment via payments within 30 seconds
  sends    the order's total
  reports  captured | fails with declined
effect send Shipment via carriers within 7 days
  sends    the order's total
  marks    sent
  reports  delivered | fails with lost
rule keep: every Order has a Shipment
rule paid: no Order has a Shipment that is sent unless its Payment is captured
when a Payment is declined: its Order is cancelled
",
    );
    let (u, asks) = placed(&p, 50);
    let pay = ask(&asks, "payments").clone();
    let (after, again) = answer(&p, &u, &pay, "declined", &[]);
    assert!(violated(&p, &after).is_none());
    assert_eq!(after.root, u.root);
    assert!(has_note(&after, "refused by rule keep"), "{:?}", after.notes);
    assert_eq!(again, vec![pay]);
}

fn time_out(p: &Program, u: &Unit, ask: &Ask) -> (Unit, Vec<Ask>) {
    answer(p, u, ask, TIMED_OUT, &[])
}

fn payment(u: &Unit) -> &suture_runtime::state::Node {
    u.root.parts.iter().rfind(|n| n.kind == "Payment").expect("a Payment")
}

#[test]
fn an_unanswered_request_is_sent_again_then_times_out() {
    let p = shop();
    let (u, asks) = order(&p, "J1", "Japan", 80);
    let pay = ask(&asks, "payments").clone();
    let (u, asks) = time_out(&p, &u, &pay);
    assert_eq!(asks, vec![pay.clone()], "the same request, with the same key");
    assert!(has_note(&u, "no answer within 30 seconds: asking again (send 2 of 3)"), "{:?}", u.notes);
    let (u, asks) = time_out(&p, &u, &pay);
    assert!(has_note(&u, "send 3 of 3"), "{:?}", u.notes);
    assert_eq!(asks.len(), 1);
    let (u, asks) = time_out(&p, &u, &pay);
    assert!(asks.is_empty(), "{asks:?}");
    assert!(has_note(&u, "Payment timed out"), "{:?}", u.notes);
    assert_eq!(payment(&u).outcome.as_deref(), Some(TIMED_OUT));
    let i = u.root.parts.iter().position(|n| n.kind == "Payment").unwrap();
    assert!(in_state(&p, &u, &[i], TIMED_OUT));
    assert_eq!(u.root.parts.iter().filter(|n| n.kind == "Payment").count(), 1, "a timed-out part is not replaced");
}

#[test]
fn a_unit_records_the_goals_it_matches() {
    let p = shop();
    let (u, _) = order(&p, "J1", "Japan", 80);
    assert_eq!(u.matched, vec![0]);
}

#[test]
fn a_goal_due_and_unmet_raises_one_alert_that_explains_why() {
    let p = shop();
    let (u, asks) = order(&p, "J1", "Japan", 80);
    let pay = ask(&asks, "payments").clone();
    let (mut u, _) = time_out(&p, &u, &pay);
    for _ in 0..2 {
        u = time_out(&p, &u, &pay).0;
    }
    let (u, sent) = run(&p, &u, Msg::GoalDue { goal: 0 });
    assert!(sent.is_empty());
    assert_eq!(u.alerts, vec![0]);
    assert!(has_note(&u, "alert: not met within 3 days: every Order that is not cancelled is paid and settled"), "{:?}", u.notes);
    assert!(has_note(&u, "Payment timed out"), "the alert explains: {:?}", u.notes);
    let (again, _) = run(&p, &u, Msg::GoalDue { goal: 0 });
    assert_eq!(again.alerts, vec![0]);
    assert!(again.notes.is_empty(), "{:?}", again.notes);
}

#[test]
fn a_goal_met_after_its_alert_is_noted_late() {
    let p = shop();
    let (u, asks) = order(&p, "J1", "Japan", 80);
    let (u, _) = run(&p, &u, Msg::GoalDue { goal: 0 });
    assert_eq!(u.alerts, vec![0]);
    let (u, asks) = answer(&p, &u, ask(&asks, "payments"), "captured", &[]);
    let (u, _) = answer(&p, &u, ask(&asks, "carriers"), "delivered", &[]);
    assert!(has_note(&u, "goal met late: every Order"), "{:?}", u.notes);
}

#[test]
fn a_time_out_is_recorded_even_when_its_reaction_would_break_a_rule() {
    let p = program(
        "kind Order { total: Money, cancelled: Bool = no }
kind Payment { order: Order }
kind Shipment { order: Order }
Order needs Payment, Shipment
effect capture Payment via payments within 30 seconds
  sends    the order's total
  reports  captured | fails with declined
effect send Shipment via carriers within 1 hour
  sends    the order's total
  marks    sent
  reports  delivered | fails with lost
rule paid: no Order has a Shipment that is sent unless its Payment is captured
rule late: no cancelled Order has a Shipment that is sent
when a Shipment is timed out: its Order is cancelled
",
    );
    let (u, asks) = placed(&p, 50);
    let (mut u, asks) = answer(&p, &u, ask(&asks, "payments"), "captured", &[]);
    let ship = ask(&asks, "carriers").clone();
    let mut sent = vec![];
    for _ in 0..3 {
        (u, sent) = time_out(&p, &u, &ship);
    }
    assert!(sent.is_empty(), "no fourth send: {sent:?}");
    let s = u.root.parts.iter().find(|n| n.kind == "Shipment").unwrap();
    assert_eq!(s.outcome.as_deref(), Some(TIMED_OUT));
    assert!(!root_is(&p, &u, "cancelled"));
    assert!(has_note(&u, "reaction held: rule late"), "{:?}", u.notes);
    assert!(violated(&p, &u).is_none());
}

#[test]
fn a_slot_fills_with_its_first_preferred_alternative() {
    let p = shop();
    let (u, _) = order(&p, "J1", "Japan", 80);
    assert_eq!(u.root.slots.get("Delivery").map(String::as_str), Some("Shipment"));
    assert!(has_part(&u, "Shipment"));
    assert!(has_note(&u, "Delivery: trying Shipment"), "{:?}", u.notes);
    assert_eq!(u.timers, vec![("J1/Delivery:Shipment#1".to_string(), 2 * 86_400)]);
}

#[test]
fn three_lost_parcels_move_the_delivery_to_a_refund() {
    let p = shop();
    let (u, asks) = order(&p, "J1", "Japan", 80);
    let (mut u, mut asks) = answer(&p, &u, ask(&asks, "payments"), "captured", &[]);
    for _ in 0..3 {
        let parcel = ask(&asks, "carriers").clone();
        (u, asks) = answer(&p, &u, &parcel, "lost", &[]);
    }
    assert_eq!(u.root.slots.get("Delivery").map(String::as_str), Some("Refund"));
    assert!(has_note(&u, "Delivery: Shipment failed, trying Refund"), "{:?}", u.notes);
    let refund = ask(&asks, "payments").clone();
    assert_eq!(refund.key, "J1/Refund#1");
    let (u, _) = answer(&p, &u, &refund, "refunded", &[]);
    assert!(root_is(&p, &u, "settled"), "{:?}", u.notes);
    assert!(has_note(&u, "goal met"), "{:?}", u.notes);
}

#[test]
fn patience_moves_an_untouched_alternative_on() {
    let p = shop();
    let (u, asks) = order(&p, "O1", "France", 120);
    let (u, _) = answer(&p, &u, ask(&asks, "payments"), "captured", &[]);
    let (u, asks) = run(&p, &u, Msg::Patience { id: "O1/Delivery:Shipment#1".into() });
    assert!(has_note(&u, "Delivery: no Shipment after 2 days, trying Refund"), "{:?}", u.notes);
    assert!(!has_part(&u, "Shipment"), "the untouched Shipment is withdrawn");
    assert_eq!(ask(&asks, "payments").key, "O1/Refund#1");
}

#[test]
fn patience_does_not_abandon_an_alternative_that_went_out() {
    let p = shop();
    let (u, asks) = order(&p, "J1", "Japan", 80);
    let (u, _) = answer(&p, &u, ask(&asks, "payments"), "captured", &[]);
    let (after, sent) = run(&p, &u, Msg::Patience { id: "J1/Delivery:Shipment#1".into() });
    assert_eq!(after.root, u.root);
    assert!(after.notes.is_empty() && sent.is_empty(), "{:?}", after.notes);
}

fn shop_plus(extra: &str) -> Program {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../fixtures/shop-slice4");
    let mut files: Vec<(String, String)> = ["shop.st", "france.st"]
        .iter()
        .map(|f| (f.to_string(), std::fs::read_to_string(format!("{dir}/{f}")).unwrap()))
        .collect();
    files.push(("extra.st".into(), extra.into()));
    suture_lang::compile(&files).unwrap_or_else(|e| panic!("{e:?}"))
}

#[test]
fn patience_that_would_break_a_rule_is_held() {
    let p = shop_plus("rule keep: every Order whose country is France has a Shipment\n");
    let (u, asks) = order(&p, "O1", "France", 120);
    let (u, _) = answer(&p, &u, ask(&asks, "payments"), "captured", &[]);
    let (after, sent) = run(&p, &u, Msg::Patience { id: u.timers[0].0.clone() });
    assert!(violated(&p, &after).is_none());
    assert_eq!(after.root, u.root);
    assert!(has_note(&after, "patience held: rule keep"), "{:?}", after.notes);
    assert!(sent.is_empty());
}

#[test]
fn a_slot_filled_again_gets_a_new_patience_timer_and_the_old_one_is_ignored() {
    let p = program(
        "kind Order { total: Money, paused: Bool = no, big: Bool = yes }
kind Ping { order: Order }
kind A { order: Order }
kind B { order: Order }
Order needs Ping
Order needs Delivery while it is not paused
Delivery is A or B
prefer Delivery: A, then B after 1 day
effect ping Ping via pings within 1 day
  sends    the order's total
  reports  up | fails with down
effect go A via as within 1 day
  sends    the order's total
  reports  ok
effect go B via bs within 1 day
  sends    the order's total
  reports  ok
rule big: no big Order has an A
when a Ping is down: its Order is paused
when a Ping is up: its Order is not paused
when an A is ok: its Order is not big
",
    );
    let (u, asks) = placed(&p, 150);
    let first = u.timers[0].0.clone();
    let (u, asks) = answer(&p, &u, ask(&asks, "pings"), "down", &[]);
    assert!(u.root.slots.is_empty(), "an inactive need empties its slot: {:?}", u.root.slots);
    let (stale, _) = run(&p, &u, Msg::Patience { id: first.clone() });
    assert_eq!(stale.root, u.root, "the timer of an emptied slot does nothing");
    let (u, _) = answer(&p, &u, ask(&asks, "pings"), "up", &[]);
    let second = u.timers.last().unwrap().0.clone();
    assert_ne!(second, first);
    let (ignored, _) = run(&p, &u, Msg::Patience { id: first });
    assert_eq!(ignored.root, u.root, "an old fill's timer does nothing");
    let (moved, asks) = run(&p, &u, Msg::Patience { id: second });
    assert_eq!(moved.root.slots.get("Delivery").map(String::as_str), Some("B"));
    ask(&asks, "bs");
}

#[test]
fn a_bound_for_each_entity_holds_back_a_part() {
    let p = program(
        "kind Order { total: Money }
kind Shipment { order: Order }
Order needs Shipment
effect send Shipment via carriers within 1 day
  sends    the order's total
  reports  delivered | fails with lost
rule few: the number of each Order's Shipments is at most 2
",
    );
    let (mut u, mut asks) = placed(&p, 50);
    for _ in 0..2 {
        let parcel = ask(&asks, "carriers").clone();
        (u, asks) = answer(&p, &u, &parcel, "lost", &[]);
    }
    assert!(has_note(&u, "Shipment waits: rule few"), "{:?}", u.notes);
    assert_eq!(u.root.parts.iter().filter(|n| n.kind == "Shipment").count(), 2);
    assert!(violated(&p, &u).is_none());
}

#[test]
fn math_in_a_rule_refuses_a_creation() {
    let p = program(&format!("{PAY}Order needs Payment\nrule cap: every Order's total is at most 100 EUR - 1 EUR\n"));
    let err = new_unit(&p, "O1", "Order", &[("total".to_string(), Val::Money(10_000))]).unwrap_err();
    assert!(err.contains("refused by rule cap"), "{err}");
    assert!(new_unit(&p, "O2", "Order", &[("total".to_string(), Val::Money(9_900))]).is_ok());
}

const LINES: &str = "kind Order { lines: list of Line, credit: Money = 0 EUR, n: Count = 0,
                   total: Money = the total of the line's price * the line's quantity over its lines }
kind Line { order: Order, price: Money, quantity: Count }
kind Payment { order: Order, amount: Money = order's total }
Order needs Payment
effect capture Payment via payments within 30 seconds
  sends    its amount
  reports  captured | fails with declined
when a Payment is captured: its order's credit becomes its order's credit + its amount
when a Payment is declined: its order's n becomes its order's n - 1
";

fn with_lines(p: &Program) -> (Unit, Vec<Ask>) {
    let line = |price: i64, quantity: i64| vec![("price".to_string(), Val::Money(price * 100)), ("quantity".to_string(), Val::Count(quantity))];
    let items = vec![("lines".to_string(), line(3, 2)), ("lines".to_string(), line(10, 1))];
    let u = suture_runtime::interp::new_unit_with(p, "O1", "Order", &[], &items).unwrap();
    run(p, &u, Msg::Create)
}

#[test]
fn formulas_and_lists_are_computed_at_creation() {
    let p = program(LINES);
    let (u, asks) = with_lines(&p);
    assert_eq!(u.root.attrs.get("total"), Some(&Val::Money(1_600)));
    assert_eq!(u.root.parts.iter().filter(|n| n.kind == "Line").count(), 2);
    assert_eq!(ask(&asks, "payments").sends, vec![("amount".to_string(), Val::Money(1_600))]);
    assert!(!has_note(&u, "Line done"), "list elements are done from the start: {:?}", u.notes);
}

#[test]
fn becomes_sets_a_value_and_never_makes_a_count_negative() {
    let p = program(LINES);
    let (u, asks) = with_lines(&p);
    let (paid, _) = answer(&p, &u, ask(&asks, "payments"), "captured", &[]);
    assert_eq!(paid.root.attrs.get("credit"), Some(&Val::Money(1_600)));
    assert!(has_note(&paid, "Order's credit becomes 16 EUR"), "{:?}", paid.notes);
    let (declined, _) = answer(&p, &u, ask(&asks, "payments"), "declined", &[]);
    assert_eq!(declined.root.attrs.get("n"), Some(&Val::Count(0)));
    assert!(has_note(&declined, "n not set: a count cannot be negative"), "{:?}", declined.notes);
}

#[test]
fn review_formulas_compute_in_any_order_and_never_store_a_negative_count() {
    let p = program("kind Order { seats: Count, lines: list of Line, double: Money = price * 2, price: Money = seats * 10 EUR, left: Count = seats - 5 }
kind Line { order: Order, share: Money = order's price }
");
    let items = vec![("lines".to_string(), vec![])];
    let u = suture_runtime::interp::new_unit_with(&p, "O1", "Order", &[("seats".to_string(), Val::Count(3))], &items).unwrap();
    assert_eq!(u.root.attrs.get("double"), Some(&Val::Money(6_000)));
    assert_eq!(u.root.parts[0].attrs.get("share"), Some(&Val::Money(3_000)));
    assert_eq!(u.root.attrs.get("left"), None, "a count is never negative");
}
