//! Slice 21: media for decision models (spec appendix V).

use std::path::PathBuf;

use serde_json::{json, Value};
use suture_lang::ir::Program;
use suture_lang::scenario::parse_scenarios;
use suture_runtime::choice::Choice;
use suture_runtime::host::Host;
use suture_runtime::http::{handle, Request};
use suture_runtime::scenario::run_with;
use suture_runtime::service::Service;
use suture_runtime::state::Val;

const T0: u64 = 1_800_000_000;

const CLAIMS: &str = "module claims version 1, currency EUR
role adjuster
kind Claim    { photo: Media, note: Text }
kind Repair   { claim: Claim }
kind Writeoff { claim: Claim }
Claim   needs Outcome
Outcome is    Repair or Writeoff
effect book Repair via garages within 1 day
  sends    the claim's note
  reports  booked
effect pay Writeoff via bank within 1 day
  sends    the claim's note
  reports  paid
prefer Outcome: Repair, then Writeoff
decide Outcome with model \"assessor@1\"
admit file a new Claim by adjuster
";

fn compile(src: &str) -> Result<Program, String> {
    suture_lang::compile(&[("claims.st".to_string(), src.to_string())]).map_err(|e| e.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("\n"))
}

/// Looks at the photo: a total loss is written off, anything else repaired.
fn assessor(h: &mut Host) {
    h.models.register(
        "assessor@1",
        Box::new(|c: &Choice| {
            let photo = c.facts.iter().find_map(|(k, v)| match v {
                Val::Media { sha256, .. } if k == "photo" => c.media.get(sha256),
                _ => None,
            })?;
            let total = photo.data.windows(5).any(|w| w == b"total");
            Some(if total { "Writeoff" } else { "Repair" }.to_string())
        }),
    );
}

fn dir(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("media").join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn a_model_sees_a_photo_from_a_scenario_and_picks_by_it() {
    let p = compile(CLAIMS).unwrap();
    let d = dir("scenario");
    std::fs::write(d.join("dent.png"), b"\x89PNG a small dent").unwrap();
    std::fs::write(d.join("wreck.png"), b"\x89PNG a total loss").unwrap();
    let src = "scenario \"photos\":
  when   adjuster Ann files a new Claim C1 with photo file \"dent.png\" and note \"door\"
  and    adjuster Ann files a new Claim C2 with photo file \"wreck.png\" and note \"roof\"
";
    let file = d.join("claims.scenario");
    let sc = &parse_scenarios(file.to_str().unwrap(), src).unwrap()[0];
    let r = run_with(&p, sc, assessor);
    assert!(r.passed(), "{:?}\n{}", r.failures, r.host.trace.join("\n"));
    let t = r.host.trace.join("\n");
    assert!(t.contains("photo (image/png, 17 bytes, sha256 "), "the trace shows the reference, not the content: {t}");
    assert!(t.contains("choice: Repair for C1?Outcome#1 by model \"assessor@1\""), "{t}");
    assert!(t.contains("choice: Writeoff for C2?Outcome#1 by model \"assessor@1\""), "{t}");
}

#[test]
fn a_scenario_file_must_exist_and_have_a_known_type() {
    let p = compile(CLAIMS).unwrap();
    let d = dir("missing");
    for (file, want) in [("gone.png", "gone.png\": "), ("notes.doc", "name a .png")] {
        let src = format!("scenario \"s\":\n  when   adjuster Ann files a new Claim C1 with photo file \"{file}\" and note \"x\"\n");
        let sc = &parse_scenarios(d.join("s.scenario").to_str().unwrap(), &src).unwrap()[0];
        let r = run_with(&p, sc, assessor);
        assert!(r.failures.iter().any(|f| f.contains(want)), "{:?}", r.failures);
    }
}

#[test]
fn no_sentence_reads_media() {
    for (extra, what) in [
        ("rule odd: no Claim whose photo is \"x\" is filed\n", "Claim's photo is media"),
        ("effect send Repair via shop within 1 day\n  sends the claim's photo\n  reports done\n", "is media"),
    ] {
        let src = CLAIMS.replace("effect book Repair via garages within 1 day\n  sends    the claim's note\n  reports  booked\n", if what == "is media" { "" } else { "effect book Repair via garages within 1 day\n  sends    the claim's note\n  reports  booked\n" });
        let e = compile(&format!("{src}{extra}")).err().unwrap_or_default();
        assert!(e.contains(what) && e.contains("only a decision model sees media"), "{extra}: {e}");
    }
    let e = compile(&format!("{CLAIMS}admit refile Claim by adjuster with scan: Media where the scan is \"x\"\n")).err().unwrap_or_default();
    assert!(e.contains("`the scan` is media"), "{e}");
}

#[test]
fn a_personal_photo_is_not_shown_to_the_model() {
    let p = compile(&CLAIMS.replace("photo: Media,", "photo: Media personal,")).unwrap();
    let d = dir("personal");
    std::fs::write(d.join("wreck.png"), b"\x89PNG a total loss").unwrap();
    let src = "scenario \"s\":\n  when   adjuster Ann files a new Claim C1 with photo file \"wreck.png\" and note \"x\"\n";
    let sc = &parse_scenarios(d.join("s.scenario").to_str().unwrap(), src).unwrap()[0];
    let r = run_with(&p, sc, assessor);
    let t = r.host.trace.join("\n");
    assert!(t.contains("choice: Repair for C1?Outcome#1 by prefer (model \"assessor@1\" declined)"), "the model saw no photo: {t}");
}

fn req(method: &str, path: &str, headers: &[(&str, &str)], body: &[u8]) -> Request {
    Request {
        method: method.into(),
        path: path.into(),
        query: String::new(),
        headers: headers.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        body: body.to_vec(),
    }
}

#[test]
fn served_media_is_uploaded_first_then_named_by_its_reference() {
    let p = compile(CLAIMS).unwrap();
    let state = dir("served");
    let mut svc = Service::open(&p, &state, T0, assessor).unwrap();
    let caller = ("suture-caller", "adjuster Ann");
    let (status, body) = handle(&mut svc, &req("POST", "/media", &[caller], b"\x89PNG total"), None, T0);
    assert_eq!(status, 400, "{body}");
    assert!(body["error"].as_str().unwrap().contains("Content-Type"), "{body}");
    let (status, photo) = handle(&mut svc, &req("POST", "/media", &[caller, ("content-type", "image/png")], b"\x89PNG total"), None, T0);
    assert_eq!(status, 200, "{photo}");
    assert_eq!(photo["mime"], "image/png");
    assert!(state.join("media").join(photo["sha256"].as_str().unwrap()).exists());
    let input = |photo: &Value| json!({ "new": { "verb": "files", "kind": "Claim", "label": "C1", "attrs": { "photo": photo, "note": "roof" } } }).to_string();
    let mut unknown = photo.clone();
    unknown["sha256"] = json!("0".repeat(64));
    let (status, body) = handle(&mut svc, &req("POST", "/inputs", &[caller], input(&unknown).as_bytes()), None, T0);
    assert_eq!(status, 409, "{body}");
    assert!(body["refused"].as_str().unwrap().contains("send it to POST /media first"), "{body}");
    let (status, body) = handle(&mut svc, &req("POST", "/inputs", &[caller], input(&photo).as_bytes()), None, T0);
    assert_eq!(status, 200, "{body}");
    let ticked = svc.tick(T0).unwrap();
    assert!(ticked.iter().any(|l| l.contains("choice: Writeoff") && l.contains("by model \"assessor@1\"")), "{ticked:?}");
    // The log holds the reference, never the content.
    let log = std::fs::read_to_string(state.join("log.jsonl")).unwrap();
    assert!(log.contains(photo["sha256"].as_str().unwrap()) && !log.contains("PNG total"), "{log}");
    let unit = svc.host.unit("C1");
    drop(svc);
    let again = Service::open(&p, &state, T0 + 1, |_| {}).unwrap();
    assert_eq!(again.host.unit("C1"), unit, "replay needs neither the model nor the media");
}
