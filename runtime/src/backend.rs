//! Model backends (appendix K.2.6–K.2.7): the Jev decision API by default, a
//! custom JSON format for other APIs, and the transport they post through.
//! The transport is a trait, so tests never reach the network.

use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Duration;

use base64::Engine;
use serde_json::{json, Map, Value};

use crate::choice::{confident, Answer, Backend, Choice, Reading};
use crate::state::Val;
use suture_lang::ir::Ty;

/// Jev's decision endpoint (docs.typesafe.ai/api).
pub const JEV_URL: &str = "https://api.typesafe.ai/v1/systemone";
pub const JEV_MODEL: &str = "jev-latest";
pub const JEV_KEY_ENV: &str = "JEV_API_KEY";

/// The error a transport gives when its timeout ran out: the model is late.
pub const TIMED_OUT: &str = "timed out";

/// Posts a JSON body and returns the JSON answer, within `timeout`.
pub trait Transport {
    /// `key`: the bearer token, if any. An HTTP error status is an error,
    /// and a timeout is the error [`TIMED_OUT`].
    fn post(&mut self, url: &str, key: Option<&str>, body: &Value, timeout: Duration) -> Result<Value, String>;
}

/// The real transport: `curl`, given the headers and body on its standard
/// input, so the key never shows in a process listing.
pub struct Curl;

impl Transport for Curl {
    fn post(&mut self, url: &str, key: Option<&str>, body: &Value, timeout: Duration) -> Result<Value, String> {
        let quote = |s: &str| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""));
        let mut config = format!("url = {}\nheader = \"Content-Type: application/json\"\n", quote(url));
        if let Some(k) = key {
            config.push_str(&format!("header = {}\n", quote(&format!("Authorization: Bearer {k}"))));
        }
        config.push_str(&format!("data-binary = {}\n", quote(&body.to_string())));
        let mut child = Command::new("curl")
            // curl reads `-m 0` as no limit, so never less than a millisecond.
            // `-q` first, so no ~/.curlrc is read; only HTTP(S), no redirects,
            // and a bounded answer (appendix BB.2.7).
            .args(["-q", "-sS", "--proto", "=https,http", "--max-redirs", "0", "--max-filesize", "4194304", "-K", "-", "-m", &format!("{:.3}", timeout.max(Duration::from_millis(1)).as_secs_f64()), "-w", "\n%{http_code}"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("cannot run curl: {e}"))?;
        child.stdin.take().expect("piped").write_all(config.as_bytes()).map_err(|e| e.to_string())?;
        let out = child.wait_with_output().map_err(|e| e.to_string())?;
        // curl's exit code 28: the operation timed out.
        if out.status.code() == Some(28) {
            return Err(TIMED_OUT.into());
        }
        // What curl or the service says goes to the trace, which callers can
        // read, so only its kind is kept: never a body that may echo the request.
        if !out.status.success() {
            return Err(format!("curl failed with exit code {}", out.status.code().unwrap_or(-1)));
        }
        let text = String::from_utf8_lossy(&out.stdout);
        let (body, status) = text.rsplit_once('\n').unwrap_or(("", &text));
        match status.trim().parse::<u16>() {
            Ok(s) if (200..300).contains(&s) => serde_json::from_str(body).map_err(|e| format!("not JSON: {e}")),
            Ok(s) => Err(format!("HTTP {s}")),
            Err(_) => Err(format!("no HTTP status in curl's output: {status}")),
        }
    }
}

/// A fact as JSON: text and numbers as such, amounts and times as shown,
/// and media by its reference.
pub fn fact(v: &Val, currency: &str) -> Value {
    match v {
        Val::Media { sha256, mime, bytes } => json!({ "media": mime, "sha256": sha256, "bytes": bytes }),
        Val::Text(s) | Val::Ref(s) => json!(s),
        Val::Bool(b) => json!(b),
        Val::Count(n) => json!(n),
        Val::Set(xs) => json!(xs),
        Val::Time(t) => json!(suture_lang::sentence::rfc3339(*t)),
        Val::Duration(d) => json!(suture_lang::sentence::iso_duration_text(*d)),
        _ => json!(v.show(currency)),
    }
}

/// A fact in an option's description: text as written, the rest as shown.
fn shown(v: &Val, currency: &str) -> String {
    match v {
        Val::Text(s) | Val::Ref(s) => s.clone(),
        _ => v.show(currency),
    }
}

/// Whether a backend is sent the content of media, or only its reference
/// (appendix V.2.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaMode {
    /// The content, in base64, beside the reference.
    Inline,
    /// The reference alone: type, size and hash.
    Omit,
}

/// How much media content one choice may send: 6 MiB, so that the request,
/// in base64, stays within the 10 MB line that curl reads its body from.
pub const MAX_MEDIA_SENT: usize = 6 * 1024 * 1024;

/// A fact as a backend gets it: media with its content when inlined.
fn sent(v: &Val, c: &Choice, mode: MediaMode) -> Value {
    sent_with(v, &c.currency, &c.media, mode)
}

fn sent_with(v: &Val, currency: &str, media: &std::collections::BTreeMap<String, crate::media::Blob>, mode: MediaMode) -> Value {
    let mut out = fact(v, currency);
    if let (Val::Media { sha256, .. }, MediaMode::Inline) = (v, mode)
        && let Some(b) = media.get(sha256)
    {
        out["base64"] = json!(base64::engine::general_purpose::STANDARD.encode(b.data.as_slice()));
    }
    out
}

fn facts(xs: &[(String, Val)], c: &Choice, mode: MediaMode) -> Value {
    Value::Object(xs.iter().map(|(k, v)| (k.clone(), sent(v, c, mode))).collect())
}

/// The state both formats send: who chooses, with their facts.
fn state(c: &Choice, mode: MediaMode) -> Value {
    json!({ "unit": c.unit, "chooser": c.chooser, "facts": facts(&c.facts, c, mode) })
}

/// Why a choice cannot be sent with its media: content the store does not
/// have, or more of it than the backend takes. The model is then not asked,
/// and `prefer` picks (appendix V.2.3).
fn media_unsent(c: &Choice, mode: MediaMode, max: usize) -> Option<String> {
    if mode == MediaMode::Omit {
        return None;
    }
    let mut total = 0;
    for h in c.media_named() {
        let Some(b) = c.media.get(h) else { return Some(format!("the media sha256 {} is not in the media store", &h[..12])) };
        total += b.data.len();
    }
    (total > max).then(|| format!("the media is {total} bytes, over the {max} this model takes"))
}

/// Like [`media_unsent`], for a reading.
fn reading_unsent(r: &Reading, mode: MediaMode, max: usize) -> Option<String> {
    if mode == MediaMode::Omit {
        return None;
    }
    let mut total = 0;
    for h in r.media_named() {
        let Some(b) = r.media.get(h) else { return Some(format!("the media sha256 {} is not in the media store", &h[..12])) };
        total += b.data.len();
    }
    (total > max).then(|| format!("the media is {total} bytes, over the {max} this model takes"))
}

/// A value a model read, as the attribute's type (appendix AF.2.5): text,
/// a whole number, an amount as a number of the module's currency, a time
/// in ISO form, a duration in seconds.
pub fn read_value(v: &Value, ty: Ty, currency: &str) -> Result<Val, String> {
    let bad = |what: &str| format!("{v} is not {what}");
    match (ty, v) {
        (Ty::Text, Value::String(s)) => Ok(Val::Text(s.clone())),
        // Yes or no (appendix AZ): a JSON boolean, or the word.
        (Ty::Bool, Value::Bool(b)) => Ok(Val::Bool(*b)),
        (Ty::Bool, Value::String(s)) if s == "yes" || s == "no" => Ok(Val::Bool(s == "yes")),
        (Ty::Bool, _) => Err(bad("yes or no")),
        (Ty::Count, Value::Number(n)) => match (n.as_i64(), n.as_f64()) {
            (Some(i), _) => Ok(Val::Count(i)),
            (None, Some(f)) if f.fract() == 0.0 && f.abs() < 9e15 => Ok(Val::Count(f as i64)),
            _ => Err(bad("a whole number")),
        },
        (Ty::Money, Value::Number(n)) => match n.as_f64() {
            Some(f) if f.is_finite() && f.abs() < 9e13 => Ok(Val::Money((f * 100.0).round() as i64)),
            _ => Err(bad(&format!("an amount of {currency}"))),
        },
        (Ty::Time, Value::String(s)) => suture_lang::sentence::iso_time(s).map(Val::Time).ok_or_else(|| bad("a date or an RFC 3339 time, as in \"2026-03-01\"")),
        (Ty::Duration, Value::Number(n)) => match n.as_f64() {
            Some(f) if f.is_finite() && (0.0..9e15).contains(&f) => Ok(Val::Duration(f.round() as i64)),
            _ => Err(bad("a duration in seconds")),
        },
        (Ty::Duration, Value::String(s)) => suture_lang::sentence::iso_duration(s).filter(|d| *d >= 0).map(Val::Duration).ok_or_else(|| bad("a duration in seconds, or as ISO 8601 such as \"PT2H\"")),
        (Ty::Text, _) => Err(bad("text")),
        (Ty::Count, _) => Err(bad("a whole number")),
        (Ty::Money, _) => Err(bad(&format!("an amount of {currency}, as a number"))),
        (Ty::Time, _) => Err(bad("a date or an RFC 3339 time, as in \"2026-03-01\"")),
        (Ty::Duration, _) => Err(bad("a duration in seconds, or as ISO 8601 such as \"PT2H\"")),
        (t, _) => Err(format!("a model does not read a {t:?}")),
    }
}

/// The value read, or nothing found, with the confidence bound applied.
fn read_answer(value: Option<&Value>, confidence: Option<f64>, r: &Reading, min: Option<f64>) -> Answer {
    let Some(v) = value.filter(|v| !v.is_null()) else { return Answer::Declined };
    match read_value(v, r.ty, &r.currency) {
        Ok(val) => match (confidence, min) {
            (Some(c), Some(m)) if c < m => Answer::Unsure(c),
            _ => Answer::Read(val, confidence),
        },
        Err(e) => Answer::Failed(e),
    }
}

fn key_from(env: Option<&str>) -> Result<Option<String>, String> {
    match env {
        None => Ok(None),
        Some(e) => std::env::var(e).map(Some).map_err(|_| format!("{e} is not set")),
    }
}


/// The Jev decision API: one `choice` question whose criteria are the options.
pub struct Jev {
    pub url: String,
    pub model: String,
    /// The environment variable holding the API key.
    pub key_env: Option<String>,
    /// Below this confidence, the pick is not used.
    pub min_confidence: Option<f64>,
    pub media: MediaMode,
    pub max_media_bytes: usize,
    pub transport: Box<dyn Transport>,
}

impl Jev {
    pub fn new(transport: Box<dyn Transport>) -> Jev {
        Jev {
            url: JEV_URL.into(),
            model: JEV_MODEL.into(),
            key_env: Some(JEV_KEY_ENV.into()),
            min_confidence: None,
            media: MediaMode::Inline,
            max_media_bytes: MAX_MEDIA_SENT,
            transport,
        }
    }

    /// The request (docs.typesafe.ai/api): the chooser as the state, and a
    /// `choice` question named `pick`. A JSON object has no order, so the
    /// instructions give `prefer`'s.
    pub fn request(&self, c: &Choice) -> Value {
        let labels: Vec<&str> = c.options.iter().map(|o| o.label.as_str()).collect();
        let criteria: Map<String, Value> = c
            .options
            .iter()
            .map(|o| {
                // An option's media goes with its facts, as an option has no description of media.
                let desc = if o.attrs.iter().any(|(_, v)| matches!(v, Val::Media { .. })) {
                    facts(&o.attrs, c, self.media)
                } else if o.attrs.is_empty() {
                    Value::Null
                } else {
                    json!(o.attrs.iter().map(|(k, v)| format!("{k} {}", shown(v, &c.currency))).collect::<Vec<_>>().join(", "))
                };
                (o.label.clone(), desc)
            })
            .collect();
        let mut instructions = format!(
            "Choose the {} for {} ({}). The options, best first by the program's own preference: {}.",
            c.point,
            c.unit,
            c.chooser,
            labels.join(", ")
        );
        // A router picks the model that will choose among these (appendix AY).
        if !c.among.is_empty() {
            let among: Vec<String> = c.among.iter().map(|o| if o.attrs.is_empty() { o.label.clone() } else { format!("{} ({})", o.label, o.attrs.iter().map(|(k, v)| format!("{k} {}", shown(v, &c.currency))).collect::<Vec<_>>().join(", ")) }).collect();
            instructions += &format!(" The model you choose will choose among: {}.", among.join("; "));
        }
        json!({
            "model": self.model,
            "state": state(c, self.media),
            "questions": { "pick": { "type": "choice", "instructions": instructions, "criteria": criteria } },
        })
    }

    /// A reading as one `choice` question: `yes` or `no`, or each point of
    /// its scale. `None` for any other value.
    pub fn read_request(&self, r: &Reading) -> Option<Value> {
        let answers: Vec<String> = match (r.ty, r.scale) {
            (Ty::Bool, _) => vec!["yes".into(), "no".into()],
            (Ty::Count, Some((a, b))) if b - a < 100 => (a..=b).map(|n| n.to_string()).collect(),
            _ => return None,
        };
        let criteria: Map<String, Value> = answers.iter().map(|a| (a.clone(), Value::Null)).collect();
        let facts: Map<String, Value> = r.facts.iter().map(|(k, v)| (k.clone(), sent_with(v, &r.currency, &r.media, self.media))).collect();
        let text = format!("{} for {} ({}). Answer one of: {}.", instructions(r), r.unit, r.kind, answers.join(", "));
        Some(json!({
            "model": self.model,
            "state": { "unit": r.unit, "chooser": r.kind, "facts": facts },
            "questions": { "pick": { "type": "choice", "instructions": text, "criteria": criteria } },
        }))
    }

    /// Reads `answers.pick.choice` and `answers.pick.confidence`.
    pub fn read(&self, answer: &Value) -> Answer {
        self.read_bounded(answer, None)
    }

    /// [`Jev::read`], held to the program's `at least … sure` too (appendix AK).
    pub fn read_bounded(&self, answer: &Value, sure: Option<f64>) -> Answer {
        let q = &answer["answers"]["pick"];
        let Some(pick) = q["choice"].as_str() else { return Answer::Failed("no answers.pick.choice in the answer".into()) };
        confident(pick.to_string(), q["confidence"].as_f64(), self.min_confidence, sure)
    }
}

impl Backend for Jev {
    fn decide(&mut self, c: &Choice, within: Duration) -> Answer {
        if let Some(why) = media_unsent(c, self.media, self.max_media_bytes) {
            return Answer::Failed(why);
        }
        let key = match key_from(self.key_env.as_deref()) {
            Ok(k) => k,
            Err(e) => return Answer::Failed(e),
        };
        let body = self.request(c);
        match self.transport.post(&self.url, key.as_deref(), &body, within) {
            Ok(v) => self.read_bounded(&v, c.sure),
            Err(e) if e == TIMED_OUT => Answer::Late,
            Err(e) => Answer::Failed(e),
        }
    }

    /// A yes or no, or a score, as a `choice` among its answers (appendix
    /// AZ.2.3). Jev picks, so it reads nothing else.
    fn extract(&mut self, r: &Reading, within: Duration) -> Answer {
        let Some(body) = self.read_request(r) else { return Answer::Failed("Jev picks; of the values a model reads, it answers only yes or no and scores of at most 100 points".into()) };
        if let Some(why) = reading_unsent(r, self.media, self.max_media_bytes) {
            return Answer::Failed(why);
        }
        let key = match key_from(self.key_env.as_deref()) {
            Ok(k) => k,
            Err(e) => return Answer::Failed(e),
        };
        match self.transport.post(&self.url, key.as_deref(), &body, within) {
            Ok(v) => {
                let q = &v["answers"]["pick"];
                let Some(pick) = q["choice"].as_str() else { return Answer::Failed("no answers.pick.choice in the answer".into()) };
                let value = match r.ty {
                    Ty::Bool => json!(pick),
                    _ => pick.parse::<i64>().map_or(json!(pick), |n| json!(n)),
                };
                read_answer(Some(&value), q["confidence"].as_f64(), r, self.min_confidence)
            }
            Err(e) if e == TIMED_OUT => Answer::Late,
            Err(e) => Answer::Failed(e),
        }
    }
}

/// Another API: the same state, the options as a list, and the pick (and the
/// confidence) read at JSON pointers.
pub struct Custom {
    pub url: String,
    pub key_env: Option<String>,
    /// `/result/pick`; empty for a model that only reads.
    pub pick: String,
    /// `/result/value`: where a value read is (appendix AF.2.5).
    pub value: Option<String>,
    pub confidence: Option<String>,
    pub min_confidence: Option<f64>,
    pub media: MediaMode,
    pub max_media_bytes: usize,
    pub transport: Box<dyn Transport>,
}

impl Custom {
    pub fn request(&self, c: &Choice) -> Value {
        let list = |xs: &[crate::choice::Option_]| -> Vec<Value> { xs.iter().map(|o| json!({ "label": o.label, "facts": facts(&o.attrs, c, self.media) })).collect() };
        let mut out = json!({ "point": c.point, "state": state(c, self.media), "options": list(&c.options) });
        // A router also gets what the model it picks will choose among (appendix AY).
        if !c.among.is_empty() {
            out["among"] = json!(list(&c.among));
        }
        out
    }

    /// A reading: the same state, and the field to read.
    pub fn read_request(&self, r: &Reading) -> Value {
        let facts: Map<String, Value> = r.facts.iter().map(|(k, v)| (k.clone(), sent_with(v, &r.currency, &r.media, self.media))).collect();
        let mut field = json!({ "name": r.attr, "type": type_name(r.ty), "instructions": instructions(r) });
        if let Some((a, b)) = r.scale {
            field["scale"] = json!({ "from": a, "to": b });
        }
        json!({ "point": "read", "state": { "unit": r.unit, "kind": r.kind, "facts": facts }, "field": field })
    }

    pub fn read(&self, answer: &Value) -> Answer {
        self.read_bounded(answer, None)
    }

    /// [`Custom::read`], held to the program's `at least … sure` too (appendix AK).
    pub fn read_bounded(&self, answer: &Value, sure: Option<f64>) -> Answer {
        if self.pick.is_empty() {
            return Answer::Failed("this model reads values; it has no `pick` pointer".into());
        }
        let Some(pick) = answer.pointer(&self.pick).and_then(Value::as_str) else {
            return Answer::Failed(format!("no text at {} in the answer", self.pick));
        };
        let confidence = self.confidence.as_deref().and_then(|p| answer.pointer(p)).and_then(Value::as_f64);
        confident(pick.to_string(), confidence, self.min_confidence, sure)
    }
}

impl Backend for Custom {
    fn decide(&mut self, c: &Choice, within: Duration) -> Answer {
        if let Some(why) = media_unsent(c, self.media, self.max_media_bytes) {
            return Answer::Failed(why);
        }
        let key = match key_from(self.key_env.as_deref()) {
            Ok(k) => k,
            Err(e) => return Answer::Failed(e),
        };
        let body = self.request(c);
        match self.transport.post(&self.url, key.as_deref(), &body, within) {
            Ok(v) => self.read_bounded(&v, c.sure),
            Err(e) if e == TIMED_OUT => Answer::Late,
            Err(e) => Answer::Failed(e),
        }
    }

    /// Reads the value at `value`, and the confidence at `confidence`.
    fn extract(&mut self, r: &Reading, within: Duration) -> Answer {
        let Some(at) = self.value.clone() else { return Answer::Failed("this model has no `value` pointer, so it does not read values".into()) };
        if let Some(why) = reading_unsent(r, self.media, self.max_media_bytes) {
            return Answer::Failed(why);
        }
        let key = match key_from(self.key_env.as_deref()) {
            Ok(k) => k,
            Err(e) => return Answer::Failed(e),
        };
        let body = self.read_request(r);
        match self.transport.post(&self.url, key.as_deref(), &body, within) {
            Ok(v) => {
                let confidence = self.confidence.as_deref().and_then(|p| v.pointer(p)).and_then(Value::as_f64);
                read_answer(v.pointer(&at), confidence, r, self.min_confidence)
            }
            Err(e) if e == TIMED_OUT => Answer::Late,
            Err(e) => Answer::Failed(e),
        }
    }
}

/// A type as the custom format names it.
fn type_name(t: Ty) -> &'static str {
    match t {
        Ty::Text => "text",
        Ty::Count => "count",
        Ty::Money => "money",
        Ty::Time => "time",
        Ty::Duration => "duration",
        Ty::Bool => "yes or no",
        _ => "other",
    }
}

/// What the model is asked: the source's `asking`, or the attribute's name,
/// with the answer's form for a yes or no and a score (appendix AZ).
fn instructions(r: &Reading) -> String {
    let what = r.asking.clone().unwrap_or_else(|| format!("the {}'s {}", r.kind, r.attr));
    match (r.ty, r.scale) {
        (Ty::Bool, _) => format!("{what}: yes or no"),
        (_, Some((a, b))) => format!("{what}: a score from {a} to {b}"),
        _ => what,
    }
}

/// Cloudflare Workers AI's extraction model, Clairic (appendix AF.2.5): the
/// facts as the state's text, images beside it, and one typed field.
pub struct WorkersAi {
    pub url: String,
    pub key_env: Option<String>,
    pub min_confidence: Option<f64>,
    pub max_media_bytes: usize,
    pub transport: Box<dyn Transport>,
}

/// Workers AI's own key variable, by default.
pub const WORKERS_AI_KEY_ENV: &str = "CLOUDFLARE_API_TOKEN";
/// The images one request takes.
pub const MAX_IMAGES: usize = 4;

impl WorkersAi {
    /// `{state, images, fields}`, or why the reading cannot be sent.
    pub fn request(&self, r: &Reading) -> Result<Value, String> {
        let mut state = format!("{} {}", r.kind, r.unit);
        let mut images = vec![];
        for (k, v) in &r.facts {
            if let Val::Media { sha256, mime, .. } = v {
                if !mime.starts_with("image/") {
                    return Err(format!("{k} is {mime}; this model reads text and images"));
                }
                let b = r.media.get(sha256).ok_or_else(|| format!("the media sha256 {} is not in the media store", &sha256[..12]))?;
                images.push(json!(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(b.data.as_slice()))));
                state.push_str(&format!("\n{k}: image {}", images.len()));
            } else {
                state.push_str(&format!("\n{k}: {}", shown(v, &r.currency)));
            }
        }
        if images.len() > MAX_IMAGES {
            return Err(format!("{} images, over the {MAX_IMAGES} this model takes", images.len()));
        }
        let mut field = json!({ "instructions": instructions(r) });
        match r.ty {
            Ty::Text => field["type"] = json!("text"),
            Ty::Bool => field["type"] = json!("boolean"),
            Ty::Count => {
                field["type"] = json!("number");
                field["integer"] = json!(true);
            }
            Ty::Money => {
                field["type"] = json!("number");
                field["decimals"] = json!(2);
                field["unit"] = json!(r.currency);
            }
            Ty::Time => field["type"] = json!("datetime"),
            Ty::Duration => {
                field["type"] = json!("duration");
                field["unit"] = json!("seconds");
            }
            t => return Err(format!("this model does not read a {t:?}")),
        }
        let mut body = json!({ "state": state, "fields": { r.attr.clone(): field } });
        if !images.is_empty() {
            body["images"] = Value::Array(images);
        }
        Ok(body)
    }

    /// Reads `answers.⟨attr⟩`, or `result.answers.⟨attr⟩` as the REST API wraps it.
    pub fn read(&self, answer: &Value, r: &Reading) -> Answer {
        let a = answer.pointer(&format!("/result/answers/{}", r.attr)).or_else(|| answer.pointer(&format!("/answers/{}", r.attr)));
        let Some(a) = a else { return Answer::Failed(format!("no answers.{} in the answer", r.attr)) };
        // `p_null`: how likely the value is absent from the input.
        if a["p_null"].as_f64().is_some_and(|p| p >= 0.5) {
            return Answer::Declined;
        }
        read_answer(a.get("value"), a["confidence"].as_f64(), r, self.min_confidence)
    }
}

impl Backend for WorkersAi {
    fn decide(&mut self, _: &Choice, _: Duration) -> Answer {
        Answer::Failed("this model reads values; it does not pick among options".into())
    }

    fn extract(&mut self, r: &Reading, within: Duration) -> Answer {
        if let Some(why) = reading_unsent(r, MediaMode::Inline, self.max_media_bytes) {
            return Answer::Failed(why);
        }
        let body = match self.request(r) {
            Ok(b) => b,
            Err(e) => return Answer::Failed(e),
        };
        let key = match key_from(self.key_env.as_deref()) {
            Ok(k) => k,
            Err(e) => return Answer::Failed(e),
        };
        match self.transport.post(&self.url, key.as_deref(), &body, within) {
            Ok(v) => self.read(&v, r),
            Err(e) if e == TIMED_OUT => Answer::Late,
            Err(e) => Answer::Failed(e),
        }
    }
}

/// Backends by model name.
pub type Named = Vec<(String, Box<dyn Backend>)>;

/// The backends a `models.json` names (appendix K.2.7), each with a
/// transport from `transport`.
pub fn load(text: &str, mut transport: impl FnMut() -> Box<dyn Transport>) -> Result<Named, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("models.json: {e}"))?;
    let models = v.as_object().ok_or("models.json: expected an object of models by `name@version`")?;
    let mut out: Named = vec![];
    for (name, cfg) in models {
        let err = |m: String| format!("models.json: {name}: {m}");
        let cfg = cfg.as_object().ok_or_else(|| err("expected an object".into()))?;
        let text = |k: &str| -> Result<Option<String>, String> {
            match cfg.get(k) {
                None => Ok(None),
                Some(Value::String(s)) => Ok(Some(s.clone())),
                Some(_) => Err(err(format!("`{k}` must be text"))),
            }
        };
        let min_confidence = match cfg.get("min_confidence") {
            None => None,
            Some(x) => Some(x.as_f64().filter(|c| (0.0..=1.0).contains(c)).ok_or_else(|| err("`min_confidence` must be a number from 0 to 1".into()))?),
        };
        let media = match text("media")?.as_deref() {
            None | Some("inline") => MediaMode::Inline,
            Some("omit") => MediaMode::Omit,
            Some(other) => return Err(err(format!("`media` is \"inline\" or \"omit\", not \"{other}\""))),
        };
        let max_media_bytes = match cfg.get("max_media_bytes") {
            None => MAX_MEDIA_SENT,
            Some(x) => x
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .filter(|n| *n <= MAX_MEDIA_SENT)
                .ok_or_else(|| err(format!("`max_media_bytes` must be a whole number, at most {MAX_MEDIA_SENT}")))?,
        };
        let backend = text("backend")?.unwrap_or_else(|| "jev".into());
        let allowed: &[&str] = match backend.as_str() {
            "jev" => &["backend", "url", "model", "key_env", "min_confidence", "media", "max_media_bytes"],
            "custom" => &["backend", "url", "key_env", "pick", "value", "confidence", "min_confidence", "media", "max_media_bytes"],
            "workers-ai" => &["backend", "url", "key_env", "min_confidence", "max_media_bytes"],
            other => return Err(err(format!("unknown backend `{other}`; use \"jev\", \"custom\" or \"workers-ai\""))),
        };
        if let Some(k) = cfg.keys().find(|k| !allowed.contains(&k.as_str())) {
            return Err(err(format!("unknown field `{k}` for backend `{backend}`")));
        }
        let b: Box<dyn Backend> = if backend == "jev" {
            let mut j = Jev::new(transport());
            j.url = text("url")?.unwrap_or(j.url);
            j.model = text("model")?.unwrap_or(j.model);
            j.key_env = text("key_env")?.or(j.key_env);
            j.min_confidence = min_confidence;
            j.media = media;
            j.max_media_bytes = max_media_bytes;
            Box::new(j)
        } else if backend == "workers-ai" {
            let url = text("url")?.ok_or_else(|| err("a workers-ai backend needs a `url`, its account's …/ai/run/@cf/cloudflare/clairic".into()))?;
            let key_env = text("key_env")?.or_else(|| Some(WORKERS_AI_KEY_ENV.into()));
            Box::new(WorkersAi { url, key_env, min_confidence, max_media_bytes, transport: transport() })
        } else {
            let url = text("url")?.ok_or_else(|| err("a custom backend needs a `url`".into()))?;
            let (pick, value) = (text("pick")?, text("value")?);
            if pick.is_none() && value.is_none() {
                return Err(err("a custom backend needs `pick`, a JSON pointer such as \"/result/pick\", or `value` for a model that reads".into()));
            }
            let pick = pick.unwrap_or_default();
            Box::new(Custom { url, key_env: text("key_env")?, pick, value, confidence: text("confidence")?, min_confidence, media, max_media_bytes, transport: transport() })
        };
        out.push((name.clone(), b));
    }
    Ok(out)
}
