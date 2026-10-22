//! Slice 40: no two alike (spec appendix AP).

use suture_lang::scenario::parse_scenarios;
use suture_runtime::scenario::{run, Report};

const CFP: &str = "module cfp version 1, currency EUR
role reviewer is a Person
role staff
kind Person (plural People) { name: Text, email: Text }
kind Talk { title: Text }
kind Review { talk: Talk, reviewer: Person, score: Count, withdrawn: Bool = no }
kind Slot { room: Text, day: Time, hour: Count }
rule once: no two Reviews that are not withdrawn have the same talk and reviewer
rule emails: no two People have the same email, checked globally
rule rooms: no two Slots have the same room, day and hour, checked globally
when a reviewer withdraws a Review: the Review is withdrawn
admit add a new Person, Talk or Slot by staff
admit score a new Review by reviewer where the Review's reviewer is the caller
admit withdraw Review by reviewer where the Review's reviewer is the caller
";

fn compiled(src: &str) -> suture_lang::ir::Program {
    suture_lang::compile(&[("t.st".to_string(), src.to_string())]).unwrap_or_else(|e| panic!("{e:?}"))
}

fn runs(src: &str, body: &str) -> Report {
    let p = compiled(src);
    let text = format!("scenario \"s\":\n{body}");
    let sc = &parse_scenarios("t.scenario", &text).unwrap_or_else(|e| panic!("{e}"))[0];
    run(&p, sc)
}

fn errors(src: &str) -> Vec<String> {
    suture_lang::compile(&[("t.st".to_string(), src.to_string())]).err().unwrap_or_default().iter().map(|e| e.to_string()).collect()
}

const PEOPLE: &str = "  given  a Person Ann with name \"Ann\" and email \"ann@x.org\"
  and    a Person Bo with name \"Bo\" and email \"bo@x.org\"
  and    a Talk T with title \"Rust\"
  and    a Talk U with title \"Go\"
";

#[test]
fn a_reviewer_reviews_a_talk_once() {
    let r = runs(
        CFP,
        &format!(
            "{PEOPLE}  when   reviewer Bo scores a new Review R1 with talk T, reviewer Bo and score 4
  and    reviewer Bo scores a new Review R2 with talk T, reviewer Bo and score 5
         is refused by rule once
  and    reviewer Bo scores a new Review R3 with talk U, reviewer Bo and score 5
  and    reviewer Ann scores a new Review R4 with talk T, reviewer Ann and score 3
  expect R3's score is 5
  expect R4's score is 3
"
        ),
    );
    assert!(r.passed(), "{:?}\n{}", r.failures, r.host.trace.join("\n"));
    assert!(r.host.trace.iter().any(|l| l.contains("number of Reviews that are not withdrawn with the same reviewer Bo taken")), "{}", r.host.trace.join("\n"));
}

#[test]
fn an_entity_that_stops_holding_the_filter_frees_its_values() {
    let r = runs(
        CFP,
        &format!(
            "{PEOPLE}  when   reviewer Bo scores a new Review R1 with talk T, reviewer Bo and score 4
  and    reviewer Bo withdraws R1
  and    reviewer Bo scores a new Review R2 with talk T, reviewer Bo and score 2
  expect R2's score is 2
"
        ),
    );
    assert!(r.passed(), "{:?}\n{}", r.failures, r.host.trace.join("\n"));
}

#[test]
fn a_global_uniqueness_compares_texts_and_several_values() {
    let r = runs(
        CFP,
        "  when   staff Sam adds a new Person P1 with name \"Ann\" and email \"ann#1@x.org\"
  and    staff Sam adds a new Person P2 with name \"Ann B\" and email \"ann#1@x.org\"
         is refused by rule emails
  and    staff Sam adds a new Person P3 with name \"Cy\" and email \"ann#1@x.org.uk\"
  and    staff Sam adds a new Slot S1 with room \"A\", day 2026-03-01 and hour 9
  and    staff Sam adds a new Slot S2 with room \"A\", day 2026-03-01 and hour 10
  and    staff Sam adds a new Slot S3 with room \"B\", day 2026-03-01 and hour 9
  and    staff Sam adds a new Slot S4 with room \"A\", day 2026-03-01 and hour 9
         is refused by rule rooms
  expect P3's name is \"Cy\"
",
    );
    assert!(r.passed(), "{:?}\n{}", r.failures, r.host.trace.join("\n"));
}

#[test]
fn a_uniqueness_that_shares_its_total_still_refuses() {
    // `no two Reviews have the same talk` is the same count as `the number
    // of each Talk's Reviews`; whichever is written first, it refuses.
    for rules in [
        "rule one: no two Reviews have the same talk\nrule few: the number of each Talk's Reviews is at most 5\n",
        "rule few: the number of each Talk's Reviews is at most 5\nrule one: no two Reviews have the same talk\n",
    ] {
        let src = CFP.replace("rule once:", &format!("{rules}rule once:"));
        let r = runs(
            &src,
            &format!(
                "{PEOPLE}  when   reviewer Bo scores a new Review R1 with talk T, reviewer Bo and score 4
  and    reviewer Ann scores a new Review R2 with talk T, reviewer Ann and score 5
         is refused by rule one
"
            ),
        );
        assert!(r.passed(), "{:?}\n{}", r.failures, r.host.trace.join("\n"));
    }
}

#[test]
fn the_rule_reads_back_as_written_with_its_groups() {
    let p = compiled(CFP);
    let rule = |n: &str| p.rules.iter().find(|r| r.name == n).unwrap();
    assert_eq!(rule("once").paraphrase, "no two Reviews that are not withdrawn have the same talk and reviewer");
    assert_eq!(rule("once").tier, "per-group");
    assert!(rule("once").formula.starts_with("for each Talk t and each reviewer: "), "{}", rule("once").formula);
    assert_eq!(rule("rooms").tier, "global (bottleneck)");
    assert!(rule("rooms").formula.starts_with("for each room, day and hour: count(s in Slot where s.room = room and s.day = day and s.hour = hour)"), "{}", rule("rooms").formula);
}

#[test]
fn the_checker_names_what_cannot_be_compared() {
    let with = |rule: &str| errors(&format!("{CFP}rule bad: {rule}\n"));
    let first = |rule: &str| with(rule).first().cloned().unwrap_or_default();
    assert!(first("no two Review have the same talk").contains("name the kind in the plural, `no two Reviews`"));
    assert!(first("no two Reviews have the same mood").contains("has no attribute `mood`"));
    assert!(first("no two Reviews have the same talk and talk").contains("`talk` is named twice"));
    assert!(first("no two Reviews have the same talk and withdrawn").contains("can change after it is created"));
    assert!(first("no two Talks have the same title").contains("end the rule with `, checked globally`"));
    assert!(first("no two Reviews have the same talk, checked globally").contains("drop `, checked globally`"));
    assert!(first("no two Reviews has the same talk").contains("expected `have the same …`"));
    let secret = errors(&CFP.replace("email: Text", "email: Text secret").replace("rule rooms", "rule rooms2: no two Slots have the same hour, checked globally\nrule rooms"));
    assert!(secret.iter().any(|e| e.contains("is secret")), "{secret:?}");
}
