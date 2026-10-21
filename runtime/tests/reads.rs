//! Slice 30: sources a model reads (spec appendix AF).

use suture_lang::ir::Program;
use suture_lang::scenario::parse_scenarios;
use suture_runtime::scenario::{run, Report};

const CLAIM: &str = "module claims version 1, currency EUR
role staff
kind Claim  { note: Text, estimate: Money, limit: Money = 5000 EUR }
kind Payout { claim: Claim }
Claim needs Payout
effect pay Payout via bank within 1 minute
  sends    the claim's limit
  marks    sent
  reports  paid
source Claim's estimate via model \"reader@1\" within 5 seconds
  sends      its note
  asking     \"the repair cost the garage estimates\"
  at least 0.8 sure
  otherwise  ask again after 1 hour
rule covered: no Claim has a Payout that is sent unless its estimate is at most its limit
admit file a new Claim by staff
";

fn prog(src: &str) -> Program {
    let files = vec![("claims.st".to_string(), src.to_string())];
    suture_lang::compile(&files).unwrap_or_else(|e| panic!("{}", e.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("\n")))
}

fn scenario(p: &Program, steps: &str) -> Report {
    let src = format!("scenario \"s\":\n{steps}");
    let sc = &parse_scenarios("s.scenario", &src).unwrap_or_else(|e| panic!("{e}"))[0];
    run(p, sc)
}

fn passes(p: &Program, steps: &str) -> Vec<String> {
    let r = scenario(p, steps);
    assert!(r.passed(), "{:?}\n{}", r.failures, r.host.trace.join("\n"));
    r.host.trace.clone()
}

fn has(trace: &[String], line: &str) -> bool {
    trace.iter().any(|l| l.contains(line))
}

#[test]
fn the_model_reads_the_estimate_and_the_payout_goes_ahead() {
    let t = passes(
        &prog(CLAIM),
        "  when   model \"reader@1\" reads 1200 EUR at 0.93
  and    staff Sam files a new Claim C1 with note \"bumper and left door, 1200 EUR\"
  expect C1's estimate is 1200 EUR
  expect C1's Payout is sent
",
    );
    assert!(has(&t, "model \"reader@1\" will read 1200 EUR at 0.93"), "{}", t.join("\n"));
    assert!(has(&t, "estimate: asking model \"reader@1\" to read it (note \"bumper and left door, 1200 EUR\")"), "{}", t.join("\n"));
    assert!(has(&t, "model \"reader@1\" read C1's estimate at 0.93"), "{}", t.join("\n"));
    assert!(has(&t, "reader@1: read, estimate 1200 EUR"), "{}", t.join("\n"));
}

#[test]
fn an_unsure_read_nothing_found_and_a_late_model_all_fail_the_fetch() {
    let p = prog(CLAIM);
    for (answer, why) in [
        ("reads 1200 EUR at 0.4", "it read 1200 EUR at 0.40, and the source needs at least 0.8 sure"),
        ("reads 1200 EUR", "it gave no confidence, and the source needs at least 0.8 sure"),
        ("reads nothing", "it found no estimate"),
        ("is late", "it did not answer within 5 seconds"),
        ("reads 12", "it gave 12, which is not a Money"),
    ] {
        let t = passes(
            &p,
            &format!(
                "  when   model \"reader@1\" {answer}
  and    staff Sam files a new Claim C1 with note \"a scratch\"
  expect C1's Payout is not sent
"
            ),
        );
        assert!(has(&t, &format!("model \"reader@1\" did not read C1's estimate: {why}")), "{answer}:\n{}", t.join("\n"));
        assert!(has(&t, "reader@1: unread"), "{}", t.join("\n"));
    }
}

#[test]
fn otherwise_asks_the_model_again() {
    let t = passes(
        &prog(CLAIM),
        "  when   model \"reader@1\" reads nothing, then 900 EUR at 0.9
  and    staff Sam files a new Claim C1 with note \"a scratch, 900 EUR\"
  expect C1's Payout is not sent
  when   1 hour passes
  expect C1's estimate is 900 EUR
  expect C1's Payout is sent
",
    );
    assert!(has(&t, "will read nothing, then 900 EUR at 0.9"), "{}", t.join("\n"));
}

#[test]
fn a_model_source_is_checked() {
    let errors = |src: &str| suture_lang::compile(&[("c.st".to_string(), src.to_string())]).err().map(|e| e.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("\n")).unwrap_or_default();
    let e = errors(&CLAIM.replace("  asking ", "  reports    found\n  asking "));
    assert!(e.contains("a source via a model has no `reports` line"), "{e}");
    let e = errors(&CLAIM.replace("at least 0.8 sure", "at least 1.5 sure"));
    assert!(e.contains("expected a confidence from 0 to 1"), "{e}");
    let e = errors(&CLAIM.replace("note: Text,", "note: Text personal,"));
    assert!(e.contains("Claim's note is personal, and a model is not sent marked attributes"), "{e}");
    // An adapter's `sends` still cannot read media; a model's can.
    let media = CLAIM.replace("note: Text,", "note: Text, photo: Media,");
    assert_eq!(errors(&media.replace("sends      its note", "sends      its note, its photo")), "");
    let e = errors(&media.replace("sends    the claim's limit", "sends    the claim's photo"));
    assert!(e.contains("is media"), "{e}");
    // The scenario step names a model the program uses.
    let r = scenario(&prog(CLAIM), "  when   model \"other@1\" reads nothing\n");
    assert_eq!(r.failures, vec!["step 1: no `decide` or source names model \"other@1\"".to_string()]);
}

#[test]
fn the_readback_states_the_model_and_its_bound() {
    let p = prog(CLAIM);
    let line = suture_lang::readback::source_line(&p, &p.sources[0]);
    assert!(line.starts_with("Claim's estimate read by model \"reader@1\" within 5 seconds (reads note; used only when at least 0.8 sure)"), "{line}");
    assert!(!suture_lang::manifest::adapters(&p).contains("reader@1"), "a model is not an adapter");
}

/// A post: the URL, the key and the body.
type Sent = (String, Option<String>, serde_json::Value);

/// Answers with `reply`, and keeps what it was sent; never the network.
#[derive(Clone)]
struct Fake {
    reply: Result<serde_json::Value, String>,
    sent: std::rc::Rc<std::cell::RefCell<Vec<Sent>>>,
}

impl suture_runtime::backend::Transport for Fake {
    fn post(&mut self, url: &str, key: Option<&str>, body: &serde_json::Value, _: std::time::Duration) -> Result<serde_json::Value, String> {
        self.sent.borrow_mut().push((url.into(), key.map(String::from), body.clone()));
        self.reply.clone()
    }
}

fn fake(reply: serde_json::Value) -> Fake {
    Fake { reply: Ok(reply), sent: Default::default() }
}

fn reading(ty: suture_lang::ir::Ty) -> suture_runtime::choice::Reading {
    use suture_runtime::state::Val;
    let mut store = suture_runtime::media::Store::default();
    let photo = store.put("image/png", b"\x89PNG dent".to_vec()).unwrap();
    let Val::Media { sha256, .. } = &photo else { panic!() };
    let media = [(sha256.clone(), store.get(sha256).unwrap())].into_iter().collect();
    suture_runtime::choice::Reading {
        unit: "C1".into(),
        key: "C1~estimate#1".into(),
        kind: "Claim".into(),
        facts: vec![("note".into(), Val::Text("a dent".into())), ("photo".into(), photo)],
        attr: "estimate".into(),
        ty,
        asking: Some("the repair cost".into()),
        currency: "EUR".into(),
        media,
    }
}

#[test]
fn workers_ai_sends_a_typed_field_and_the_image_and_reads_the_answer() {
    use serde_json::json;
    use suture_lang::ir::Ty;
    use suture_runtime::backend::WorkersAi;
    use suture_runtime::choice::{Answer, Backend};
    use suture_runtime::state::Val;
    let within = std::time::Duration::from_secs(1);
    let answer = |v: serde_json::Value| json!({ "result": { "answers": { "estimate": v } }, "success": true });
    let f = fake(answer(json!({ "value": 1200.5, "p_null": 0.001, "confidence": 0.97 })));
    let mut w = WorkersAi { url: "https://ai.example/run/@cf/cloudflare/clairic".into(), key_env: None, min_confidence: None, max_media_bytes: 1 << 20, transport: Box::new(f.clone()) };
    assert_eq!(w.extract(&reading(Ty::Money), within), Answer::Read(Val::Money(120_050), Some(0.97)));
    let body = f.sent.borrow()[0].2.clone();
    assert_eq!(body["state"], "Claim C1\nnote: a dent\nphoto: image 1");
    assert_eq!(body["images"], json!(["data:image/png;base64,iVBORyBkZW50"]));
    assert_eq!(body["fields"], json!({ "estimate": { "type": "number", "decimals": 2, "unit": "EUR", "instructions": "the repair cost" } }));
    // Likely absent: nothing found; below the backend's bound: unsure.
    w.transport = Box::new(fake(answer(json!({ "value": 3, "p_null": 0.8, "confidence": 0.9 }))));
    assert_eq!(w.extract(&reading(Ty::Money), within), Answer::Declined);
    w.transport = Box::new(fake(json!({ "answers": { "estimate": { "value": 1200, "p_null": 0.01, "confidence": 0.6 } } })));
    w.min_confidence = Some(0.7);
    assert_eq!(w.extract(&reading(Ty::Money), within), Answer::Unsure(0.6));
    // Dates come in ISO form.
    let f = fake(answer(json!({ "value": "2026-03-01T14:30:00Z", "p_null": 0.0, "confidence": 0.99 })));
    w.transport = Box::new(f.clone());
    let at = (31 + 28) * 86_400 + 14 * 3_600 + 30 * 60;
    assert_eq!(w.extract(&reading(Ty::Time), within), Answer::Read(Val::Time(at), Some(0.99)));
    assert_eq!(f.sent.borrow()[0].2["fields"]["estimate"]["type"], "datetime");
    // Workers AI reads; it does not pick.
    let c = suture_runtime::choice::Choice { point: "x".into(), unit: "C1".into(), key: "k".into(), chooser: "Claim".into(), facts: vec![], options: vec![], currency: "EUR".into(), media: Default::default() };
    assert!(matches!(w.decide(&c, within), Answer::Failed(_)));
}

#[test]
fn models_json_names_a_reading_backend() {
    use suture_runtime::backend;
    let t = || -> Box<dyn backend::Transport> { Box::new(fake(serde_json::json!({}))) };
    let named = backend::load(r#"{ "reader@1": { "backend": "workers-ai", "url": "https://ai.example/run", "min_confidence": 0.5 },
                                   "mine@1": { "backend": "custom", "url": "https://m.example", "value": "/value", "confidence": "/p" } }"#, t).unwrap();
    assert_eq!(named.len(), 2);
    let e = backend::load(r#"{ "reader@1": { "backend": "workers-ai" } }"#, t).err().unwrap();
    assert!(e.contains("needs a `url`"), "{e}");
    let e = backend::load(r#"{ "m@1": { "backend": "custom", "url": "https://m.example" } }"#, t).err().unwrap();
    assert!(e.contains("or `value` for a model that reads"), "{e}");
}

#[test]
fn a_custom_backend_reads_at_its_pointer() {
    use serde_json::json;
    use suture_lang::ir::Ty;
    use suture_runtime::backend::{Custom, MediaMode};
    use suture_runtime::choice::{Answer, Backend};
    use suture_runtime::state::Val;
    let f = fake(json!({ "out": { "value": "a dent", "p": 0.8 } }));
    let mut c = Custom { url: "https://m.example".into(), key_env: None, pick: String::new(), value: Some("/out/value".into()), confidence: Some("/out/p".into()), min_confidence: None, media: MediaMode::Omit, max_media_bytes: 0, transport: Box::new(f.clone()) };
    assert_eq!(c.extract(&reading(Ty::Text), std::time::Duration::from_secs(1)), Answer::Read(Val::Text("a dent".into()), Some(0.8)));
    let body = f.sent.borrow()[0].2.clone();
    assert_eq!(body["point"], "read");
    assert_eq!(body["field"], json!({ "name": "estimate", "type": "text", "instructions": "the repair cost" }));
    assert!(body["state"]["facts"]["photo"].get("base64").is_none(), "media omitted");
    c.transport = Box::new(fake(json!({ "out": { "value": null } })));
    assert_eq!(c.extract(&reading(Ty::Text), std::time::Duration::from_secs(1)), Answer::Declined);
}

/// Reads the amount out of the note: the first number in it, as EUR.
struct NoteReader;

impl suture_runtime::choice::Backend for NoteReader {
    fn decide(&mut self, _: &suture_runtime::choice::Choice, _: std::time::Duration) -> suture_runtime::choice::Answer {
        suture_runtime::choice::Answer::Declined
    }

    fn extract(&mut self, r: &suture_runtime::choice::Reading, _: std::time::Duration) -> suture_runtime::choice::Answer {
        use suture_runtime::state::Val;
        let note = r.facts.iter().find_map(|(k, v)| if let (true, Val::Text(t)) = (k == "note", v) { Some(t.clone()) } else { None }).unwrap_or_default();
        match note.split(|c: char| !c.is_ascii_digit()).find(|w| !w.is_empty()).and_then(|w| w.parse::<i64>().ok()) {
            Some(n) => suture_runtime::choice::Answer::Read(Val::Money(n * 100), Some(0.9)),
            None => suture_runtime::choice::Answer::Declined,
        }
    }
}

#[test]
fn a_served_program_reads_after_the_step_and_replays_the_read_from_its_log() {
    use suture_runtime::service::{self, Record, Service, Who};
    const T0: u64 = 1_800_000_000;
    let p = prog(CLAIM);
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("reads");
    let _ = std::fs::remove_dir_all(&dir);
    let mut svc = Service::open(&p, &dir, T0, |h| h.models.register_backend("reader@1", Box::new(NoteReader))).unwrap();
    let attrs = service::attrs_json(&p, "Claim", Some(&serde_json::json!({ "note": "door, 1200 EUR" }))).unwrap();
    let new = Record::New { by: Who { role: "staff".into(), name: "Sam".into() }, verb: "files".into(), kind: "Claim".into(), label: "C1".into(), attrs, items: vec![] };
    let applied = svc.input(new, T0).unwrap();
    assert!(!has(&applied.trace, "reader@1: read"), "not read in the input's step: {:?}", applied.trace);
    assert_eq!(svc.host.pending().len(), 1);
    let ticked = svc.tick(T0).unwrap();
    assert!(has(&ticked, "model \"reader@1\" read C1's estimate at 0.90"), "{ticked:?}");
    assert!(has(&ticked, "reader@1: read, estimate 1200 EUR"), "{ticked:?}");
    let log = std::fs::read_to_string(dir.join("log.jsonl")).unwrap();
    assert_eq!(log.matches("\"answer\"").count(), 1, "{log}");
    let unit = svc.host.unit("C1");
    drop(svc);
    // With no model registered, the read comes from the log.
    let mut again = Service::open(&p, &dir, T0 + 1, |_| {}).unwrap();
    assert_eq!(again.host.unit("C1"), unit);
    assert!(again.host.pending().iter().all(|a| a.verb != "read"));
    let ticked = again.tick(T0 + 1).unwrap();
    assert!(!has(&ticked, "reader@1"), "{ticked:?}");
}
