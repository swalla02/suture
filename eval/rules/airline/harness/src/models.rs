//! OpenRouter, and the record of every answer. The key comes from
//! `OPENROUTER_API_KEY` or `~/.config/chess-ladder/openrouter.key`, and goes
//! to curl on stdin, never in its arguments or the repo. An answer is kept
//! in `RULES_RUNS` (default `~/rules-runs/airline`) the first time it comes,
//! so a rerun asks nothing again. Tests never come here.

use serde_json::{Value, json};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

pub const LLM: &str = "openai/gpt-oss-120b";
pub const EXTRACTOR: &str = "typesafe/jev-1.14";

pub fn runs_dir() -> PathBuf {
    std::env::var_os("RULES_RUNS").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("rules-runs/airline"))
}

fn key() -> Result<String, String> {
    if let Ok(k) = std::env::var("OPENROUTER_API_KEY") {
        return Ok(k);
    }
    let p = PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config/chess-ladder/openrouter.key");
    Ok(std::fs::read_to_string(&p).map_err(|e| format!("no OPENROUTER_API_KEY and {}: {e}", p.display()))?.trim().to_string())
}

static BODIES: AtomicU64 = AtomicU64::new(0);

/// POSTs `body` to `url`, retrying a busy gateway; the parsed answer.
pub fn post(url: &str, body: &Value) -> Result<Value, String> {
    let dir = runs_dir().join("tmp");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let file = dir.join(format!("body-{}-{}.json", std::process::id(), BODIES.fetch_add(1, Ordering::SeqCst)));
    std::fs::write(&file, body.to_string()).map_err(|e| e.to_string())?;
    let key = key()?;
    let mut tries = 0;
    let out = loop {
        let v = curl(url, &key, &file);
        let busy = match &v {
            Ok(v) => matches!(v["error"]["code"].as_i64(), Some(408 | 429 | 500 | 502 | 503 | 504)),
            Err(_) => true,
        };
        if busy && tries < 5 {
            tries += 1;
            std::thread::sleep(std::time::Duration::from_secs(5 * tries));
            continue;
        }
        break v;
    };
    let _ = std::fs::remove_file(&file);
    let v = out?;
    if v.get("error").is_some() {
        return Err(format!("error: {}", v["error"]));
    }
    Ok(v)
}

fn curl(url: &str, key: &str, file: &Path) -> Result<Value, String> {
    let mut c = Command::new("curl")
        .args(["-s", "-m", "300", "--config", "-", "--data-binary"])
        .arg(format!("@{}", file.display()))
        .arg(url)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    c.stdin.take().ok_or("no stdin")?.write_all(format!("header = \"authorization: Bearer {key}\"\nheader = \"content-type: application/json\"\n").as_bytes()).map_err(|e| e.to_string())?;
    let out = c.wait_with_output().map_err(|e| e.to_string())?;
    serde_json::from_slice(&out.stdout).map_err(|e| format!("{e}: {}", String::from_utf8_lossy(&out.stdout).chars().take(300).collect::<String>()))
}

/// The recorded answer at `path`, or a new one from `ask`, recorded with
/// how long it took. A failure is not recorded, so the next run asks again.
pub fn recorded(path: &Path, ask: impl FnOnce() -> Result<Value, String>) -> Result<Value, String> {
    if let Ok(text) = std::fs::read_to_string(path) {
        return serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()));
    }
    let start = std::time::Instant::now();
    let mut v = ask()?;
    v["seconds"] = json!(start.elapsed().as_secs_f64());
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, serde_json::to_string_pretty(&v).unwrap()).map_err(|e| e.to_string())?;
    Ok(v)
}

/// A chat completion from the prompt LLM, with its text, reasoning effort
/// low and temperature 0 as RuleArena asked its models.
pub fn chat(system: &str, user: &str) -> Result<Value, String> {
    chat_at(system, user, 0.0)
}

/// The same at another temperature: a variant written again after one
/// that failed its check.
pub fn chat_at(system: &str, user: &str, temperature: f64) -> Result<Value, String> {
    let body = json!({
        "model": LLM,
        "temperature": temperature,
        "max_tokens": 12000,
        "reasoning": {"effort": "low"},
        "usage": {"include": true},
        "messages": [{"role": "system", "content": system}, {"role": "user", "content": user}],
    });
    let v = post("https://openrouter.ai/api/v1/chat/completions", &body)?;
    let text = v["choices"][0]["message"]["content"].as_str().ok_or(format!("no answer: {}", v.to_string().chars().take(300).collect::<String>()))?;
    Ok(json!({"text": text, "cost": v["usage"]["cost"], "usage": v["usage"], "provider": v["provider"], "model": v["model"]}))
}

/// The Decisions API: `questions` about `state`, each answered by the
/// option given the highest probability.
pub fn decide(state: &Value, questions: &Value) -> Result<Value, String> {
    let v = post("https://openrouter.ai/api/alpha/decisions", &json!({"model": EXTRACTOR, "state": state, "questions": questions}))?;
    let mut picks = serde_json::Map::new();
    for (q, a) in v["answers"].as_object().ok_or(format!("no answers: {v}"))? {
        let probs = a["probabilities"].as_object().ok_or(format!("no probabilities for {q}"))?;
        let best = probs.iter().max_by(|a, b| a.1.as_f64().unwrap_or(0.0).total_cmp(&b.1.as_f64().unwrap_or(0.0)));
        picks.insert(q.clone(), json!(best.map(|b| b.0.clone())));
    }
    Ok(json!({"picks": picks, "cost": v["usage"]["cost"], "usage": v["usage"], "model": v["model"]}))
}
