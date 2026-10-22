//! Slice 50: yes or no, and scores, that a model reads from media (spec appendix AZ).

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use serde_json::{json, Value};
use suture_lang::ir::{Program, Ty};
use suture_lang::scenario::parse_scenarios;
use suture_runtime::backend::{Custom, Jev, MediaMode, Transport, MAX_MEDIA_SENT};
use suture_runtime::choice::{Answer, Backend, Reading};
use suture_runtime::scenario::{run, Report};
use suture_runtime::state::Val;

const CLAIM: &str = "module claims version 1, currency EUR
role staff
kind Claim  { photo: Media, note: Text, damaged: Bool, severity: Count }
kind Payout { claim: Claim }
Claim needs Payout
effect pay Payout via bank within 1 minute
  sends    the claim's note
  marks    sent
  reports  paid
source Claim's damaged via model \"inspector@1\" within 5 seconds
  sends      its photo
  asking     \"does the photo show damage to the car\"
source Claim's severity via model \"grader@1\" within 5 seconds
  sends      its photo, its note
  scored     from 1 to 5
rule covered: no Claim has a Payout that is sent unless it is damaged and its severity is at most 4
admit file a new Claim by staff
";

fn prog(src: &str) -> Program {
    suture_lang::compile(&[("claims.st".to_string(), src.to_string())]).unwrap_or_else(|e| panic!("{}", e.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("\n")))
}

fn errors(src: &str) -> String {
    suture_lang::compile(&[("claims.st".to_string(), src.to_string())]).err().unwrap_or_default().iter().map(|e| e.to_string()).collect::<Vec<_>>().join("\n")
}

/// Runs the steps beside a photo file.
fn scenario(p: &Program, name: &str, steps: &str) -> Report {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("judged").join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("dent.png"), b"\x89PNG a dent").unwrap();
    let src = format!("scenario \"s\":\n{steps}");
    let sc = &parse_scenarios(d.join("s.scenario").to_str().unwrap(), &src).unwrap_or_else(|e| panic!("{e}"))[0];
    run(p, sc)
}

const FILE: &str = "  and    staff Sam files a new Claim C1 with photo file \"dent.png\" and note \"door\"\n";

#[test]
fn a_yes_and_a_score_let_the_payout_go_ahead() {
    let r = scenario(&prog(CLAIM), "yes", &format!("  when   model \"inspector@1\" reads yes at 0.9\n  and    model \"grader@1\" reads 3\n{FILE}  expect C1 is damaged\n  expect C1's severity is 3\n  expect C1's Payout is sent\n"));
    assert!(r.passed(), "{:?}\n{}", r.failures, r.host.trace.join("\n"));
}

#[test]
fn a_no_holds_the_payout_and_a_score_off_the_scale_is_not_used() {
    let r = scenario(&prog(CLAIM), "no", &format!("  when   model \"inspector@1\" reads no\n  and    model \"grader@1\" reads 2\n{FILE}  expect C1 is not damaged\n  expect C1's Payout is not sent\n"));
    assert!(r.passed(), "{:?}\n{}", r.failures, r.host.trace.join("\n"));
    let r = scenario(&prog(CLAIM), "off", &format!("  when   model \"inspector@1\" reads yes\n  and    model \"grader@1\" reads 7\n{FILE}  expect C1's Payout is not sent\n"));
    assert!(r.passed(), "{:?}\n{}", r.failures, r.host.trace.join("\n"));
    let t = r.host.trace.join("\n");
    assert!(t.contains("it scored 7, outside the scale from 1 to 5"), "{t}");
}

#[test]
fn only_a_model_answers_yes_or_no_and_only_a_count_is_scored() {
    let e = errors(&CLAIM.replace("source Claim's damaged via model \"inspector@1\" within 5 seconds\n  sends      its photo\n  asking     \"does the photo show damage to the car\"\n", "source Claim's damaged via garage within 5 seconds\n  sends      its note\n  reports    found with damaged: Bool\n"));
    assert!(e.contains("an adapter sources values, not states"), "{e}");
    let e = errors(&CLAIM.replace("kind Claim  { photo: Media, note: Text, damaged: Bool, severity: Count }", "kind Claim  { photo: Media, note: Text, damaged: Bool, severity: Money }").replace("at most 4", "at most 4 EUR"));
    assert!(e.contains("only a count a model reads is scored"), "{e}");
    let e = errors(&CLAIM.replace("from 1 to 5", "from 5 to 1"));
    assert!(e.contains("a scale runs from a lower number to a higher one"), "{e}");
}

/// Answers with `reply`, and keeps the bodies it was sent.
#[derive(Clone)]
struct Fake {
    reply: Value,
    sent: Rc<RefCell<Vec<Value>>>,
}

impl Transport for Fake {
    fn post(&mut self, _: &str, _: Option<&str>, body: &Value, _: Duration) -> Result<Value, String> {
        self.sent.borrow_mut().push(body.clone());
        Ok(self.reply.clone())
    }
}

fn fake(reply: Value) -> Fake {
    Fake { reply, sent: Rc::default() }
}

fn reading(ty: Ty, scale: Option<(i64, i64)>) -> Reading {
    let mut store = suture_runtime::media::Store::default();
    let photo = store.put("image/png", b"\x89PNG dent".to_vec()).unwrap();
    let Val::Media { sha256, .. } = &photo else { panic!() };
    Reading {
        unit: "C1".into(),
        key: "C1~damaged#1".into(),
        kind: "Claim".into(),
        facts: vec![("photo".into(), photo.clone())],
        attr: "damaged".into(),
        ty,
        asking: Some("does the photo show damage".into()),
        scale,
        currency: "EUR".into(),
        media: [(sha256.clone(), store.get(sha256).unwrap())].into_iter().collect(),
    }
}

#[test]
fn jev_answers_yes_or_no_and_a_score_as_a_choice() {
    let within = Duration::from_secs(1);
    let jev = |f: &Fake| {
        let mut j = Jev::new(Box::new(f.clone()));
        j.key_env = None;
        j
    };
    let f = fake(json!({ "answers": { "pick": { "type": "choice", "choice": "yes", "confidence": 0.92 } } }));
    assert_eq!(jev(&f).extract(&reading(Ty::Bool, None), within), Answer::Read(Val::Bool(true), Some(0.92)));
    let q = f.sent.borrow()[0]["questions"]["pick"].clone();
    assert_eq!(q["type"], "choice");
    assert_eq!(q["criteria"], json!({ "yes": null, "no": null }));
    assert_eq!(q["instructions"], "does the photo show damage: yes or no for C1 (Claim). Answer one of: yes, no.");
    assert!(f.sent.borrow()[0]["state"]["facts"]["photo"]["base64"].is_string(), "the photo goes with its content");
    let f = fake(json!({ "answers": { "pick": { "choice": "4", "confidence": 0.8 } } }));
    assert_eq!(jev(&f).extract(&reading(Ty::Count, Some((1, 5))), within), Answer::Read(Val::Count(4), Some(0.8)));
    assert_eq!(f.sent.borrow()[0]["questions"]["pick"]["criteria"], json!({ "1": null, "2": null, "3": null, "4": null, "5": null }));
    // Jev picks: an amount, or a count with no scale, is not something it reads.
    for r in [reading(Ty::Money, None), reading(Ty::Count, None), reading(Ty::Count, Some((0, 1000)))] {
        assert!(matches!(jev(&fake(json!({}))).extract(&r, within), Answer::Failed(e) if e.contains("only yes or no and scores")));
    }
}

#[test]
fn a_custom_backend_is_told_the_form_and_reads_a_boolean() {
    let f = fake(json!({ "out": { "value": true, "p": 0.9 } }));
    let mut c = Custom { url: "https://m.example".into(), key_env: None, pick: String::new(), value: Some("/out/value".into()), confidence: Some("/out/p".into()), min_confidence: None, media: MediaMode::Omit, max_media_bytes: MAX_MEDIA_SENT, transport: Box::new(f.clone()) };
    assert_eq!(c.extract(&reading(Ty::Bool, None), Duration::from_secs(1)), Answer::Read(Val::Bool(true), Some(0.9)));
    assert_eq!(f.sent.borrow()[0]["field"], json!({ "name": "damaged", "type": "yes or no", "instructions": "does the photo show damage: yes or no" }));
    let f = fake(json!({ "out": { "value": 2 } }));
    c.transport = Box::new(f.clone());
    assert_eq!(c.extract(&reading(Ty::Count, Some((1, 5))), Duration::from_secs(1)), Answer::Read(Val::Count(2), None));
    assert_eq!(f.sent.borrow()[0]["field"]["scale"], json!({ "from": 1, "to": 5 }));
}
