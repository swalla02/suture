//! A program as a service (appendix L): one host, the inputs applied one at
//! a time, a log on disk that a restart replays, the real clock, and the
//! requests posted to adapters.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use suture_lang::ir::{Program, Ty};
use suture_lang::scenario::Caller;

use crate::backend::{self, Transport};
use crate::choice::Answer;
use crate::host::Host;
use crate::state::{Ask, Val};

/// The log's format; its header says which (appendix L.2.1).
pub const LOG_FORMAT: u32 = 1;

/// The longest an adapter's post may take, whatever its effect's deadline.
const POST_TIMEOUT: Duration = Duration::from_secs(10);

/// Who sends an input: a role and a name, as the gateway says (appendix L.2.5).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Who {
    pub role: String,
    pub name: String,
}

/// One line of the log (appendix L.2.1).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Record {
    /// The clock moved on by this many seconds.
    Pass(u64),
    /// An entity of the world, which does not pass the gate.
    World { kind: String, label: String, attrs: Vec<(String, Val)>, items: Items },
    New { by: Who, verb: String, kind: String, label: String, attrs: Vec<(String, Val)>, items: Items },
    Event { by: Who, verb: String, target: String },
    Change { by: Who, target: String, attr: String, value: Val },
    /// An adapter's answer: to the request with `key`, or the oldest for `target`.
    Answer { adapter: String, outcome: String, key: Option<String>, target: Option<String>, data: Vec<(String, Val)> },
    /// A model's answer to the choice with this key, given while the input before it applied.
    Model { key: String, answer: Answer },
}

#[derive(Serialize, Deserialize)]
struct Header {
    suture: u32,
    module: String,
    version: u32,
    started: u64,
}

/// Where an adapter is reached (appendix L.2.8).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Adapter {
    pub url: String,
    pub key_env: Option<String>,
}

/// The adapters an `adapters.json` names.
pub fn adapters(text: &str) -> Result<BTreeMap<String, Adapter>, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("adapters.json: {e}"))?;
    let all = v.as_object().ok_or("adapters.json: expected an object of adapters by name")?;
    let mut out = BTreeMap::new();
    for (name, cfg) in all {
        let err = |m: &str| format!("adapters.json: {name}: {m}");
        let cfg = cfg.as_object().ok_or_else(|| err("expected an object"))?;
        if let Some(k) = cfg.keys().find(|k| !["url", "key_env"].contains(&k.as_str())) {
            return Err(err(&format!("unknown field `{k}`; use \"url\" and \"key_env\"")));
        }
        let url = cfg.get("url").and_then(Value::as_str).ok_or_else(|| err("needs a `url`"))?.to_string();
        let key_env = match cfg.get("key_env") {
            None => None,
            Some(Value::String(s)) => Some(s.clone()),
            Some(_) => return Err(err("`key_env` must be text")),
        };
        out.insert(name.clone(), Adapter { url, key_env });
    }
    Ok(out)
}

/// What an input did: the trace lines it caused, and whether it was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Applied {
    pub trace: Vec<String>,
    pub refused: Option<String>,
}

pub struct Service {
    pub host: Host,
    /// The Unix time of the clock's zero.
    pub started: u64,
    log: Option<File>,
    adapters: BTreeMap<String, Adapter>,
    transport: Box<dyn Transport>,
    /// The requests already posted by this process, by key.
    posted: BTreeSet<String>,
    /// Requests whose post failed, by key, with when to try again.
    retry: BTreeMap<String, u64>,
    /// Why the log could not be written: from then on, every input is
    /// refused, since the log no longer holds what memory does.
    broken: Option<String>,
}

/// How long a failed post waits before it is tried again, in seconds.
const RETRY_SECS: u64 = 5;

fn synced(path: &Path, text: &str) -> std::io::Result<()> {
    let mut f = File::create(path)?;
    f.write_all(text.as_bytes())?;
    f.sync_all()
}

impl Service {
    /// A service with no log: for tests, and for trying a program out.
    pub fn in_memory(prog: &Program, now: u64) -> Service {
        Service {
            host: Host::new(prog.clone()),
            started: now,
            log: None,
            adapters: BTreeMap::new(),
            transport: Box::new(backend::Curl),
            posted: BTreeSet::new(),
            retry: BTreeMap::new(),
            broken: None,
        }
    }

    /// Opens the state directory: a new one starts its log at `now`; one with a
    /// log replays it, refusing a log written under another program (appendix L.2.3).
    pub fn open(prog: &Program, state: &Path, now: u64, setup: impl FnOnce(&mut Host)) -> Result<Service, String> {
        let shown = |p: &Path| p.display().to_string();
        std::fs::create_dir_all(state).map_err(|e| format!("{}: {e}", shown(state)))?;
        let (prog_path, log_path) = (state.join("program.json"), state.join("log.jsonl"));
        let compiled = serde_json::to_value(prog).map_err(|e| e.to_string())?;
        let mut svc = Service::in_memory(prog, now);
        setup(&mut svc.host);
        if log_path.exists() {
            let old: Value = std::fs::read_to_string(&prog_path)
                .map_err(|e| format!("{}: {e}", shown(&prog_path)))
                .and_then(|t| serde_json::from_str(&t).map_err(|e| format!("{}: {e}", shown(&prog_path))))?;
            if old != compiled {
                let was = |k: &str| old.get(k).map(Value::to_string).unwrap_or_default();
                return Err(format!(
                    "{}: the log was written under {} v{}, and this program differs; a deploy with live units waits for versioning, so start with a fresh state directory",
                    shown(state),
                    was("module").trim_matches('"'),
                    was("version")
                ));
            }
            svc.replay(&log_path)?;
        } else {
            // The program first, then the log, each synced, then the directory:
            // a log is never there without the program it was written under.
            synced(&prog_path, &serde_json::to_string(&compiled).map_err(|e| e.to_string())?).map_err(|e| format!("{}: {e}", shown(&prog_path)))?;
            let header = Header { suture: LOG_FORMAT, module: prog.module.clone(), version: prog.version, started: now };
            synced(&log_path, &(serde_json::to_string(&header).map_err(|e| e.to_string())? + "\n")).map_err(|e| format!("{}: {e}", shown(&log_path)))?;
            File::open(state).and_then(|d| d.sync_all()).map_err(|e| format!("{}: {e}", shown(state)))?;
        }
        svc.log = Some(OpenOptions::new().append(true).open(&log_path).map_err(|e| format!("{}: {e}", shown(&log_path)))?);
        Ok(svc)
    }

    fn replay(&mut self, path: &Path) -> Result<(), String> {
        let at = |n: usize| format!("{}:{}", path.display(), n + 1);
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        // Each record ends with a newline. Whatever follows the last one was
        // cut short by a crash, and never answered: it is cut off the file, so
        // the next record starts on a line of its own.
        let mut lines = vec![];
        let mut start = 0;
        let mut good = 0;
        for (n, line) in bytes.split_inclusive(|b| *b == b'\n').enumerate() {
            let end = start + line.len();
            if line.ends_with(b"\n") {
                let text = std::str::from_utf8(line).map_err(|e| format!("{}: {e}", at(n)))?;
                if !text.trim().is_empty() {
                    lines.push((n, text.trim_end().to_string(), end));
                }
                good = end;
            }
            start = end;
        }
        // A last record that does not parse was cut short too (appendix L.2.2).
        if lines.len() > 1 && lines.last().is_some_and(|(_, l, _)| serde_json::from_str::<Record>(l).is_err()) {
            lines.pop();
            good = lines.last().map_or(0, |l| l.2);
        }
        if good < bytes.len() {
            let f = OpenOptions::new().write(true).open(path).map_err(|e| format!("{}: {e}", path.display()))?;
            f.set_len(good as u64).and_then(|_| f.sync_all()).map_err(|e| format!("{}: {e}", path.display()))?;
        }
        let Some(((_, first, _), rest)) = lines.split_first() else { return Err(format!("{}: the log has no header", path.display())) };
        let header: Header = serde_json::from_str(first).map_err(|e| format!("{}: not a log header: {e}", at(0)))?;
        if header.suture != LOG_FORMAT {
            return Err(format!("{}: log format {} is not {LOG_FORMAT}", at(0), header.suture));
        }
        self.started = header.started;
        let mut records = vec![];
        for (n, line, _) in rest {
            records.push(serde_json::from_str::<Record>(line).map_err(|e| format!("{}: {e}", at(*n)))?);
        }
        let mut i = 0;
        while i < records.len() {
            // The model answers an input caused come right after it.
            let answers = records[i + 1..].iter().take_while(|r| matches!(r, Record::Model { .. }));
            self.host.replay = answers.map(|r| match r {
                Record::Model { key, answer } => (key.clone(), answer.clone()),
                _ => unreachable!(),
            }).collect();
            let skip = self.host.replay.len();
            let _ = self.apply(&records[i]);
            if !self.host.replay.is_empty() {
                self.host.trace.push(format!("replay: {} model answers in the log were not asked for again", self.host.replay.len()));
            }
            self.host.replay.clear();
            i += 1 + skip;
        }
        Ok(())
    }

    /// Sets where adapters are reached, and how requests are posted.
    pub fn connect(&mut self, adapters: BTreeMap<String, Adapter>, transport: Box<dyn Transport>) {
        self.adapters = adapters;
        self.transport = transport;
    }

    /// The runtime clock at Unix time `now`.
    pub fn elapsed(&self, now: u64) -> u64 {
        now.saturating_sub(self.started)
    }

    /// Fires the timers that fell due by Unix time `now`, as one `pass` (appendix L.2.4).
    pub fn tick(&mut self, now: u64) -> Result<Vec<String>, String> {
        let elapsed = self.elapsed(now);
        // A clock that stepped back waits until it is past the runtime clock again.
        let fired = match self.host.next_due() {
            Some(due) if due <= elapsed && elapsed > self.host.now => self.input(Record::Pass(elapsed - self.host.now), now)?.trace,
            _ => vec![],
        };
        // Posts that failed are tried again.
        let from = self.host.trace.len();
        self.post_new(now)?;
        Ok([fired, self.host.trace[from..].to_vec()].concat())
    }

    /// Applies an input at Unix time `now`: the clock moves on first; then
    /// the input and the model answers it caused are logged, and only then
    /// are new requests posted (appendix L.2.2). The error is the log's.
    pub fn input(&mut self, r: Record, now: u64) -> Result<Applied, String> {
        if let Some(why) = &self.broken {
            return Err(format!("the log could not be written ({why}), so no input is taken until the service restarts"));
        }
        let from = self.host.trace.len();
        let elapsed = self.elapsed(now);
        if !matches!(r, Record::Pass(_)) && elapsed > self.host.now {
            self.applied(Record::Pass(elapsed - self.host.now))?;
        }
        let refused = self.applied(r)?;
        self.post_new(now)?;
        Ok(Applied { trace: self.host.trace[from..].to_vec(), refused })
    }

    /// Applies one record and logs it with the model answers it caused.
    fn applied(&mut self, r: Record) -> Result<Option<String>, String> {
        let asked = self.host.model_answers.len();
        let refused = self.apply(&r).err();
        let mut lines = vec![r];
        lines.extend(self.host.model_answers[asked..].iter().map(|(key, answer)| Record::Model { key: key.clone(), answer: answer.clone() }));
        self.write(&lines)?;
        Ok(refused)
    }

    fn write(&mut self, records: &[Record]) -> Result<(), String> {
        let Some(log) = self.log.as_mut() else { return Ok(()) };
        let mut text = String::new();
        for r in records {
            text.push_str(&serde_json::to_string(r).map_err(|e| e.to_string())?);
            text.push('\n');
        }
        if let Err(e) = log.write_all(text.as_bytes()).and_then(|_| log.sync_data()) {
            self.broken = Some(e.to_string());
            return Err(format!("the log could not be written ({e}), so no input is taken until the service restarts"));
        }
        Ok(())
    }

    /// Why the log could not be written, if it could not.
    pub fn broken(&self) -> Option<&str> {
        self.broken.as_deref()
    }

    fn apply(&mut self, r: &Record) -> Result<(), String> {
        let caller = |by: &Who, verb: &str| Caller { role: by.role.clone(), name: by.name.clone(), verb: verb.to_string() };
        let h = &mut self.host;
        match r {
            Record::Pass(secs) => h.pass(*secs),
            Record::World { kind, label, attrs, items } => h.create_with(kind, label, attrs, items),
            Record::New { by, verb, kind, label, attrs, items } => h.input_new(Some(&caller(by, verb)), kind, label, attrs, items),
            Record::Event { by, verb, target } => h.input_event(Some(&caller(by, verb)), target),
            Record::Change { by, target, attr, value } => h.input_change(Some(&caller(by, "changes")), target, attr, value.clone()),
            Record::Answer { adapter, outcome, key: Some(key), data, .. } => h.answer_key(adapter, outcome, data, key),
            Record::Answer { adapter, outcome, target, data, .. } => h.answer(adapter, outcome, data, target.as_deref()),
            Record::Model { .. } => Ok(()),
        }
    }

    /// Posts each waiting request this process has not posted yet to its
    /// adapter, if `adapters.json` names it. A reply with an outcome is the
    /// answer; anything else leaves the request waiting (appendix L.2.8).
    pub fn post_new(&mut self, now: u64) -> Result<(), String> {
        loop {
            let due = |k: &String| self.retry.get(k).is_none_or(|at| *at <= now);
            let next = self.host.pending().into_iter().find(|a| self.adapters.contains_key(&a.adapter) && !self.posted.contains(&a.key) && due(&a.key)).cloned();
            let Some(ask) = next else { return Ok(()) };
            self.posted.insert(ask.key.clone());
            self.retry.remove(&ask.key);
            let failed = |s: &mut Service, why: String| {
                s.host.trace.push(format!("      {}: {why}; trying again in {RETRY_SECS} seconds   ({})", ask.adapter, ask.key));
                s.posted.remove(&ask.key);
                s.retry.insert(ask.key.clone(), now + RETRY_SECS);
            };
            let adapter = self.adapters[&ask.adapter].clone();
            let key = match adapter.key_env.as_deref().map(|e| std::env::var(e).map_err(|_| format!("{e} is not set"))) {
                Some(Err(e)) => {
                    failed(self, format!("not posted: {e}"));
                    continue;
                }
                Some(Ok(k)) => Some(k),
                None => None,
            };
            let body = request_json(&self.host.prog, &ask);
            let timeout = Duration::from_secs(self.host.prog.request_within(&ask.kind).max(1)).min(POST_TIMEOUT);
            match self.transport.post(&adapter.url, key.as_deref(), &body, timeout) {
                Ok(reply) => match reply.get("outcome").and_then(Value::as_str) {
                    Some(outcome) => {
                        let data = reply.get("data").map_or(Ok(vec![]), |d| answer_data(&self.host.prog, &ask, d));
                        match data {
                            Ok(data) => {
                                let r = Record::Answer { adapter: ask.adapter.clone(), outcome: outcome.to_string(), key: Some(ask.key.clone()), target: None, data };
                                self.input(r, now)?;
                            }
                            Err(e) => self.host.trace.push(format!("      {}: an answer it cannot read: {e}   ({})", ask.adapter, ask.key)),
                        }
                    }
                    None => self.host.trace.push(format!("      {}: posted; it answers later   ({})", ask.adapter, ask.key)),
                },
                Err(e) => failed(self, format!("could not be posted: {e}")),
            }
        }
    }

    /// The state directory's default place, beside the program.
    pub fn default_state(dir: &Path) -> PathBuf {
        dir.join(".suture")
    }
}

/// A request as posted to its adapter, and as `GET /pending` shows it.
pub fn request_json(prog: &Program, a: &Ask) -> Value {
    let sends: serde_json::Map<String, Value> = a.sends.iter().map(|(k, v)| (k.clone(), to_json(v, &prog.currency))).collect();
    json!({ "key": a.key, "adapter": a.adapter, "verb": a.verb, "kind": a.kind, "unit": a.label, "sends": sends })
}

/// A value as the API shows it (appendix L.2.7).
pub fn to_json(v: &Val, currency: &str) -> Value {
    match v {
        Val::Duration(s) | Val::Time(s) => json!(s),
        _ => backend::fact(v, currency),
    }
}

/// A value from the API, by the attribute's type (appendix L.2.7). Without a
/// type, it is read from its JSON form.
pub fn from_json(v: &Value, ty: Option<Ty>, currency: &str) -> Result<Val, String> {
    let money = |s: &str| money(s, currency);
    match (ty, v) {
        (Some(Ty::Text), Value::String(s)) => Ok(Val::Text(s.clone())),
        (Some(Ty::Bool) | None, Value::Bool(b)) => Ok(Val::Bool(*b)),
        (Some(Ty::Count) | None, Value::Number(n)) => n.as_i64().map(Val::Count).ok_or_else(|| format!("{n} is not a whole number")),
        (Some(Ty::Duration), Value::Number(n)) => n.as_i64().map(Val::Duration).ok_or_else(|| format!("{n} is not a whole number of seconds")),
        (Some(Ty::Time), Value::Number(n)) => n.as_i64().map(Val::Time).ok_or_else(|| format!("{n} is not a whole number of seconds")),
        (Some(Ty::Money), Value::String(s)) => money(s),
        // A reference is given by name; the host checks the name (appendix F.2.2).
        (Some(Ty::Ref), Value::String(s)) => Ok(Val::Text(s.clone())),
        (Some(Ty::Set) | None, Value::Array(xs)) => {
            let mut names = xs.iter().map(|x| x.as_str().map(String::from).ok_or("a set lists names")).collect::<Result<Vec<_>, _>>()?;
            names.sort();
            names.dedup();
            Ok(Val::Set(names))
        }
        (None, Value::String(s)) => Ok(money(s).unwrap_or_else(|_| Val::Text(s.clone()))),
        (Some(Ty::Money), _) => Err(format!("an amount is text such as \"80 {currency}\", not {v}")),
        (Some(t), _) => Err(format!("{v} is not {}", ty_name(t))),
        (None, _) => Err(format!("{v} is not a value")),
    }
}

fn ty_name(t: Ty) -> &'static str {
    match t {
        Ty::Text => "text",
        Ty::Bool => "yes or no (true or false)",
        Ty::Money => "an amount",
        Ty::Count => "a whole number",
        Ty::Ref => "a name",
        Ty::Set => "a list of names",
        Ty::Time => "a time in seconds",
        Ty::Duration => "a duration in seconds",
    }
}

/// `"80 EUR"` or `"12.50 EUR"`, in cents of the module's currency.
fn money(s: &str, currency: &str) -> Result<Val, String> {
    let bad = || format!("\"{s}\" is not an amount such as \"80 {currency}\"");
    let (amount, cur) = s.trim().split_once(' ').ok_or_else(bad)?;
    if cur != currency {
        return Err(format!("\"{s}\" is not in this module's currency, {currency}"));
    }
    let (whole, cents) = amount.split_once('.').unwrap_or((amount, "0"));
    let digits = |x: &str| !x.is_empty() && x.chars().all(|c| c.is_ascii_digit());
    if !digits(whole) || !digits(cents) || cents.len() > 2 {
        return Err(bad());
    }
    let cents: i64 = format!("{cents:0<2}").parse().map_err(|_| bad())?;
    whole.parse::<i64>().ok().and_then(|w| w.checked_mul(100)).and_then(|w| w.checked_add(cents)).map(Val::Money).ok_or_else(bad)
}

/// Attributes of `kind` from a JSON object.
pub fn attrs_json(prog: &Program, kind: &str, v: Option<&Value>) -> Result<Vec<(String, Val)>, String> {
    let Some(v) = v else { return Ok(vec![]) };
    let obj = v.as_object().ok_or("attributes are an object")?;
    obj.iter()
        .map(|(k, x)| {
            let ty = prog.kind(kind).and_then(|kd| kd.attr(k)).map(|a| a.ty);
            from_json(x, ty, &prog.currency).map(|val| (k.clone(), val)).map_err(|e| format!("{k}: {e}"))
        })
        .collect()
}

/// List elements, by list name.
pub type Items = Vec<(String, Vec<(String, Val)>)>;

/// List elements from a JSON object of lists by name.
pub fn items_json(prog: &Program, kind: &str, v: Option<&Value>) -> Result<Items, String> {
    let Some(v) = v else { return Ok(vec![]) };
    let obj = v.as_object().ok_or("items are an object of lists by name")?;
    let mut out = vec![];
    for (list, elements) in obj {
        let l = prog.lists.iter().find(|l| l.owner == kind && l.name == *list).ok_or_else(|| format!("{kind} has no list {list}"))?;
        for e in elements.as_array().ok_or_else(|| format!("{list} is a list"))? {
            out.push((list.clone(), attrs_json(prog, &l.element, Some(e))?));
        }
    }
    Ok(out)
}

/// An adapter's answer data, typed by the attributes of the requesting kind.
pub fn answer_data(prog: &Program, ask: &Ask, v: &Value) -> Result<Vec<(String, Val)>, String> {
    let kind = ask.kind.split(['~', '@']).next().unwrap_or(&ask.kind);
    attrs_json(prog, kind, Some(v))
}
