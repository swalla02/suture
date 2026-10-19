//! A program as a service (appendix L): one host, the inputs applied one at
//! a time, a log on disk that a restart replays, the real clock, and the
//! requests posted to adapters. Several pods may share one state directory
//! (appendix O): they append to the log in turn, under a lock.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
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
    Event {
        by: Who,
        verb: String,
        target: String,
        /// The kind of the part it goes to (appendix R.2.2).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        part: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        data: Vec<(String, Val)>,
    },
    Change { by: Who, target: String, attr: String, value: Val },
    /// An adapter's answer: to the request with `key`, or the oldest for `target`.
    Answer { adapter: String, outcome: String, key: Option<String>, target: Option<String>, data: Vec<(String, Val)> },
    /// A model's answer to the choice with this key, given while the input before it applied.
    Model { key: String, answer: Answer },
    /// The program was deployed in this version, kept as `program-v⟨n⟩.json` (appendix M.2.6).
    Deploy { version: u32 },
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
    /// The state directory, where each version's program is kept.
    state: Option<PathBuf>,
    /// The programs deployed by this process, by version.
    programs: BTreeMap<u32, Program>,
    /// The trace of the deploy `open` made, if it made one.
    pub deployed: Vec<String>,
    /// How many secrets `secrets.json` holds.
    kept_secrets: usize,
    /// How far into the log this process has read and applied, in bytes.
    read_to: u64,
    /// The lock on the log, and how deep this process holds it.
    lock: Option<File>,
    held: u32,
    /// This pod's name (appendix O.2.4).
    pub pod: String,
}

/// Why a deploy did not happen.
enum Refusal {
    Breaks(Vec<String>),
    Other(String),
}

/// What `suture deploy` finds (appendix M.2.7).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Check {
    /// Nothing to deploy: the state already runs this program.
    Same,
    /// The deploy would go ahead, with these trace lines.
    Deploys(Vec<String>),
    /// The deploy would be refused, for these breaks.
    Refused(Vec<String>),
}

/// How long a pod stays live after its last beat, in seconds (appendix O.2.4).
pub const BEAT_SECS: u64 = 10;

/// The units' owner among the live pods, by the FNV-1a hash of the label
/// (appendix O.2.5).
pub fn owner<'a>(label: &str, pods: &'a [String]) -> Option<&'a String> {
    let hash = label.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3));
    pods.get(usize::try_from(hash % pods.len().max(1) as u64).ok()?)
}

/// The log's lines, each with its number from `first` and where it ends;
/// and where the last whole line ends. Whatever follows was cut short by
/// a crash, as is a last line that does not parse (appendix L.2.2).
fn lines(bytes: &[u8], first: usize, header: bool) -> Result<(Vec<(usize, String)>, usize), String> {
    let mut out = vec![];
    let (mut start, mut good) = (0, 0);
    for (n, line) in bytes.split_inclusive(|b| *b == b'\n').enumerate() {
        let end = start + line.len();
        if line.ends_with(b"\n") {
            let text = std::str::from_utf8(line).map_err(|e| format!("line {}: {e}", first + n))?;
            if !text.trim().is_empty() {
                out.push((first + n, text.trim_end().to_string(), end));
            }
            good = end;
        }
        start = end;
    }
    if out.len() > usize::from(header) && out.last().is_some_and(|(_, l, _)| serde_json::from_str::<Record>(l).is_err()) {
        out.pop();
        good = out.last().map_or(0, |l| l.2);
    }
    Ok((out.into_iter().map(|(n, l, _)| (n, l)).collect(), good))
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
            state: None,
            programs: BTreeMap::new(),
            deployed: vec![],
            kept_secrets: 0,
            read_to: 0,
            lock: None,
            held: 0,
            pod: "solo".into(),
        }
    }

    /// Writes the secret store to `secrets.json`, readable only by this
    /// user, synced, and replaced in one step (appendix N.2.6).
    fn keep_secrets(&mut self) -> Result<(), String> {
        let Some(state) = &self.state else { return Ok(()) };
        let (path, tmp) = (state.join("secrets.json"), state.join("secrets.json.tmp"));
        let text = serde_json::to_string(&self.host.secrets).map_err(|e| e.to_string())?;
        let write = || -> std::io::Result<()> {
            let mut o = OpenOptions::new();
            o.write(true).create(true).truncate(true);
            #[cfg(unix)]
            std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
            let mut f = o.open(&tmp)?;
            f.write_all(text.as_bytes())?;
            f.sync_all()?;
            std::fs::rename(&tmp, &path)?;
            File::open(state)?.sync_all()
        };
        write().map_err(|e| format!("{}: {e}", path.display()))?;
        self.kept_secrets = self.host.secrets.len();
        Ok(())
    }

    /// The secret store a state directory keeps, if any.
    fn read_secrets(state: &Path) -> Result<BTreeMap<u64, String>, String> {
        let path = state.join("secrets.json");
        match std::fs::read_to_string(&path) {
            Ok(t) => serde_json::from_str(&t).map_err(|e| format!("{}: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }

    /// The file a version's program is kept in: the first version's is
    /// `program.json`, each deployed one's `program-v⟨n⟩.json`.
    fn program_file(state: &Path, version: u32, first: bool) -> PathBuf {
        if first { state.join("program.json") } else { state.join(format!("program-v{version}.json")) }
    }

    /// Replays an existing state directory into memory, under the programs
    /// it ran (appendix M.2.6); `cut` cuts off a last line a crash left short.
    fn load(state: &Path, now: u64, setup: impl FnOnce(&mut Host), cut: bool) -> Result<Service, String> {
        let shown = |p: &Path| p.display().to_string();
        let first = Self::program_file(state, 0, true);
        let prog: Program = std::fs::read_to_string(&first)
            .map_err(|e| format!("{}: {e}", shown(&first)))
            .and_then(|t| serde_json::from_str(&t).map_err(|e| format!("{}: {e}", shown(&first))))?;
        let mut svc = Service::in_memory(&prog, now);
        svc.state = Some(state.to_path_buf());
        setup(&mut svc.host);
        svc.host.secrets = Self::read_secrets(state)?;
        svc.kept_secrets = svc.host.secrets.len();
        svc.replay(&state.join("log.jsonl"), cut)?;
        Ok(svc)
    }

    /// The lock file of a state directory.
    fn lock_file(state: &Path) -> Result<File, String> {
        let path = state.join("log.lock");
        OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&path).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Runs `f` holding the log's lock, after reading what other pods
    /// appended (appendix O.2.1). It may be held again inside `f`.
    fn locked<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T, String>) -> Result<T, String> {
        // A pod whose write failed holds an input the log does not: it must
        // not build on the log any more, only restart and replay it.
        if let Some(why) = &self.broken {
            return Err(format!("the log could not be written ({why}), so this pod takes nothing until it restarts"));
        }
        if self.held == 0
            && let Some(l) = &self.lock {
            l.lock().map_err(|e| format!("the log's lock: {e}"))?;
        }
        self.held += 1;
        let r = if self.held == 1 { self.catch_up(true) } else { Ok(()) }.and_then(|_| f(self));
        self.held -= 1;
        if self.held == 0
            && let Some(l) = &self.lock {
            let _ = l.unlock();
        }
        r
    }

    /// Reads what other pods appended to the log, so a read sees every
    /// input taken before it (appendix O.2.2).
    pub fn refresh(&mut self) -> Result<(), String> {
        if let Some(why) = &self.broken {
            return Err(format!("the log could not be written ({why}), so this pod serves nothing until it restarts"));
        }
        if self.held > 0 {
            return Ok(());
        }
        let Some(l) = &self.lock else { return Ok(()) };
        l.lock_shared().map_err(|e| format!("the log's lock: {e}"))?;
        let r = self.catch_up(false);
        if let Some(l) = &self.lock {
            let _ = l.unlock();
        }
        r
    }

    /// Applies the lines of the log past `read_to`. Holding the lock
    /// alone, `cut` cuts off a line a crashed pod left short.
    fn catch_up(&mut self, cut: bool) -> Result<(), String> {
        let (Some(state), Some(_)) = (self.state.clone(), &self.log) else { return Ok(()) };
        let path = state.join("log.jsonl");
        let shown = |e: std::io::Error| format!("{}: {e}", path.display());
        let mut f = File::open(&path).map_err(shown)?;
        if f.metadata().map_err(shown)?.len() <= self.read_to {
            return Ok(());
        }
        let mut bytes = vec![];
        f.seek(SeekFrom::Start(self.read_to)).and_then(|_| f.read_to_end(&mut bytes)).map_err(shown)?;
        let (lines, good) = lines(&bytes, 0, false).map_err(|e| format!("{} after byte {}: {e}", path.display(), self.read_to))?;
        if cut && good < bytes.len() {
            let w = OpenOptions::new().write(true).open(&path).map_err(shown)?;
            w.set_len(self.read_to + good as u64).and_then(|_| w.sync_all()).map_err(shown)?;
        }
        let mut records = vec![];
        for (_, line) in &lines {
            records.push(serde_json::from_str::<Record>(line).map_err(|e| format!("{} after byte {}: {e}", path.display(), self.read_to))?);
        }
        // The secrets these lines refer to were written before them (appendix O.2.3).
        self.host.secrets = Self::read_secrets(&state)?;
        self.kept_secrets = self.host.secrets.len();
        self.apply_all(&records, &path)?;
        self.read_to += good as u64;
        Ok(())
    }

    /// Joins the pods on this state directory as `name` (appendix O.2.4).
    pub fn join(&mut self, name: &str, now: u64) -> Result<(), String> {
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
            return Err(format!("pod name `{name}`: use lower-case letters, digits and `-`"));
        }
        self.pod = name.to_string();
        self.beat(now)
    }

    /// Writes this pod's beat: the Unix time, in `pods/⟨name⟩`.
    pub fn beat(&mut self, now: u64) -> Result<(), String> {
        let Some(state) = &self.state else { return Ok(()) };
        let dir = state.join("pods");
        let (path, tmp) = (dir.join(&self.pod), dir.join(format!(".{}.tmp", self.pod)));
        std::fs::create_dir_all(&dir)
            .and_then(|_| std::fs::write(&tmp, now.to_string()))
            .and_then(|_| std::fs::rename(&tmp, &path))
            .map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Leaves the pods: its units go to the others at once.
    pub fn leave(&mut self) {
        if let Some(state) = &self.state {
            let _ = std::fs::remove_file(state.join("pods").join(&self.pod));
        }
    }

    /// The live pods at Unix time `now`, sorted by name; this one among them.
    pub fn pods(&self, now: u64) -> Vec<String> {
        let mut out = vec![self.pod.clone()];
        if let Some(Ok(dir)) = self.state.as_ref().map(|s| std::fs::read_dir(s.join("pods"))) {
            for e in dir.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                let beat = std::fs::read_to_string(e.path()).ok().and_then(|t| t.trim().parse::<u64>().ok());
                if !name.starts_with('.') && beat.is_some_and(|b| b + BEAT_SECS > now) {
                    out.push(name);
                }
            }
        }
        out.sort();
        out.dedup();
        out
    }

    /// How `prog` compares with what the state runs: `Ok(true)` if it is the
    /// same, `Ok(false)` if it is a later version, an error otherwise.
    fn newer(&self, prog: &Program) -> Result<bool, String> {
        let running = &self.host.prog;
        let same = serde_json::to_value(&**running).map_err(|e| e.to_string())? == serde_json::to_value(prog).map_err(|e| e.to_string())?;
        if same {
            return Ok(true);
        }
        if prog.module != running.module {
            return Err(format!("the state runs module {}, not {}", running.module, prog.module));
        }
        if prog.version == running.version {
            return Err(format!("the state runs {} v{}, and this program differs under the same version; give it a new version to deploy it", running.module, running.version));
        }
        if prog.version < running.version {
            return Err(format!("the state runs {} v{}; going back to v{} is not supported", running.module, running.version, prog.version));
        }
        Ok(false)
    }

    /// The deploy check of `prog` against the live state in `state`, which
    /// is read and never written (appendix M.2.7).
    pub fn check(prog: &Program, state: &Path, now: u64) -> Result<Check, String> {
        let lock = Self::lock_file(state)?;
        lock.lock_shared().map_err(|e| format!("{}: {e}", state.join("log.lock").display()))?;
        let loaded = Self::load(state, now, |_| {}, false);
        let _ = lock.unlock();
        let mut svc = loaded?;
        if svc.newer(prog)? {
            return Ok(Check::Same);
        }
        let from = svc.host.trace.len();
        match svc.deploy(prog, now) {
            Ok(a) => match a.refused {
                None => Ok(Check::Deploys(svc.host.trace[from..].to_vec())),
                Some(e) => Ok(Check::Refused(vec![format!("the deploy would go ahead, but then: {e}")])),
            },
            Err(Refusal::Breaks(b)) => Ok(Check::Refused(b)),
            Err(Refusal::Other(e)) => Err(e),
        }
    }

    /// Deploys `prog` over the running program at Unix time `now`
    /// (appendix M.2.6): the clock moves on first, and the check runs at
    /// that time. Then its file is kept, the deploy applies, and it is logged
    /// only if the new program took over. `refused` in the result is an
    /// error after it took over, such as a step that faulted.
    fn deploy(&mut self, prog: &Program, now: u64) -> Result<Applied, Refusal> {
        if let Some(why) = &self.broken {
            return Err(Refusal::Other(format!("the log could not be written ({why})")));
        }
        // A deploy refused as things stand writes nothing.
        let breaks = self.host.deploy_check(prog).map_err(Refusal::Other)?;
        if !breaks.is_empty() {
            return Err(Refusal::Breaks(breaks));
        }
        let from = self.host.trace.len();
        let elapsed = self.elapsed(now);
        if elapsed > self.host.now {
            self.applied(Record::Pass(elapsed - self.host.now)).map_err(Refusal::Other)?;
        }
        let breaks = self.host.deploy_check(prog).map_err(Refusal::Other)?;
        if !breaks.is_empty() {
            return Err(Refusal::Breaks(breaks));
        }
        if let (Some(state), Some(_)) = (&self.state, &self.log) {
            let path = Self::program_file(state, prog.version, false);
            let shown = |p: &Path, e: std::io::Error| Refusal::Other(format!("{}: {e}", p.display()));
            synced(&path, &serde_json::to_string(prog).map_err(|e| Refusal::Other(e.to_string()))?).map_err(|e| shown(&path, e))?;
            File::open(state).and_then(|d| d.sync_all()).map_err(|e| shown(state, e))?;
        }
        self.programs.insert(prog.version, prog.clone());
        let asked = self.host.model_answers.len();
        let r = Record::Deploy { version: prog.version };
        let refused = self.apply(&r).err();
        if self.host.prog.version != prog.version {
            return Err(Refusal::Other(refused.unwrap_or_default()));
        }
        let mut lines = vec![r];
        lines.extend(self.host.model_answers[asked..].iter().map(|(key, answer)| Record::Model { key: key.clone(), answer: answer.clone() }));
        self.write(&lines).map_err(Refusal::Other)?;
        if let Some(e) = &refused {
            self.host.trace.push(format!("deploy: {e}"));
        }
        self.post_new(now).map_err(Refusal::Other)?;
        Ok(Applied { trace: self.host.trace[from..].to_vec(), refused })
    }

    /// Opens the state directory: a new one starts its log at `now`; one with a
    /// log replays it, refusing a log written under another program (appendix L.2.3).
    pub fn open(prog: &Program, state: &Path, now: u64, setup: impl FnOnce(&mut Host)) -> Result<Service, String> {
        std::fs::create_dir_all(state).map_err(|e| format!("{}: {e}", state.display()))?;
        // Pods open the state directory one at a time (appendix O.2.8).
        let lock = Self::lock_file(state)?;
        lock.lock().map_err(|e| format!("{}: {e}", state.join("log.lock").display()))?;
        let opened = Self::opened(prog, state, now, setup);
        let _ = lock.unlock();
        let mut svc = opened?;
        svc.lock = Some(lock);
        Ok(svc)
    }

    fn opened(prog: &Program, state: &Path, now: u64, setup: impl FnOnce(&mut Host)) -> Result<Service, String> {
        let shown = |p: &Path| p.display().to_string();
        let (prog_path, log_path) = (state.join("program.json"), state.join("log.jsonl"));
        let compiled = serde_json::to_value(prog).map_err(|e| e.to_string())?;
        if log_path.exists() {
            let mut svc = Self::load(state, now, setup, true)?;
            let same = svc.newer(prog).map_err(|e| format!("{}: {e}", shown(state)))?;
            svc.log = Some(OpenOptions::new().append(true).open(&log_path).map_err(|e| format!("{}: {e}", shown(&log_path)))?);
            if !same {
                svc.deployed = match svc.deploy(prog, now) {
                    Ok(a) => a.trace,
                    Err(Refusal::Breaks(b)) => {
                        return Err(format!("{}: the deploy of {} v{} is refused; these breaks need a resolution:\n  {}", shown(state), prog.module, prog.version, b.join("\n  ")))
                    }
                    Err(Refusal::Other(e)) => return Err(format!("{}: {e}", shown(state))),
                };
            }
            return Ok(svc);
        }
        let mut svc = Service::in_memory(prog, now);
        svc.state = Some(state.to_path_buf());
        setup(&mut svc.host);
        {
            // The program first, then the log, each synced, then the directory:
            // a log is never there without the program it was written under.
            synced(&prog_path, &serde_json::to_string(&compiled).map_err(|e| e.to_string())?).map_err(|e| format!("{}: {e}", shown(&prog_path)))?;
            let header = Header { suture: LOG_FORMAT, module: prog.module.clone(), version: prog.version, started: now };
            synced(&log_path, &(serde_json::to_string(&header).map_err(|e| e.to_string())? + "\n")).map_err(|e| format!("{}: {e}", shown(&log_path)))?;
            File::open(state).and_then(|d| d.sync_all()).map_err(|e| format!("{}: {e}", shown(state)))?;
        }
        svc.log = Some(OpenOptions::new().append(true).open(&log_path).map_err(|e| format!("{}: {e}", shown(&log_path)))?);
        svc.read_to = std::fs::metadata(&log_path).map_err(|e| format!("{}: {e}", shown(&log_path)))?.len();
        Ok(svc)
    }

    fn replay(&mut self, path: &Path, cut: bool) -> Result<(), String> {
        let at = |n: usize| format!("{}:{}", path.display(), n + 1);
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        // Each record ends with a newline. Whatever follows the last one was
        // cut short by a crash, and never answered: it is cut off the file, so
        // the next record starts on a line of its own.
        let (lines, good) = lines(&bytes, 0, true).map_err(|e| format!("{}: {e}", path.display()))?;
        self.read_to = good as u64;
        if good < bytes.len() && cut {
            let f = OpenOptions::new().write(true).open(path).map_err(|e| format!("{}: {e}", path.display()))?;
            f.set_len(good as u64).and_then(|_| f.sync_all()).map_err(|e| format!("{}: {e}", path.display()))?;
        }
        let Some(((_, first), rest)) = lines.split_first() else { return Err(format!("{}: the log has no header", path.display())) };
        let header: Header = serde_json::from_str(first).map_err(|e| format!("{}: not a log header: {e}", at(0)))?;
        if header.suture != LOG_FORMAT {
            return Err(format!("{}: log format {} is not {LOG_FORMAT}", at(0), header.suture));
        }
        self.started = header.started;
        let mut records = vec![];
        for (n, line) in rest {
            records.push(serde_json::from_str::<Record>(line).map_err(|e| format!("{}: {e}", at(*n)))?);
        }
        self.apply_all(&records, path)
    }

    /// Applies the records of the log, each input with the model answers it caused.
    fn apply_all(&mut self, records: &[Record], path: &Path) -> Result<(), String> {
        let mut i = 0;
        while i < records.len() {
            // The model answers an input caused come right after it.
            let answers = records[i + 1..].iter().take_while(|r| matches!(r, Record::Model { .. }));
            self.host.replay = answers.map(|r| match r {
                Record::Model { key, answer } => (key.clone(), answer.clone()),
                _ => unreachable!(),
            }).collect();
            let skip = self.host.replay.len();
            // A reference missing from the secret store is an error (appendix N.2.6).
            let sealed = self.seal(records[i].clone()).map_err(|e| format!("{}: {e}", path.display()))?;
            let applied = self.apply(&sealed);
            if let Record::Deploy { version } = &records[i] {
                // A deploy that cannot be made again would replay the rest under the wrong program.
                if self.host.prog.version != *version {
                    return Err(format!("{}: the deploy of v{version} cannot be made again: {}", path.display(), applied.err().unwrap_or_default()));
                }
            }
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
        // A broken pod stops beating, so its units go to the others.
        if let Some(why) = &self.broken {
            let why = why.clone();
            self.leave();
            return Err(format!("the log could not be written ({why}), so this pod takes nothing until it restarts"));
        }
        self.beat(now)?;
        let elapsed = self.elapsed(now);
        // A clock that stepped back waits until it is past the runtime clock
        // again. Another pod may have fired the timers already (appendix O.2.7).
        let fired = self.locked(|s| {
            let from = s.host.trace.len();
            match s.host.next_due() {
                Some(due) if due <= elapsed && elapsed > s.host.now => {
                    s.applied(Record::Pass(elapsed - s.host.now))?;
                    Ok(s.host.trace[from..].to_vec())
                }
                _ => Ok(vec![]),
            }
        })?;
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
        let elapsed = self.elapsed(now);
        let (from, refused) = self.locked(|s| {
            let from = s.host.trace.len();
            if !matches!(r, Record::Pass(_)) && elapsed > s.host.now {
                s.applied(Record::Pass(elapsed - s.host.now))?;
            }
            Ok((from, s.applied(r)?))
        })?;
        self.post_new(now)?;
        Ok(Applied { trace: self.host.trace[from..].to_vec(), refused })
    }

    /// Puts the record's secret values in the secret store and references in
    /// their place, so the log never holds a secret (appendix N.2.4).
    fn seal(&mut self, r: Record) -> Result<Record, String> {
        let h = &mut self.host;
        let items = |h: &mut Host, kind: &str, items: Items| -> Result<Items, String> {
            items
                .into_iter()
                .map(|(list, attrs)| {
                    let element = h.prog.lists.iter().find(|l| l.owner == kind && l.name == list).map_or(String::new(), |l| l.element.clone());
                    h.seal(&element, attrs).map(|attrs| (list, attrs))
                })
                .collect()
        };
        Ok(match r {
            Record::World { kind, label, attrs, items: xs } => {
                let attrs = h.seal(&kind, attrs)?;
                let xs = items(h, &kind, xs)?;
                Record::World { kind, label, attrs, items: xs }
            }
            Record::New { by, verb, kind, label, attrs, items: xs } => {
                let attrs = h.seal(&kind, attrs)?;
                let xs = items(h, &kind, xs)?;
                Record::New { by, verb, kind, label, attrs, items: xs }
            }
            Record::Change { by, target, attr, value } => {
                let kind = h.unit(&target).map(|u| u.root.kind).unwrap_or_default();
                let value = h.seal(&kind, vec![(attr.clone(), value)])?.remove(0).1;
                Record::Change { by, target, attr, value }
            }
            Record::Answer { adapter, outcome, key, target, data } => {
                let ask = h.pending().into_iter().find(|a| {
                    a.adapter == adapter && key.as_ref().map_or_else(|| target.as_ref().is_none_or(|t| *t == a.label), |k| *k == a.key)
                });
                // Without a pending request (a late or a stray answer), each
                // value is sealed if any kind marks an attribute of its name
                // secret, so the log never holds it (appendix N.2.4).
                let data = match ask.map(|a| a.kind.clone()) {
                    Some(kind) => h.seal(&kind, data)?,
                    None => {
                        let mut out = vec![];
                        for (a, v) in data {
                            let kind = h.prog.kinds.iter().find(|k| k.attr(&a).is_some_and(|x| x.mark == Some(suture_lang::ir::Mark::Secret))).map(|k| k.name.clone());
                            out.extend(h.seal(&kind.unwrap_or_default(), vec![(a, v)])?);
                        }
                        out
                    }
                };
                Record::Answer { adapter, outcome, key, target, data }
            }
            r => r,
        })
    }

    /// Applies one record and logs it with the model answers it caused.
    fn applied(&mut self, r: Record) -> Result<Option<String>, String> {
        let r = self.seal(r)?;
        let asked = self.host.model_answers.len();
        let refused = self.apply(&r).err();
        let mut lines = vec![r];
        lines.extend(self.host.model_answers[asked..].iter().map(|(key, answer)| Record::Model { key: key.clone(), answer: answer.clone() }));
        self.write(&lines)?;
        Ok(refused)
    }

    fn write(&mut self, records: &[Record]) -> Result<(), String> {
        if self.log.is_none() {
            return Ok(());
        }
        // The secrets a line refers to are on disk before the line is.
        if self.host.secrets.len() != self.kept_secrets
            && let Err(e) = self.keep_secrets() {
            self.broken = Some(e.clone());
            return Err(format!("the secret store could not be written ({e}), so no input is taken until the service restarts"));
        }
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
        self.read_to += text.len() as u64;
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
            Record::Event { by, verb, target, part, data } => h.input_event(Some(&caller(by, verb)), target, part.as_deref(), data),
            Record::Change { by, target, attr, value } => h.input_change(Some(&caller(by, "changes")), target, attr, value.clone()),
            Record::Answer { adapter, outcome, key: Some(key), data, .. } => h.answer_key(adapter, outcome, data, key),
            Record::Answer { adapter, outcome, target, data, .. } => h.answer(adapter, outcome, data, target.as_deref()),
            Record::Model { .. } => Ok(()),
            Record::Deploy { version } => {
                let prog = match self.programs.get(version) {
                    Some(p) => p.clone(),
                    None => {
                        let state = self.state.as_ref().ok_or("a deploy needs the state directory")?;
                        let path = Self::program_file(state, *version, false);
                        let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
                        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?
                    }
                };
                self.host.deploy(prog)
            }
        }
    }

    /// Posts each waiting request this process has not posted yet to its
    /// adapter, if `adapters.json` names it. A reply with an outcome is the
    /// answer; anything else leaves the request waiting (appendix L.2.8).
    pub fn post_new(&mut self, now: u64) -> Result<(), String> {
        if self.broken.is_some() {
            return Ok(());
        }
        // A pod posts the requests of its own units only (appendix O.2.5).
        let pods = self.pods(now);
        // A request whose unit went to another pod, or that is answered, is
        // forgotten, so it is posted again if its unit comes back (O.2.6).
        let keep: BTreeSet<String> = self.host.pending().into_iter().filter(|a| owner(&a.label, &pods) == Some(&self.pod)).map(|a| a.key.clone()).collect();
        self.posted.retain(|k| keep.contains(k));
        self.retry.retain(|k, _| keep.contains(k));
        let began = std::time::Instant::now();
        loop {
            let due = |k: &String| self.retry.get(k).is_none_or(|at| *at <= now);
            let mine = |a: &Ask| owner(&a.label, &pods) == Some(&self.pod);
            let next = self.host.pending().into_iter().find(|a| self.adapters.contains_key(&a.adapter) && !self.posted.contains(&a.key) && due(&a.key) && mine(a)).cloned();
            let Some(ask) = next else { return Ok(()) };
            // A long run of slow posts does not let this pod's beat lapse.
            self.beat(now + began.elapsed().as_secs())?;
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
            let body = request_json(&self.host, &ask, true);
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
/// A request as an adapter receives it. Only with `reveal` does it carry the
/// marked fields' values; otherwise they are masked (appendix N.2.4–N.2.5).
pub fn request_json(host: &Host, a: &Ask, reveal: bool) -> Value {
    let prog = &host.prog;
    let field = |k: &String, v: &Val| if reveal { to_json(&host.reveal(v), &prog.currency) } else { masked(prog, k, v) };
    let sends: serde_json::Map<String, Value> = a.sends.iter().map(|(k, v)| (k.clone(), field(k, v))).collect();
    json!({ "key": a.key, "adapter": a.adapter, "verb": a.verb, "kind": a.kind, "unit": a.label, "sends": sends })
}

/// A value as a read shows it: a marked one masked, as in the trace.
fn masked(prog: &Program, attr: &str, v: &Val) -> Value {
    match crate::interp::shown(prog, attr, v) {
        s if s == "(secret)" || s == "(personal)" => json!(s),
        _ => to_json(v, &prog.currency),
    }
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
