//! The `suture` command (spec appendix B.1):
//!
//! - `suture build <dir>`
//! - `suture run <dir> --scenario <file> [--models]`
//! - `suture explain <dir> --scenario <file> <name>`
//! - `suture serve <dir> [--state <dir>] [--listen <addr>] [--pod <name>] [--models]` (appendices L, O)
//! - `suture lsp`: the language server, over stdin and stdout (appendix P)

mod check;
mod help;
mod init;
mod lsp;

use std::path::Path;
use std::process::ExitCode;

use suture_lang::ir::Program;

/// A line of output, into a string.
macro_rules! outln {
    ($o:ident, $($t:tt)*) => {{
        $o += &format!($($t)*);
        $o.push('\n');
    }};
}

const USAGE: &str =
    "usage:\n  suture init <dir>\n  suture help [<topic> | all | --for <dir or file>]\n  suture check <dir> [--json]\n  suture build <dir>\n  suture run <dir> --scenario <file> [--models]\n  suture explain <dir> --scenario <file> <name>\n  suture serve <dir> [--state <dir>] [--listen <addr>] [--pod <name>] [--models]\n  suture deploy <dir> [--state <dir>]\n  suture lsp";

/// Where `suture serve` listens unless told otherwise.
const LISTEN: &str = "127.0.0.1:7878";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let result = match args.as_slice() {
        ["help"] | ["--help"] | ["-h"] => help::help(None).map(|t| print_ok(&t)),
        ["help", "--for", path] => help::help_for(std::path::Path::new(path)).map(|t| print_ok(&t)),
        ["help", topic] => help::help(Some(topic)).map(|t| print_ok(&t)),
        ["init", dir] => init::init(Path::new(dir)).map(|t| print_ok(&t)),
        ["check", dir] => Ok(print_check(Path::new(dir), false)),
        ["check", dir, "--json"] | ["check", "--json", dir] => Ok(print_check(Path::new(dir), true)),
        ["build", dir] => build(Path::new(dir)),
        ["run", dir, "--scenario", file] => scenarios(Path::new(dir), Path::new(file), None, false),
        ["run", dir, "--scenario", file, "--models"] => scenarios(Path::new(dir), Path::new(file), None, true),
        ["explain", dir, "--scenario", file, name] => scenarios(Path::new(dir), Path::new(file), Some(*name), false),
        ["serve", dir, flags @ ..] => serve(Path::new(dir), flags),
        ["deploy", dir] => deploy(Path::new(dir), None),
        ["deploy", dir, "--state", state] => deploy(Path::new(dir), Some(Path::new(state))),
        ["lsp"] => lsp::run(),
        _ => Err(USAGE.to_string()),
    };
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(msg) => {
            eprintln!("{msg}");
            ExitCode::from(2)
        }
    }
}

fn print_ok(text: &str) -> bool {
    print!("{text}");
    true
}

/// `suture check`: exit 0 when it compiles and every scenario passes.
fn print_check(dir: &Path, json: bool) -> bool {
    let found = check::check(dir);
    if json {
        println!("{}", found.json());
    } else {
        print!("{}", found.text());
    }
    found.passed()
}

fn compile(dir: &Path) -> Result<Program, String> {
    suture_lang::compile_dir(dir).map_err(|errors| errors.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("\n"))
}

/// Compiles, writes the program file, and prints every rule's read-back.
fn build(dir: &Path) -> Result<bool, String> {
    print!("{}", built(dir)?);
    Ok(true)
}

/// [`build`]'s work and its output: the program file written, and the read-back.
fn built(dir: &Path) -> Result<String, String> {
    let mut text = String::new();
    let prog = compile(dir)?;
    let out = dir.join(format!("{}-v{}.sut", prog.module, prog.version));
    let json = serde_json::to_string_pretty(&prog).map_err(|e| e.to_string())?;
    std::fs::write(&out, json).map_err(|e| format!("{}: {e}", out.display()))?;
    let manifest = suture_lang::manifest::manifest(&prog);
    let mpath = dir.join(format!("{}-v{}.manifest", prog.module, prog.version));
    std::fs::write(&mpath, &manifest).map_err(|e| format!("{}: {e}", mpath.display()))?;
    outln!(text, "{} v{} → {}, {}", prog.module, prog.version, out.display(), mpath.display());
    outln!(text, 
        "  kinds {} · effects {} · rules {} · goals {}",
        prog.kinds.len(),
        prog.effects.len(),
        prog.rules.len(),
        prog.goals.len()
    );
    for w in &prog.warnings {
        outln!(text, "warning: {w}");
    }
    for r in &prog.rules {
        outln!(text, "\nrule {}: {}", r.name, r.source);
        outln!(text, "  understood as:   {}", r.formula);
        outln!(text, "  in other words:  {}", r.paraphrase);
        outln!(text, "  cost:            {}", r.tier);
        if let Some(a) = &r.applies {
            outln!(text, "  applies to:      {} created from version {}", prog.kind(&a.kind).map_or(&a.kind, |k| &k.plural), a.from);
        }
        if let Some(src) = suture_lang::readback::rule_sources(&prog, r) {
            outln!(text, "  sources:         {src}");
        }
    }
    if !prog.sources.is_empty() {
        outln!(text, "\nsourcing:");
        for (i, s) in prog.sources.iter().enumerate() {
            outln!(text, "  {}", suture_lang::readback::source_line(&prog, s));
            // After an attribute's last source, who picks among several.
            let last = !prog.sources[i + 1..].iter().any(|x| x.kind == s.kind && x.attr == s.attr);
            if last && prog.sources_of(&s.kind, &s.attr).len() > 1 {
                let point = format!("{}'s {}", s.kind, s.attr);
                let fallback = "otherwise in the order they are declared".to_string();
                outln!(text, "    decided by: {}", suture_lang::readback::handlers_for(&prog, &point, &s.kind, fallback).join("; "));
            }
        }
    }
    if !prog.bindings.is_empty() {
        outln!(text, "\nchoices:");
        for b in &prog.bindings {
            outln!(text, "  {}", suture_lang::readback::choice(&prog, b));
            if !prog.decisions.is_empty() && !b.key_attr.is_empty() {
                outln!(text, "    decided by: {}", suture_lang::readback::handlers(&prog, b).join("; "));
            }
        }
    }
    if !prog.slots.is_empty() {
        outln!(text, "\nslots:");
        for s in &prog.slots {
            outln!(text, "  {}'s {}: {}", s.owner, s.name, s.alternatives.join(", then "));
            if !prog.decisions.is_empty() {
                let fallback = format!("otherwise `prefer {}`", s.name);
                outln!(text, "    decided by: {}", suture_lang::readback::handlers_for(&prog, &s.name, &s.owner, fallback).join("; "));
            }
            outln!(text, "    {}", suture_lang::readback::slot_policy(&prog, s));
        }
    }
    if !prog.roles.is_empty() || !prog.admits.is_empty() {
        outln!(text, "\nroles: {}", suture_lang::readback::roles(&prog));
        outln!(text, "admitted:");
        let mut seen = vec![];
        for a in &prog.admits {
            if !seen.contains(&&a.source) {
                outln!(text, "  {}", a.source);
                seen.push(&a.source);
            }
        }
        let watches = suture_lang::readback::watches(&prog);
        if !watches.is_empty() {
            outln!(text, "watches:");
            for w in watches {
                outln!(text, "  {w}");
            }
        }
    }
    if !prog.effects.is_empty() || !prog.sources.is_empty() {
        text += &format!("\n{}", suture_lang::manifest::adapters(&prog));
    }
    Ok(text)
}

/// Runs every scenario in `file` and prints its trace. With `explain`, it
/// also explains that unit at the end of each scenario that has it. With
/// `models`, the backends in the module's `models.json` answer the models a
/// scenario does not script (appendix K.2.7).
fn scenarios(dir: &Path, file: &Path, explain: Option<&str>, models: bool) -> Result<bool, String> {
    let (out, passed) = scenarios_text(dir, file, explain, models)?;
    print!("{out}");
    Ok(passed)
}

/// [`scenarios`]' work and its output, and whether every scenario passed.
fn scenarios_text(dir: &Path, file: &Path, explain: Option<&str>, models: bool) -> Result<(String, bool), String> {
    let mut text = String::new();
    let prog = compile(dir)?;
    let config = if models {
        let path = dir.join("models.json");
        Some(std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?)
    } else {
        None
    };
    if let Some(text) = &config {
        // Checked once up front, so a bad file stops the run before any scenario.
        suture_runtime::backend::load(text, || Box::new(suture_runtime::backend::Curl))?;
    }
    let src = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let list = suture_lang::scenario::parse_scenarios(&file.display().to_string(), &src).map_err(|e| e.to_string())?;
    let mut all_passed = true;
    for sc in &list {
        let report = suture_runtime::scenario::run_with(&prog, sc, |host| {
            let backends = config.as_deref().map(|t| suture_runtime::backend::load(t, || Box::new(suture_runtime::backend::Curl)));
            for (name, b) in backends.and_then(Result::ok).unwrap_or_default() {
                host.models.register_backend(&name, b);
            }
        });
        outln!(text, "scenario \"{}\"", report.title);
        for line in &report.host.trace {
            outln!(text, "  {line}");
        }
        if let Some(unit) = explain.and_then(|name| report.host.unit(name)) {
            text.push('\n');
            let host = &report.host;
            for line in suture_runtime::explain::explain_at(&prog, &unit, Some(host.now as i64), |k| host.deadline(k).map(|d| d - host.now)) {
                outln!(text, "  {line}");
            }
            for line in host.gate_for(&unit.label) {
                outln!(text, "  gate: {line}");
            }
        }
        for f in &report.failures {
            outln!(text, "  ✗ {f}");
        }
        outln!(text, "  {}\n", if report.passed() { "passed" } else { "FAILED" });
        all_passed &= report.passed();
    }
    Ok((text, all_passed))
}

fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// The deploy check against the live state, which it never writes (appendix M.2.7).
fn deploy(dir: &Path, state: Option<&Path>) -> Result<bool, String> {
    use suture_runtime::service::{Check, Service};
    let prog = compile(dir)?;
    let state = state.map_or_else(|| Service::default_state(dir), Path::to_path_buf);
    if !state.join("log.jsonl").exists() {
        return Err(format!("{}: no log, so nothing is live; `suture serve` starts it", state.display()));
    }
    match Service::check(&prog, &state, unix_now())? {
        Check::Same => {
            println!("{} v{}: the state already runs this program", prog.module, prog.version);
            Ok(true)
        }
        Check::Deploys(lines) => {
            println!("{} v{}: the deploy can go ahead; it would do this:", prog.module, prog.version);
            for l in lines {
                println!("  {l}");
            }
            Ok(true)
        }
        Check::Refused(breaks) => {
            println!("{} v{}: the deploy would be refused; these breaks need a resolution:", prog.module, prog.version);
            for b in breaks {
                println!("  {b}");
            }
            Ok(false)
        }
    }
}

/// Serves the program until the process is stopped (appendix L): its state
/// directory replayed first, `adapters.json` and (with `--models`)
/// `models.json` read from beside it.
fn serve(dir: &Path, flags: &[&str]) -> Result<bool, String> {
    let (mut state, mut listen, mut models) = (suture_runtime::service::Service::default_state(dir), LISTEN.to_string(), false);
    let mut pod = "solo".to_string();
    let mut rest = flags.iter();
    while let Some(f) = rest.next() {
        match *f {
            "--state" => state = rest.next().map(Path::new).ok_or(USAGE)?.to_path_buf(),
            "--listen" => listen = rest.next().ok_or(USAGE)?.to_string(),
            "--models" => models = true,
            "--pod" => pod = rest.next().ok_or(USAGE)?.to_string(),
            _ => return Err(USAGE.to_string()),
        }
    }
    let prog = compile(dir)?;
    let read = |name: &str| -> Result<Option<String>, String> {
        let path = dir.join(name);
        match std::fs::read_to_string(&path) {
            Ok(t) => Ok(Some(t)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    };
    let backends = match (models, read("models.json")?) {
        (false, _) => vec![],
        (true, None) => return Err(format!("{}: not found", dir.join("models.json").display())),
        (true, Some(t)) => suture_runtime::backend::load(&t, || Box::new(suture_runtime::backend::Curl))?,
    };
    let adapters = read("adapters.json")?.map_or(Ok(Default::default()), |t| suture_runtime::service::adapters(&t))?;
    let mut svc = suture_runtime::service::Service::open(&prog, &state, unix_now(), |host| {
        for (name, b) in backends {
            host.models.register_backend(&name, b);
        }
    })?;
    let replayed = svc.host.trace.len() - svc.deployed.len();
    svc.join(&pod, unix_now())?;
    // Whatever stops it after it joined, the pod leaves, so its units move at once.
    let served = serving(&mut svc, &prog, &state, &listen, &pod, replayed, adapters);
    if served.is_err() {
        svc.leave();
    }
    served
}

fn serving(
    svc: &mut suture_runtime::service::Service,
    prog: &suture_lang::ir::Program,
    state: &Path,
    listen: &str,
    pod: &str,
    replayed: usize,
    adapters: std::collections::BTreeMap<String, suture_runtime::service::Adapter>,
) -> Result<bool, String> {
    svc.connect(adapters, Box::new(suture_runtime::backend::Curl));
    let listener = std::net::TcpListener::bind(listen).map_err(|e| format!("{listen}: {e}"))?;
    let operator = std::env::var("SUTURE_OPERATOR_TOKEN").ok();
    if operator.as_deref().is_some_and(|t| t.trim().is_empty()) {
        return Err("SUTURE_OPERATOR_TOKEN is empty; set a token, or unset it to refuse operator requests".into());
    }
    let bound = listener.local_addr().map_err(|e| e.to_string())?;
    println!("{} v{} on http://{bound}, state in {}, pod {pod}", prog.module, prog.version, state.display());
    let others: Vec<String> = svc.pods(unix_now()).into_iter().filter(|p| *p != pod).collect();
    if !others.is_empty() {
        println!("  with pods {}", others.join(", "));
    }
    if !bound.ip().is_loopback() {
        println!("  warning: listening beyond this machine; only the gateway may reach this port, since it trusts the Suture-Caller header");
    }
    for l in &svc.deployed {
        println!("  {l}");
    }
    println!("  replayed {replayed} trace lines; operator requests {}", if operator.is_some() { "need the token" } else { "refused: SUTURE_OPERATOR_TOKEN is not set" });
    // Requests that were waiting when the service stopped are posted again.
    svc.post_new(unix_now())?;
    stop_on_signals();
    suture_runtime::http::serve(svc, listener, operator.as_deref(), &STOP, unix_now, &mut std::io::stdout()).map_err(|e| e.to_string())?;
    println!("stopped; pod {pod} left");
    Ok(true)
}

/// Set when the process is asked to stop: the service finishes the request
/// it is on, leaves the pods, so its units move at once, and exits (appendix U.2.2).
static STOP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(unix)]
fn stop_on_signals() {
    const SIGINT: i32 = 2;
    const SIGTERM: i32 = 15;
    unsafe extern "C" {
        fn signal(signum: i32, handler: extern "C" fn(i32)) -> usize;
    }
    // Only an atomic store, which is safe in a signal handler.
    extern "C" fn stop(_: i32) {
        STOP.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    // SAFETY: `signal` is the C library's, and the handler only stores to an atomic.
    unsafe {
        signal(SIGINT, stop);
        signal(SIGTERM, stop);
    }
}

#[cfg(not(unix))]
fn stop_on_signals() {}
