//! Slice 10: model backends (spec appendix K.2.6–K.2.7), through a fake transport.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use serde_json::{json, Value};
use suture_lang::scenario::parse_scenarios;
use suture_runtime::backend::{self, Custom, Jev, MediaMode, Transport, MAX_MEDIA_SENT};
use suture_runtime::choice::{Answer, Backend, Choice, Option_};
use suture_runtime::scenario::run;
use suture_runtime::state::Val;

/// A post: the URL, the key, the body and the timeout.
type Sent = (String, Option<String>, Value, Duration);

/// Answers with `reply`, and keeps what it was sent.
#[derive(Clone)]
struct Fake {
    reply: Result<Value, String>,
    sent: Rc<RefCell<Vec<Sent>>>,
}

impl Fake {
    fn new(reply: Result<Value, String>) -> Fake {
        Fake { reply, sent: Rc::default() }
    }
}

impl Transport for Fake {
    fn post(&mut self, url: &str, key: Option<&str>, body: &Value, timeout: Duration) -> Result<Value, String> {
        self.sent.borrow_mut().push((url.into(), key.map(String::from), body.clone(), timeout));
        self.reply.clone()
    }
}

fn choice() -> Choice {
    Choice {
        point: "Carrier".into(),
        unit: "O1".into(),
        key: "O1/Shipment#1@Carrier".into(),
        chooser: "Shipment".into(),
        facts: vec![("weight".into(), Val::Count(3))],
        options: vec![
            Option_ { label: "Cheap".into(), attrs: vec![("fee".into(), Val::Money(500)), ("zone".into(), Val::Text("EU".into()))] },
            Option_ { label: "Fast".into(), attrs: vec![("fee".into(), Val::Money(900)), ("zone".into(), Val::Text("EU".into()))] },
        ],
        currency: "EUR".into(),
        media: Default::default(),
    }
}

/// The documented answer to one `choice` question (docs.typesafe.ai/api).
fn jev_answer(pick: &str, confidence: f64) -> Value {
    json!({
        "model": "jev-1.13.0",
        "answers": { "pick": { "type": "choice", "choice": pick, "probabilities": { "Cheap": 1.0 - confidence, "Fast": confidence }, "confidence": confidence } },
        "usage": { "input_tokens": 120, "output_tokens": 1 }
    })
}

fn jev(fake: &Fake) -> Jev {
    let mut j = Jev::new(Box::new(fake.clone()));
    j.key_env = None;
    j
}

#[test]
fn jev_sends_the_documented_request() {
    let fake = Fake::new(Ok(jev_answer("Fast", 0.9)));
    let mut j = jev(&fake);
    assert_eq!(j.decide(&choice(), Duration::from_millis(300)), Answer::Picked("Fast".into()));
    let sent = fake.sent.borrow();
    let (url, key, body, timeout) = &sent[0];
    assert_eq!(url, "https://api.typesafe.ai/v1/systemone");
    assert_eq!(*key, None);
    assert_eq!(*timeout, Duration::from_millis(300));
    assert_eq!(body["model"], "jev-latest");
    assert_eq!(body["state"], json!({ "unit": "O1", "chooser": "Shipment", "facts": { "weight": 3 } }));
    let q = &body["questions"]["pick"];
    assert_eq!(q["type"], "choice");
    assert_eq!(q["criteria"], json!({ "Cheap": "fee 5 EUR, zone EU", "Fast": "fee 9 EUR, zone EU" }));
    assert_eq!(
        q["instructions"],
        "Choose the Carrier for O1 (Shipment). The options, best first by the program's own preference: Cheap, Fast."
    );
}

#[test]
fn jev_reads_the_confidence_and_fails_cleanly() {
    let mut j = jev(&Fake::new(Ok(jev_answer("Fast", 0.4))));
    j.min_confidence = Some(0.6);
    assert_eq!(j.decide(&choice(), Duration::from_secs(1)), Answer::Unsure(0.4));
    let mut j = jev(&Fake::new(Ok(json!({ "error": "bad" }))));
    assert_eq!(j.decide(&choice(), Duration::from_secs(1)), Answer::Failed("no answers.pick.choice in the answer".into()));
    let mut j = jev(&Fake::new(Err("HTTP 422: questions.pick.criteria".into())));
    assert_eq!(j.decide(&choice(), Duration::from_secs(1)), Answer::Failed("HTTP 422: questions.pick.criteria".into()));
    let mut j = jev(&Fake::new(Err(backend::TIMED_OUT.into())));
    assert_eq!(j.decide(&choice(), Duration::from_secs(1)), Answer::Late, "a transport's timeout is a late answer");
    let mut j = jev(&Fake::new(Ok(jev_answer("Fast", 0.9))));
    j.key_env = Some("SUTURE_TEST_NO_SUCH_KEY".into());
    assert_eq!(j.decide(&choice(), Duration::from_secs(1)), Answer::Failed("SUTURE_TEST_NO_SUCH_KEY is not set".into()));
}

#[test]
fn a_custom_format_reads_the_pick_at_its_pointer() {
    let fake = Fake::new(Ok(json!({ "result": { "pick": "Cheap", "p": 0.7 } })));
    let mut c = Custom {
        url: "https://models.example/route".into(),
        key_env: None,
        pick: "/result/pick".into(),
        value: None,
        confidence: Some("/result/p".into()),
        min_confidence: Some(0.5),
        media: MediaMode::Inline,
        max_media_bytes: MAX_MEDIA_SENT,
        transport: Box::new(fake.clone()),
    };
    assert_eq!(c.decide(&choice(), Duration::from_secs(1)), Answer::Picked("Cheap".into()));
    let body = fake.sent.borrow()[0].2.clone();
    assert_eq!(body["point"], "Carrier");
    assert_eq!(body["options"][1], json!({ "label": "Fast", "facts": { "fee": "9 EUR", "zone": "EU" } }));
    c.min_confidence = Some(0.8);
    assert_eq!(c.decide(&choice(), Duration::from_secs(1)), Answer::Unsure(0.7));
    c.pick = "/nowhere".into();
    assert_eq!(c.decide(&choice(), Duration::from_secs(1)), Answer::Failed("no text at /nowhere in the answer".into()));
}

#[test]
fn models_json_names_backends() {
    let fake = Fake::new(Ok(jev_answer("Fast", 0.9)));
    let text = r#"{
        "routing@1": { "model": "jev-1.14", "key_env": "JEV_KEY", "min_confidence": 0.6 },
        "sources@1": { "backend": "custom", "url": "https://models.example/s", "pick": "/pick" }
    }"#;
    let loaded = backend::load(text, || Box::new(fake.clone())).unwrap();
    let names: Vec<&str> = loaded.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["routing@1", "sources@1"]);
    let bad = |t: &str| backend::load(t, || Box::new(fake.clone())).err().unwrap();
    assert_eq!(bad(r#"{"m@1": {"backend": "magic"}}"#), "models.json: m@1: unknown backend `magic`; use \"jev\", \"custom\" or \"workers-ai\"");
    assert_eq!(bad(r#"{"m@1": {"modle": "jev-latest"}}"#), "models.json: m@1: unknown field `modle` for backend `jev`");
    assert_eq!(bad(r#"{"m@1": {"backend": "custom", "url": "u"}}"#), "models.json: m@1: a custom backend needs `pick`, a JSON pointer such as \"/result/pick\", or `value` for a model that reads");
    assert_eq!(bad(r#"{"m@1": {"min_confidence": 2}}"#), "models.json: m@1: `min_confidence` must be a number from 0 to 1");
}

#[test]
fn a_jev_backend_decides_a_choice_in_a_running_program() {
    let src = "module shop version 1, currency EUR
kind Carrier  { name: Text, zone: Text, fee: Money }
kind Order    { zone: Text }
kind Shipment { order: Order }
Order needs Shipment
Shipment uses a Carrier where the Carrier's zone is the Order's zone
prefer Carrier: lowest fee
decide Carrier with model \"carriers@1\" within 500 milliseconds
";
    let p = suture_lang::compile(&[("shop.st".to_string(), src.to_string())]).unwrap_or_else(|e| panic!("{e:?}"));
    let sc = &parse_scenarios(
        "s.scenario",
        "scenario \"s\":\n  given  a Carrier Cheap with name Cheap, zone EU and fee 5 EUR\n  and    a Carrier Fast with name Fast, zone EU and fee 9 EUR\n",
    )
    .unwrap()[0];
    let mut r = run(&p, sc);
    let fake = Fake::new(Ok(jev_answer("Fast", 0.9)));
    r.host.models.register_backend("carriers@1", Box::new(jev(&fake)));
    r.host.input_new(None, "Order", "O1", &[("zone".into(), Val::Text("EU".into()))], &[]).unwrap();
    let t = r.host.trace.join("\n");
    assert!(t.contains("choice: Fast for O1/Shipment#1@Carrier by model \"carriers@1\" (of Cheap, Fast)"), "{t}");
    assert_eq!(fake.sent.borrow()[0].3, Duration::from_millis(500));
}

fn with_photo(store_has_it: bool) -> Choice {
    let mut c = choice();
    let mut store = suture_runtime::media::Store::default();
    let photo = store.put("image/png", b"\x89PNG dent".to_vec()).unwrap();
    let Val::Media { sha256, .. } = &photo else { panic!() };
    if store_has_it {
        c.media.insert(sha256.clone(), store.get(sha256).unwrap());
    }
    c.facts.push(("photo".into(), photo));
    c
}

#[test]
fn media_goes_to_the_model_with_its_content_or_as_a_reference_only() {
    let fake = Fake::new(Ok(jev_answer("Fast", 0.9)));
    let mut j = Jev::new(Box::new(fake.clone()));
    j.key_env = None;
    assert_eq!(j.decide(&with_photo(true), Duration::from_secs(1)), Answer::Picked("Fast".into()));
    let photo = fake.sent.borrow()[0].2["state"]["facts"]["photo"].clone();
    assert_eq!(photo["media"], "image/png");
    assert_eq!(photo["bytes"], 9);
    assert_eq!(photo["sha256"], suture_runtime::media::sha256(b"\x89PNG dent"));
    assert_eq!(photo["base64"], "iVBORyBkZW50");
    j.media = MediaMode::Omit;
    j.decide(&with_photo(false), Duration::from_secs(1));
    let photo = fake.sent.borrow()[1].2["state"]["facts"]["photo"].clone();
    assert!(photo.get("base64").is_none() && photo["media"] == "image/png", "{photo}");
}

#[test]
fn media_the_model_cannot_be_sent_leaves_the_choice_to_prefer_without_asking() {
    let fake = Fake::new(Ok(jev_answer("Fast", 0.9)));
    let mut j = Jev::new(Box::new(fake.clone()));
    j.key_env = None;
    assert!(matches!(j.decide(&with_photo(false), Duration::from_secs(1)), Answer::Failed(e) if e.contains("is not in the media store")));
    j.max_media_bytes = 4;
    assert!(matches!(j.decide(&with_photo(true), Duration::from_secs(1)), Answer::Failed(e) if e.contains("the media is 9 bytes, over the 4 this model takes")));
    assert!(fake.sent.borrow().is_empty(), "the model is not asked");
}

#[test]
fn models_json_sets_how_media_is_sent() {
    let named = backend::load(r#"{"a@1": {"media": "omit", "max_media_bytes": 10}}"#, || Box::new(Fake::new(Ok(json!({}))))).unwrap();
    assert_eq!(named.len(), 1);
    let e = backend::load(r#"{"a@1": {"media": "all"}}"#, || Box::new(Fake::new(Ok(json!({}))))).err().unwrap();
    assert!(e.contains("`media` is \"inline\" or \"omit\""), "{e}");
}
