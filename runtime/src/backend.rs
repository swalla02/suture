//! Model backends (appendix K.2.6–K.2.7): the Jev decision API by default, a
//! custom JSON format for other APIs, and the transport they post through.
//! The transport is a trait, so tests never reach the network.

use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::{json, Map, Value};

use crate::choice::{Answer, Backend, Choice};
use crate::state::Val;

/// Jev's decision endpoint (docs.typesafe.ai/api).
pub const JEV_URL: &str = "https://api.typesafe.ai/v1/systemone";
pub const JEV_MODEL: &str = "jev-latest";
pub const JEV_KEY_ENV: &str = "JEV_API_KEY";

/// Posts a JSON body and returns the JSON answer, within `timeout`.
pub trait Transport {
    /// `key`: the bearer token, if any. An HTTP error status is an error.
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
            .args(["-sS", "-K", "-", "-m", &format!("{:.3}", timeout.as_secs_f64()), "-w", "\n%{http_code}"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("cannot run curl: {e}"))?;
        child.stdin.take().expect("piped").write_all(config.as_bytes()).map_err(|e| e.to_string())?;
        let out = child.wait_with_output().map_err(|e| e.to_string())?;
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

/// A fact as JSON: text and numbers as such, amounts and times as shown.
pub fn fact(v: &Val, currency: &str) -> Value {
    match v {
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

fn facts(xs: &[(String, Val)], currency: &str) -> Value {
    Value::Object(xs.iter().map(|(k, v)| (k.clone(), fact(v, currency))).collect())
}

/// The state both formats send: who chooses, with their facts.
fn state(c: &Choice) -> Value {
    json!({ "unit": c.unit, "chooser": c.chooser, "facts": facts(&c.facts, &c.currency) })
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
    pub transport: Box<dyn Transport>,
}

impl Jev {
    pub fn new(transport: Box<dyn Transport>) -> Jev {
        Jev { url: JEV_URL.into(), model: JEV_MODEL.into(), key_env: Some(JEV_KEY_ENV.into()), min_confidence: None, transport }
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
                let desc = if o.attrs.is_empty() {
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
            "state": state(c),
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
        let key = match key_from(self.key_env.as_deref()) {
            Ok(k) => k,
            Err(e) => return Answer::Failed(e),
        };
        let body = self.request(c);
        match self.transport.post(&self.url, key.as_deref(), &body, within) {
            Ok(v) => self.read(&v),
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
    pub transport: Box<dyn Transport>,
}

impl Custom {
    pub fn request(&self, c: &Choice) -> Value {
        let options: Vec<Value> = c.options.iter().map(|o| json!({ "label": o.label, "facts": facts(&o.attrs, &c.currency) })).collect();
        json!({ "point": c.point, "state": state(c), "options": options })
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
        let key = match key_from(self.key_env.as_deref()) {
            Ok(k) => k,
            Err(e) => return Answer::Failed(e),
        };
        let body = self.request(c);
        match self.transport.post(&self.url, key.as_deref(), &body, within) {
            Ok(v) => self.read(&v),
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
        let backend = text("backend")?.unwrap_or_else(|| "jev".into());
        let allowed: &[&str] = match backend.as_str() {
            "jev" => &["backend", "url", "model", "key_env", "min_confidence"],
            "custom" => &["backend", "url", "key_env", "pick", "confidence", "min_confidence"],
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
            Box::new(j)
        } else {
            let url = text("url")?.ok_or_else(|| err("a custom backend needs a `url`".into()))?;
            let pick = text("pick")?.ok_or_else(|| err("a custom backend needs `pick`, a JSON pointer such as \"/result/pick\"".into()))?;
            Box::new(Custom { url, key_env: text("key_env")?, pick, confidence: text("confidence")?, min_confidence, transport: transport() })
        };
        out.push((name.clone(), b));
    }
    Ok(out)
}
