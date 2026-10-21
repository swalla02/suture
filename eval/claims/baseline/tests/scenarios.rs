//! The scenarios of eval/claims/suture/claims.scenario, against the baseline.
#![allow(clippy::inconsistent_digit_grouping)] // money in cents: 300_00 is 300.00 EUR

use claims_baseline::*;

const DENT: &[u8] = b"\x89PNG small dent on the door";
const WRECK: &[u8] = b"\x89PNG total loss, roof crushed";

fn who(role: Role, name: &str) -> Caller {
    Caller { role, name: name.into() }
}

fn ops() -> Caller {
    who(Role::Ops, "Oz")
}

fn ada() -> Caller {
    who(Role::Holder, "Ada")
}

fn bo() -> Caller {
    who(Role::Adjuster, "Bo")
}

/// Looks at the photo, as the scenarios' model does.
fn model(pick: Option<Kind>) -> Model {
    Box::new(move |_| pick)
}

/// Reads the photo, as the scenarios' reader does: the next reading each time.
fn reads(readings: &[(i64, f64)]) -> Reader {
    let mut readings = Vec::from(readings).into_iter();
    Box::new(move |_| readings.next())
}

fn world(limit: i64) -> System {
    let mut s = System::default();
    s.add_holder(&ops(), "Ada", "Ada").unwrap();
    s.add_policy(&ops(), "P1", Policy { holder: "Ada".into(), region: "north".into(), limit, deductible: 300_00, iban: "NL01".into() }).unwrap();
    s
}

fn sent_book(s: &System, id: &str) -> bool {
    s.outbox.iter().any(|r| matches!(r, Request::Book { claim, .. } if claim == id))
}

fn sent_pay(s: &System, id: &str) -> bool {
    s.outbox.iter().any(|r| matches!(r, Request::Pay { claim, .. } if claim == id))
}

#[test]
fn a_dent_is_repaired_at_the_garage_with_the_most_room() {
    let mut s = world(20000_00);
    s.add_garage(&ops(), "G1", "Bolt", "north", 1).unwrap();
    s.add_garage(&ops(), "G2", "Nut", "north", 3).unwrap();
    s.add_garage(&ops(), "G3", "Far", "south", 9).unwrap();
    s.model = Some(model(Some(Kind::Repair)));
    s.file_claim(&ada(), "C1", "P1", DENT.to_vec(), "door", 1200_00).unwrap();
    s.fraud_answers("C1", Some(10));
    assert!(sent_book(&s, "C1"));
    assert_eq!(s.garages["G2"].taken, 1);
    s.garage_answers("C1", true);
    assert!(s.claims["C1"].settled());
}

#[test]
fn a_total_loss_is_written_off_for_the_claim_less_the_deductible() {
    let mut s = world(20000_00);
    s.model = Some(model(Some(Kind::Writeoff)));
    s.reader = Some(reads(&[(4300_00, 0.95)]));
    s.file_claim(&ada(), "C1", "P1", WRECK.to_vec(), "roof", 4300_00).unwrap();
    s.fraud_answers("C1", Some(10));
    assert!(s.outbox.contains(&Request::Pay { claim: "C1".into(), amount: 4000_00, iban: "NL01".into() }));
    s.bank_answers("C1", true);
    assert!(s.claims["C1"].settled());
}

#[test]
fn a_big_write_off_waits_for_an_adjuster() {
    let mut s = world(20000_00);
    s.model = Some(model(Some(Kind::Writeoff)));
    s.reader = Some(reads(&[(9300_00, 0.95)]));
    s.file_claim(&ada(), "C1", "P1", WRECK.to_vec(), "roof", 9300_00).unwrap();
    assert!(!sent_pay(&s, "C1"));
    s.set_approved(&bo(), "C1", true).unwrap();
    assert!(sent_pay(&s, "C1"));
}

#[test]
fn a_write_off_over_the_policy_limit_is_never_paid_approved_or_not() {
    let mut s = world(3000_00);
    s.model = Some(model(Some(Kind::Writeoff)));
    s.reader = Some(reads(&[(4300_00, 0.95)]));
    s.file_claim(&ada(), "C1", "P1", WRECK.to_vec(), "roof", 4300_00).unwrap();
    s.set_approved(&bo(), "C1", true).unwrap();
    assert!(!sent_pay(&s, "C1"));
    s.tick(14 * DAY);
    assert_eq!(s.alerts, ["C1"]);
}

#[test]
fn a_suspect_claim_waits_for_an_adjuster_and_a_failing_bureau_counts_as_suspect() {
    let mut s = world(20000_00);
    s.add_garage(&ops(), "G1", "Bolt", "north", 1).unwrap();
    s.model = Some(model(Some(Kind::Repair)));
    s.file_claim(&ada(), "C1", "P1", DENT.to_vec(), "door", 1200_00).unwrap();
    s.fraud_answers("C1", None);
    assert_eq!(s.claims["C1"].score.as_ref().unwrap().value, 100);
    assert!(!sent_book(&s, "C1"));
    s.set_approved(&bo(), "C1", true).unwrap();
    assert!(sent_book(&s, "C1"));
}

#[test]
fn a_fraud_score_older_than_a_day_is_fetched_again_before_a_repair_is_booked() {
    let mut s = world(20000_00);
    s.add_garage(&ops(), "G1", "Bolt", "north", 1).unwrap();
    s.model = Some(model(Some(Kind::Repair)));
    s.file_claim(&ada(), "C1", "P1", DENT.to_vec(), "door", 1200_00).unwrap();
    s.fraud_answers("C1", Some(80));
    assert!(!sent_book(&s, "C1"));
    s.tick(DAY);
    s.fraud_answers("C1", Some(20));
    assert_eq!(s.claims["C1"].score.as_ref().unwrap().value, 20);
    assert!(sent_book(&s, "C1"));
}

#[test]
fn with_no_model_and_no_garage_with_room_the_claim_is_written_off_after_two_days() {
    let mut s = world(20000_00);
    s.add_garage(&ops(), "G1", "Bolt", "north", 1).unwrap();
    s.add_holder(&ops(), "Cy", "Cy").unwrap();
    s.add_policy(&ops(), "P2", Policy { holder: "Cy".into(), region: "north".into(), limit: 20000_00, deductible: 300_00, iban: "NL02".into() }).unwrap();
    s.model = Some(model(None));
    s.file_claim(&ada(), "C1", "P1", DENT.to_vec(), "door", 1200_00).unwrap();
    s.fraud_answers("C1", Some(10));
    assert!(sent_book(&s, "C1"));
    s.reader = Some(reads(&[(1300_00, 0.95)]));
    s.file_claim(&who(Role::Holder, "Cy"), "C2", "P2", DENT.to_vec(), "door", 1300_00).unwrap();
    assert!(!sent_book(&s, "C2"));
    s.tick(2 * DAY);
    s.fraud_answers("C2", Some(10));
    assert!(sent_pay(&s, "C2"));
}

#[test]
fn only_the_policys_holder_files_or_withdraws_and_a_withdrawn_claim_stops() {
    let mut s = world(20000_00);
    s.add_holder(&ops(), "Cy", "Cy").unwrap();
    let cy = who(Role::Holder, "Cy");
    assert!(s.file_claim(&cy, "C9", "P1", DENT.to_vec(), "door", 1200_00).is_err());
    s.model = Some(model(Some(Kind::Writeoff)));
    s.reader = Some(reads(&[(9300_00, 0.95)]));
    s.file_claim(&ada(), "C1", "P1", WRECK.to_vec(), "roof", 9300_00).unwrap();
    assert!(s.withdraw(&cy, "C1").is_err());
    s.withdraw(&ada(), "C1").unwrap();
    s.set_approved(&bo(), "C1", true).unwrap();
    assert!(s.claims["C1"].withdrawn && !s.claims["C1"].settled());
    assert!(!sent_pay(&s, "C1"));
    s.tick(14 * DAY);
    assert!(s.alerts.is_empty());
}

#[test]
fn a_booking_nobody_answers_is_sent_again_and_the_claim_stays_with_the_garage() {
    let mut s = world(20000_00);
    s.add_garage(&ops(), "G1", "Bolt", "north", 1).unwrap();
    s.model = Some(model(Some(Kind::Repair)));
    s.file_claim(&ada(), "C1", "P1", DENT.to_vec(), "door", 1200_00).unwrap();
    s.fraud_answers("C1", Some(10));
    s.tick(2 * DAY);
    let books = s.outbox.iter().filter(|r| matches!(r, Request::Book { .. })).count();
    assert_eq!(books, 2);
    assert_eq!(s.garages["G1"].taken, 1);
    s.garage_answers("C1", true);
    assert!(s.claims["C1"].settled());
}

#[test]
fn a_withdrawn_claim_stops_asking_the_bureau() {
    let mut s = world(20000_00);
    s.add_garage(&ops(), "G1", "Bolt", "north", 1).unwrap();
    s.model = Some(model(Some(Kind::Repair)));
    s.file_claim(&ada(), "C1", "P1", DENT.to_vec(), "door", 1200_00).unwrap();
    s.withdraw(&ada(), "C1").unwrap();
    s.tick(20);
    let asked = s.outbox.iter().filter(|r| matches!(r, Request::Fraud { .. })).count();
    assert_eq!(asked, 1);
    assert_eq!(s.garages["G1"].taken, 0);
}

#[test]
fn a_refused_payment_is_tried_three_times_then_a_repair_is_tried() {
    let mut s = world(20000_00);
    s.model = Some(model(Some(Kind::Writeoff)));
    s.reader = Some(reads(&[(4300_00, 0.95)]));
    s.file_claim(&ada(), "C1", "P1", WRECK.to_vec(), "roof", 4300_00).unwrap();
    s.fraud_answers("C1", Some(10));
    for _ in 0..3 {
        assert!(matches!(s.claims["C1"].outcome.as_ref().unwrap().effect, Effect::Sent { .. }));
        s.bank_answers("C1", false);
    }
    assert_eq!(s.claims["C1"].outcome.as_ref().unwrap().kind, Kind::Repair);
}

#[test]
fn a_policy_pays_out_at_most_25000_eur_a_year() {
    let mut s = world(20000_00);
    s.model = Some(model(Some(Kind::Writeoff)));
    s.reader = Some(reads(&[(15300_00, 0.95), (12300_00, 0.95)]));
    s.file_claim(&ada(), "C1", "P1", WRECK.to_vec(), "roof", 15300_00).unwrap();
    s.file_claim(&ada(), "C2", "P1", WRECK.to_vec(), "roof", 12300_00).unwrap();
    s.set_approved(&bo(), "C1", true).unwrap();
    s.set_approved(&bo(), "C2", true).unwrap();
    assert!(sent_pay(&s, "C1"));
    assert!(!sent_pay(&s, "C2"));
}

#[test]
fn a_holder_can_be_paid_the_payout_in_cash_under_the_same_checks() {
    let mut s = world(20000_00);
    s.model = Some(model(Some(Kind::Cash)));
    s.reader = Some(reads(&[(1200_00, 0.95)]));
    s.file_claim(&ada(), "C1", "P1", DENT.to_vec(), "door", 1200_00).unwrap();
    s.fraud_answers("C1", Some(80));
    assert!(!sent_pay(&s, "C1"));
    s.set_approved(&bo(), "C1", true).unwrap();
    assert!(sent_pay(&s, "C1"));
    s.bank_answers("C1", true);
    assert!(s.claims["C1"].settled());
}

#[test]
fn a_payout_over_the_cost_the_photo_shows_waits_for_an_adjuster() {
    let mut s = world(20000_00);
    s.model = Some(model(Some(Kind::Writeoff)));
    s.reader = Some(reads(&[(2500_00, 0.95)]));
    s.file_claim(&ada(), "C1", "P1", WRECK.to_vec(), "roof", 4300_00).unwrap();
    assert_eq!(s.claims["C1"].estimate, Some(2500_00));
    assert!(!sent_pay(&s, "C1"));
    s.set_approved(&bo(), "C1", true).unwrap();
    assert!(sent_pay(&s, "C1"));
}

#[test]
fn a_reading_the_model_is_not_sure_of_is_read_again_an_hour_later() {
    let mut s = world(20000_00);
    s.model = Some(model(Some(Kind::Writeoff)));
    s.reader = Some(reads(&[(4300_00, 0.5), (4300_00, 0.9)]));
    s.file_claim(&ada(), "C1", "P1", WRECK.to_vec(), "roof", 4300_00).unwrap();
    assert!(!sent_pay(&s, "C1"));
    s.tick(3600);
    s.fraud_answers("C1", Some(10));
    assert_eq!(s.claims["C1"].estimate, Some(4300_00));
    assert!(sent_pay(&s, "C1"));
}
