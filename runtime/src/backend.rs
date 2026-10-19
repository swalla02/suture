//! Model backends (appendix K.2.6–K.2.7): the Jev decision API by default, a
//! custom JSON format for other APIs, and the transport they post through.
//! The transport is a trait, so tests never reach the network.

use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Duration;

use base64::Engine;
use serde_json::{json, Map, Value};

use crate::choice::{Answer, Backend, Choice};
use crate::state::Val;

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
            .args(["-sS", "-K", "-", "-m", &format!("{:.3}", timeout.max(Duration::from_millis(1)).as_secs_f64()), "-w", "\n%{http_code}"])
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
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
        let text = String::from_utf8_lossy(&out.stdout);
        let (body, status) = text.rsplit_once('\n').unwrap_or(("", &text));
        match status.trim().parse::<u16>() {
            Ok(s) if (200..300).contains(&s) => serde_json::from_str(body).map_err(|e| format!("not JSON: {e}")),
            Ok(s) => Err(format!("HTTP {s}: {}", body.chars().take(200).collect::<String>())),
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
    let mut out = fact(v, &c.currency);
    if let (Val::Media { sha256, .. }, MediaMode::Inline) = (v, mode)
        && let Some(b) = c.media.get(sha256)
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

fn key_from(env: Option<&str>) -> Result<Option<String>, String> {
    match env {
        None => Ok(None),
        Some(e) => std::env::var(e).map(Some).map_err(|_| format!("{e} is not set")),
    }
}

fn confident(pick: String, confidence: Option<f64>, min: Option<f64>) -> Answer {
    match (confidence, min) {
        (Some(c), Some(m)) if c < m => Answer::Unsure(c),
        _ => Answer::Picked(pick),
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
        let instructions = format!(
            "Choose the {} for {} ({}). The options, best first by the program's own preference: {}.",
            c.point,
            c.unit,
            c.chooser,
            labels.join(", ")
        );
        json!({
            "model": self.model,
            "state": state(c, self.media),
            "questions": { "pick": { "type": "choice", "instructions": instructions, "criteria": criteria } },
        })
    }

    /// Reads `answers.pick.choice` and `answers.pick.confidence`.
    pub fn read(&self, answer: &Value) -> Answer {
        let q = &answer["answers"]["pick"];
        let Some(pick) = q["choice"].as_str() else { return Answer::Failed("no answers.pick.choice in the answer".into()) };
        confident(pick.to_string(), q["confidence"].as_f64(), self.min_confidence)
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
            Ok(v) => self.read(&v),
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
    /// `/result/pick`.
    pub pick: String,
    pub confidence: Option<String>,
    pub min_confidence: Option<f64>,
    pub media: MediaMode,
    pub max_media_bytes: usize,
    pub transport: Box<dyn Transport>,
}

impl Custom {
    pub fn request(&self, c: &Choice) -> Value {
        let options: Vec<Value> = c.options.iter().map(|o| json!({ "label": o.label, "facts": facts(&o.attrs, c, self.media) })).collect();
        json!({ "point": c.point, "state": state(c, self.media), "options": options })
    }

    pub fn read(&self, answer: &Value) -> Answer {
        let Some(pick) = answer.pointer(&self.pick).and_then(Value::as_str) else {
            return Answer::Failed(format!("no text at {} in the answer", self.pick));
        };
        let confidence = self.confidence.as_deref().and_then(|p| answer.pointer(p)).and_then(Value::as_f64);
        confident(pick.to_string(), confidence, self.min_confidence)
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
            Ok(v) => self.read(&v),
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
            "custom" => &["backend", "url", "key_env", "pick", "confidence", "min_confidence", "media", "max_media_bytes"],
            other => return Err(err(format!("unknown backend `{other}`; use \"jev\" or \"custom\""))),
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
        } else {
            let url = text("url")?.ok_or_else(|| err("a custom backend needs a `url`".into()))?;
            let pick = text("pick")?.ok_or_else(|| err("a custom backend needs `pick`, a JSON pointer such as \"/result/pick\"".into()))?;
            Box::new(Custom { url, key_env: text("key_env")?, pick, confidence: text("confidence")?, min_confidence, media, max_media_bytes, transport: transport() })
        };
        out.push((name.clone(), b));
    }
    Ok(out)
}
