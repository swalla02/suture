//! The runtime's data: values, the nodes of a unit, messages and outside
//! requests, and how each is carried in kernel payloads.

use std::collections::BTreeMap;
use std::cell::Cell;

use suture::Value;
use suture_lang::ir::{Lit, Ty};

/// An attribute's value. Money is in cents; times and durations are in
/// seconds of the runtime clock.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Val {
    Text(String),
    Bool(bool),
    Money(i64),
    Count(i64),
    /// A reference to a shared entity: its unit's label (appendix F.2.2).
    Ref(String),
    /// References to shared entities, sorted, each once.
    Set(Vec<String>),
    Time(i64),
    Duration(i64),
    /// A secret Text, by its number in the secret store: the value itself
    /// never enters a unit (appendix N.2.4).
    Secret(u64),
    /// Media, by the SHA-256 of its content: the content itself never enters
    /// a unit, and is kept in the media store (appendix V.2.2).
    Media { sha256: String, mime: String, bytes: u64 },
}

impl Val {
    pub fn from_lit(l: &Lit) -> Val {
        match l {
            Lit::Text(s) => Val::Text(s.clone()),
            Lit::Bool(b) => Val::Bool(*b),
            Lit::Money(c) => Val::Money(*c),
            Lit::Count(n) => Val::Count(*n),
            Lit::Duration(s) => Val::Duration(*s),
            Lit::Time(t) => Val::Time(*t),
        }
    }

    pub fn ty(&self) -> Ty {
        match self {
            Val::Text(_) | Val::Secret(_) => Ty::Text,
            Val::Bool(_) => Ty::Bool,
            Val::Money(_) => Ty::Money,
            Val::Count(_) => Ty::Count,
            Val::Ref(_) => Ty::Ref,
            Val::Set(_) => Ty::Set,
            Val::Time(_) => Ty::Time,
            Val::Duration(_) => Ty::Duration,
            Val::Media { .. } => Ty::Media,
        }
    }

    /// The value as people read it: `"alcohol"`, `120 EUR`, `12.50 EUR`, `3`, `yes`.
    pub fn show(&self, currency: &str) -> String {
        match self {
            Val::Text(s) => format!("\"{s}\""),
            Val::Bool(true) => "yes".into(),
            Val::Bool(false) => "no".into(),
            Val::Money(c) if c % 100 == 0 => format!("{} {currency}", c / 100),
            Val::Money(c) => format!("{}.{:02} {currency}", c / 100, (c % 100).abs()),
            Val::Count(n) => n.to_string(),
            Val::Ref(l) => l.clone(),
            Val::Set(ls) => format!("{{{}}}", ls.join(", ")),
            Val::Time(t) => time_text(*t),
            Val::Duration(d) => suture_lang::readback::duration_text(*d),
            Val::Secret(_) => "(secret)".into(),
            Val::Media { sha256, mime, bytes } => format!("({mime}, {bytes} bytes, sha256 {})", &sha256[..sha256.len().min(12)]),
        }
    }

    pub(crate) fn to_value(&self) -> Value {
        match self {
            Val::Text(s) => Value::tuple([Value::from(0i64), text(s)]),
            Val::Bool(b) => Value::tuple([Value::from(1i64), Value::from(*b)]),
            Val::Money(c) => Value::tuple([Value::from(2i64), Value::from(*c)]),
            Val::Count(n) => Value::tuple([Value::from(3i64), Value::from(*n)]),
            Val::Ref(l) => Value::tuple([Value::from(4i64), text(l)]),
            Val::Set(ls) => Value::tuple([Value::from(5i64), Value::list(ls.iter().map(|l| text(l)))]),
            Val::Time(t) => Value::tuple([Value::from(6i64), Value::from(*t)]),
            Val::Duration(d) => Value::tuple([Value::from(7i64), Value::from(*d)]),
            Val::Secret(n) => Value::tuple([Value::from(8i64), Value::from(*n as i64)]),
            Val::Media { sha256, mime, bytes } => Value::tuple([Value::from(9i64), Value::tuple([text(sha256), text(mime), Value::from(*bytes as i64)])]),
        }
    }

    pub(crate) fn from_value(v: &Value) -> Result<Val, String> {
        let [tag, x] = fields::<2>(v, "value")?;
        Ok(match int(tag)? {
            0 => Val::Text(string(x)?),
            1 => Val::Bool(x.as_bool().ok_or_else(|| format!("not a yes or no: {x}"))?),
            2 => Val::Money(int(x)?),
            3 => Val::Count(int(x)?),
            4 => Val::Ref(string(x)?),
            5 => Val::Set(items(x)?.iter().map(string).collect::<Result<_, _>>()?),
            6 => Val::Time(int(x)?),
            7 => Val::Duration(int(x)?),
            8 => Val::Secret(u64::try_from(int(x)?).map_err(|e| e.to_string())?),
            9 => {
                let [sha256, mime, bytes] = fields::<3>(x, "media")?;
                Val::Media { sha256: string(sha256)?, mime: string(mime)?, bytes: u64::try_from(int(bytes)?).map_err(|e| e.to_string())? }
            }
            t => return Err(format!("unknown value tag {t}")),
        })
    }
}

/// The calendar origin of the runtime clock: 0 is 2026-01-01 00:00 UTC
/// (appendix F.2.12), as days since 1970-01-01.
pub const EPOCH_DAYS: i64 = 20_454;

/// The civil date of a day counted from 1970-01-01 (Howard Hinnant's algorithm).
pub fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// `2026-01-03 14:05`, for a time on the runtime clock.
pub fn time_text(t: i64) -> String {
    let days = t.div_euclid(86_400);
    let secs = t.rem_euclid(86_400);
    let (y, m, d) = civil(EPOCH_DAYS + days);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}", secs / 3_600, secs % 3_600 / 60)
}

/// How many days a span may cover (appendix AD.2.2).
pub const SPAN_DAYS: i64 = 3_660;

/// The take of an entity that spans periods names them all, as
/// `span day ⟨from⟩ ⟨to⟩` after the `@` (appendix AD.2.2).
pub fn span_key(period: &str, from: i64, to: i64) -> String {
    format!("span {period} {from} {to}")
}

/// A take's total as a trace shows it: a span by its dates,
/// `… per day from its from to its to, 2026-01-05 to 2026-01-08`.
pub fn take_text(name: &str) -> String {
    // A group's values, as written (appendix AP.2.2).
    if let Some((base, group)) = name.split_once('#') {
        return format!("{base} {}", group.replace("%40", "@").replace("%23", "#").replace("%25", "%"));
    }
    let Some((base, span)) = name.split_once("@span ") else { return name.to_string() };
    let mut parts = span.split(' ').skip(1).map(str::parse::<i64>);
    match (parts.next(), parts.next()) {
        (Some(Ok(from)), Some(Ok(to))) => {
            let date = |t: i64| time_text(t).trim_end_matches(" 00:00").to_string();
            format!("{base}, {} to {}", date(from), date(to))
        }
        _ => name.to_string(),
    }
}

/// The periods a take falls in: the one it names, or each period a span
/// covers, first to last, both ends included. A span that ends before it
/// starts covers none.
pub fn periods_in(at: &str) -> Result<Vec<String>, String> {
    let Some(span) = at.strip_prefix("span ") else { return Ok(vec![at.to_string()]) };
    let mut parts = span.split(' ');
    let (Some(per), Some(Ok(from)), Some(Ok(to)), None) = (parts.next(), parts.next().map(str::parse::<i64>), parts.next().map(str::parse::<i64>), parts.next()) else {
        return Err(format!("not a span: {at}"));
    };
    let (first, last) = (from.div_euclid(86_400), to.div_euclid(86_400));
    if last - first >= SPAN_DAYS {
        return Err(format!("a span covers at most {SPAN_DAYS} days, and this one covers {}", last - first + 1));
    }
    let mut out: Vec<String> = vec![];
    for d in first..=last {
        let k = period_key(per, d * 86_400);
        if out.last() != Some(&k) {
            out.push(k);
        }
    }
    Ok(out)
}

/// The period a time falls in (appendix F.2.12), in UTC: `2026-01-17` for
/// a day, the Monday it starts on for a week, `2026-01` for a month, `2026`
/// for a year.
pub fn period_key(period: &str, t: i64) -> String {
    let days = EPOCH_DAYS + t.div_euclid(86_400);
    let date = |d: i64| {
        let (y, m, d) = civil(d);
        format!("{y:04}-{m:02}-{d:02}")
    };
    let (y, m, _) = civil(days);
    match period {
        // 1970-01-01, day 0, was a Thursday.
        "week" => format!("week of {}", date(days - (days + 3).rem_euclid(7))),
        "month" => format!("{y:04}-{m:02}"),
        "year" => format!("{y:04}"),
        _ => date(days),
    }
}

/// A shared entity as another unit carries it (appendix F.2.3): its kind,
/// when it was created, its attributes, and the totals across units that
/// the units referring to it read (appendix AA.2.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fact {
    pub kind: String,
    pub created: i64,
    pub attrs: BTreeMap<String, Val>,
    pub taken: BTreeMap<String, Val>,
}

impl Fact {
    fn to_value(&self) -> Value {
        let mut fields = vec![text(&self.kind), Value::from(self.created), attrs_value(&self.attrs)];
        // Only a fact with totals has the fourth field, so other facts
        // encode as before.
        if !self.taken.is_empty() {
            fields.push(attrs_value(&self.taken));
        }
        Value::tuple(fields)
    }

    fn from_value(v: &Value) -> Result<Fact, String> {
        if let Ok([kind, created, attrs, taken]) = fields::<4>(v, "fact") {
            return Ok(Fact { kind: string(kind)?, created: int(created)?, attrs: attrs_from(attrs)?, taken: attrs_from(taken)? });
        }
        let [kind, created, attrs] = fields::<3>(v, "fact")?;
        Ok(Fact { kind: string(kind)?, created: int(created)?, attrs: attrs_from(attrs)?, taken: BTreeMap::new() })
    }
}

/// Carried facts, by the label of the unit they come from.
pub type Facts = BTreeMap<String, Fact>;

fn facts_value(facts: &Facts) -> Value {
    Value::list(facts.iter().map(|(label, f)| Value::tuple([text(label), f.to_value()])))
}

fn facts_from(v: &Value) -> Result<Facts, String> {
    let mut carried = Facts::new();
    for f in items(v)? {
        let [label, fact] = fields::<2>(f, "carried fact")?;
        carried.insert(string(label)?, Fact::from_value(fact)?);
    }
    Ok(carried)
}

/// Where a binding of a part stands (appendix F.2.5–F.2.10).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Bind {
    /// The host is choosing; these candidates refused it.
    Asked { skip: Vec<String> },
    /// No candidate was left; the host wakes it when one may be.
    Waiting { skip: Vec<String> },
    /// Granted by the unit `to`, which took `amount` as `name`.
    Granted { to: String, taken: Option<(String, Val)> },
}

impl Bind {
    fn to_value(&self) -> Value {
        let labels = |ls: &[String]| Value::list(ls.iter().map(|l| text(l)));
        match self {
            Bind::Asked { skip } => Value::tuple([Value::from(0i64), labels(skip)]),
            Bind::Waiting { skip } => Value::tuple([Value::from(1i64), labels(skip)]),
            Bind::Granted { to, taken } => Value::tuple([Value::from(2i64), text(to), taken_value(taken)]),
        }
    }

    fn from_value(v: &Value) -> Result<Bind, String> {
        let xs = items(v)?;
        let labels = |x: &Value| items(x)?.iter().map(string).collect::<Result<Vec<_>, _>>();
        Ok(match (xs.first().map(int), xs.len()) {
            (Some(Ok(0)), 2) => Bind::Asked { skip: labels(&xs[1])? },
            (Some(Ok(1)), 2) => Bind::Waiting { skip: labels(&xs[1])? },
            (Some(Ok(2)), 3) => Bind::Granted { to: string(&xs[1])?, taken: taken_from(&xs[2])? },
            _ => return Err(format!("not a binding: {v}")),
        })
    }
}

/// An amount taken: the running total's name and the amount, if any.
fn taken_value(t: &Option<(String, Val)>) -> Value {
    Value::list(t.iter().map(|(name, v)| Value::tuple([text(name), v.to_value()])))
}

fn taken_from(v: &Value) -> Result<Option<(String, Val)>, String> {
    match items(v)? {
        [] => Ok(None),
        [x] => {
            let [name, amount] = fields::<2>(x, "amount taken")?;
            Ok(Some((string(name)?, Val::from_value(amount)?)))
        }
        _ => Err(format!("not an amount taken: {v}")),
    }
}

fn attrs_value(attrs: &BTreeMap<String, Val>) -> Value {
    Value::list(attrs.iter().map(|(k, v)| Value::tuple([text(k), v.to_value()])))
}

fn attrs_from(v: &Value) -> Result<BTreeMap<String, Val>, String> {
    let mut map = BTreeMap::new();
    for a in items(v)? {
        let [k, x] = fields::<2>(a, "attribute")?;
        map.insert(string(k)?, Val::from_value(x)?);
    }
    Ok(map)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Idle,
    Requested,
    Done,
    Failed,
}

impl Status {
    fn code(self) -> i64 {
        match self {
            Status::Idle => 0,
            Status::Requested => 1,
            Status::Done => 2,
            Status::Failed => 3,
        }
    }

    fn from_code(c: i64) -> Result<Status, String> {
        Ok(match c {
            0 => Status::Idle,
            1 => Status::Requested,
            2 => Status::Done,
            3 => Status::Failed,
            _ => return Err(format!("unknown status {c}")),
        })
    }
}

/// Where a sourced attribute's value stands (appendix I.2).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Fetch {
    /// How many fetches it has made: the open or last one's number.
    pub n: u32,
    /// Sends of the open fetch; 0 while none is open.
    pub sends: u32,
    /// When the value was fetched, or fell back, if it has one.
    pub at: Option<i64>,
    /// Until when its `good for` trusts it; `None`: for ever.
    pub until: Option<i64>,
    /// The last fetch failed, and no recovery set a value.
    pub failed: bool,
    /// The value is the source's `otherwise use`.
    pub fallback: bool,
    /// The adapter of the source the open or last fetch asked (appendix K.2.4).
    pub via: String,
    /// The sources this fetch has asked and that failed, in order.
    pub tried: Vec<String>,
}

impl Fetch {
    /// Whether the value can be used now by a rule that accepts values at
    /// most `bound` seconds old.
    pub fn fresh(&self, now: i64, bound: Option<u64>) -> bool {
        let Some(at) = self.at else { return false };
        self.until.is_none_or(|u| now < u) && bound.is_none_or(|b| now - at <= b as i64)
    }

    fn to_value(&self) -> Value {
        let opt = |x: Option<i64>| Value::list(x.into_iter().map(Value::from));
        Value::tuple([
            Value::from(i64::from(self.n)),
            Value::from(i64::from(self.sends)),
            opt(self.at),
            opt(self.until),
            Value::from(self.failed),
            Value::from(self.fallback),
            text(&self.via),
            Value::list(self.tried.iter().map(|t| text(t))),
        ])
    }

    fn from_value(v: &Value) -> Result<Fetch, String> {
        let [n, sends, at, until, failed, fallback, via, tried] = fields::<8>(v, "fetch")?;
        let opt = |x: &Value| items(x)?.first().map(int).transpose();
        let small = |x: &Value| int(x).and_then(|i| u32::try_from(i).map_err(|e| e.to_string()));
        let flag = |x: &Value| x.as_bool().ok_or_else(|| format!("not a yes or no: {x}"));
        Ok(Fetch {
            n: small(n)?,
            sends: small(sends)?,
            at: opt(at)?,
            until: opt(until)?,
            failed: flag(failed)?,
            fallback: flag(fallback)?,
            via: string(via)?,
            tried: items(tried)?.iter().map(string).collect::<Result<_, _>>()?,
        })
    }
}

/// One entity of a unit: its root, or a part (spec appendix B.3.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    pub kind: String,
    pub attrs: BTreeMap<String, Val>,
    /// When it was created, on the runtime clock (appendix F.2.14).
    pub created: i64,
    pub status: Status,
    /// What the outside world reported, once it has.
    pub outcome: Option<String>,
    /// Which attempt of its need this part is, from 1.
    pub attempt: u32,
    /// How many times its request has gone out: 0 until it is requested, at
    /// the program's `attempts` (appendix C.3.3, E.2.8).
    pub sends: u32,
    /// The rule its request is waiting on, if any. A part with no effect uses
    /// it for the rule that keeps it from being done.
    pub waiting: Option<String>,
    /// The needs whose next part waits on a rule: the part's kind, and the rule.
    pub held: BTreeMap<String, String>,
    /// Each slot it fills, and the alternative it is on (appendix D.3.1).
    pub slots: BTreeMap<String, String>,
    /// Its bindings to shared entities, by the kind bound, and for `per`, the
    /// element's number: `Carrier`, `Stock.0` (appendix F.2.5).
    pub binds: BTreeMap<String, Bind>,
    /// Its parts, every attempt kept, in the order they were created.
    pub parts: Vec<Node>,
    /// Its sourced attributes' fetches, by attribute (appendix I.2).
    pub fetched: BTreeMap<String, Fetch>,
    /// For each slot, the alternatives tried since it was last filled afresh
    /// (appendix K.2.3), the one it is on last.
    pub tried: BTreeMap<String, Vec<String>>,
    /// The choices put to a model and not yet answered, by request key:
    /// the options, in `prefer` order (appendix K.2.3).
    pub choosing: BTreeMap<String, Vec<String>>,
}

impl Node {
    fn to_value(&self) -> Value {
        Value::tuple([
            text(&self.kind),
            attrs_value(&self.attrs),
            Value::from(self.created),
            Value::from(self.status.code()),
            text(self.outcome.as_deref().unwrap_or("")),
            Value::from(i64::from(self.attempt)),
            Value::from(i64::from(self.sends)),
            text(self.waiting.as_deref().unwrap_or("")),
            Value::list(self.held.iter().map(|(part, rule)| Value::tuple([text(part), text(rule)]))),
            Value::list(self.slots.iter().map(|(slot, alt)| Value::tuple([text(slot), text(alt)]))),
            Value::list(self.binds.iter().map(|(id, b)| Value::tuple([text(id), b.to_value()]))),
            Value::list(self.parts.iter().map(Node::to_value)),
            Value::list(self.fetched.iter().map(|(a, f)| Value::tuple([text(a), f.to_value()]))),
            lists_value(&self.tried),
            lists_value(&self.choosing),
        ])
    }

    fn from_value(v: &Value) -> Result<Node, String> {
        let [kind, attrs, created, status, outcome, attempt, sends, waiting, held, slots, binds, parts, fetched, tried, choosing] =
            fields::<15>(v, "node")?;
        let mut fetches = BTreeMap::new();
        for f in items(fetched)? {
            let [a, f] = fields::<2>(f, "fetched attribute")?;
            fetches.insert(string(a)?, Fetch::from_value(f)?);
        }
        let mut bound = BTreeMap::new();
        for b in items(binds)? {
            let [id, b] = fields::<2>(b, "binding")?;
            bound.insert(string(id)?, Bind::from_value(b)?);
        }
        let mut held_by = BTreeMap::new();
        for h in items(held)? {
            let [part, rule] = fields::<2>(h, "held need")?;
            held_by.insert(string(part)?, string(rule)?);
        }
        let mut slot_alts = BTreeMap::new();
        for x in items(slots)? {
            let [slot, alt] = fields::<2>(x, "slot")?;
            slot_alts.insert(string(slot)?, string(alt)?);
        }
        let some = |s: String| if s.is_empty() { None } else { Some(s) };
        Ok(Node {
            kind: string(kind)?,
            attrs: attrs_from(attrs)?,
            created: int(created)?,
            status: Status::from_code(int(status)?)?,
            outcome: some(string(outcome)?),
            attempt: u32::try_from(int(attempt)?).map_err(|e| e.to_string())?,
            sends: u32::try_from(int(sends)?).map_err(|e| e.to_string())?,
            waiting: some(string(waiting)?),
            held: held_by,
            slots: slot_alts,
            binds: bound,
            parts: items(parts)?.iter().map(Node::from_value).collect::<Result<_, _>>()?,
            fetched: fetches,
            tried: lists_from(tried)?,
            choosing: lists_from(choosing)?,
        })
    }
}

fn lists_value(m: &BTreeMap<String, Vec<String>>) -> Value {
    Value::list(m.iter().map(|(k, xs)| Value::tuple([text(k), Value::list(xs.iter().map(|x| text(x)))])))
}

fn lists_from(v: &Value) -> Result<BTreeMap<String, Vec<String>>, String> {
    let mut out = BTreeMap::new();
    for x in items(v)? {
        let [k, xs] = fields::<2>(x, "named list")?;
        out.insert(string(k)?, items(xs)?.iter().map(string).collect::<Result<_, _>>()?);
    }
    Ok(out)
}

/// A root entity and everything it owns: one kernel agent (appendix B.3.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unit {
    pub label: String,
    pub root: Node,
    /// What the last step did, for the trace.
    pub notes: Vec<String>,
    /// The goals already reported as met, by index, so each is reported once.
    pub met: Vec<u32>,
    /// The goals with a deadline whose subject this unit has matched, by
    /// index, in the order it matched them; the host starts their timers.
    pub matched: Vec<u32>,
    /// The goals that raised an alert, by index (appendix C.3.4).
    pub alerts: Vec<u32>,
    /// The timers this unit asked for: an id and a duration. The host starts
    /// each id once. A slot's patience (appendix D.3.5) fires a `Patience`;
    /// a source's id has a `~` and fires a `Due` (appendix I.2.5–I.2.6).
    pub timers: Vec<(String, u64)>,
    /// The shared entities it reads, carried in (appendix F.2.3).
    pub facts: Facts,
    /// The running totals of what other units took from it, by name (appendix F.2.8).
    pub taken: BTreeMap<String, Val>,
    /// The version of the program it was created under (appendix M.2.2).
    pub version: u32,
}

/// Work done on this thread, counted rather than timed, so tests can check
/// that the cost of an input does not grow with the number of units (spec
/// appendix BA). Per thread, so tests that run side by side do not count
/// each other's work.
#[derive(Default)]
pub struct Work {
    /// Units read out of the kernel.
    pub decoded: Cell<u64>,
    /// Steps the interpreter took.
    pub steps: Cell<u64>,
}

thread_local! {
    pub static WORK: Work = Work::default();
}

impl Work {
    /// (units read, steps taken) so far on this thread.
    pub fn now() -> (u64, u64) {
        WORK.with(|w| (w.decoded.get(), w.steps.get()))
    }

    pub(crate) fn step() {
        WORK.with(|w| w.steps.set(w.steps.get() + 1));
    }
}

impl Unit {
    pub fn to_value(&self) -> Value {
        Value::tuple([
            text(&self.label),
            self.root.to_value(),
            Value::list(self.notes.iter().map(|n| text(n))),
            indexes(&self.met),
            indexes(&self.matched),
            indexes(&self.alerts),
            Value::list(self.timers.iter().map(|(id, secs)| Value::tuple([text(id), Value::from(*secs as i64)]))),
            facts_value(&self.facts),
            attrs_value(&self.taken),
            Value::from(i64::from(self.version)),
        ])
    }

    pub fn from_value(v: &Value) -> Result<Unit, String> {
        WORK.with(|w| w.decoded.set(w.decoded.get() + 1));
        let [label, root, notes, met, matched, alerts, timers, facts, taken, version] = fields::<10>(v, "unit")?;
        let mut wanted = vec![];
        for t in items(timers)? {
            let [id, secs] = fields::<2>(t, "timer")?;
            wanted.push((string(id)?, u64::try_from(int(secs)?).map_err(|e| e.to_string())?));
        }
        Ok(Unit {
            label: string(label)?,
            root: Node::from_value(root)?,
            notes: items(notes)?.iter().map(string).collect::<Result<_, _>>()?,
            met: indexes_from(met)?,
            matched: indexes_from(matched)?,
            alerts: indexes_from(alerts)?,
            timers: wanted,
            facts: facts_from(facts)?,
            taken: attrs_from(taken)?,
            version: u32::try_from(int(version)?).map_err(|e| e.to_string())?,
        })
    }
}

/// What arrives at a unit's mailbox.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Msg {
    /// Settle a unit that was just created.
    Create,
    /// The outside world's answer to the request with this key.
    Answer { key: String, outcome: String, data: Vec<(String, Val)> },
    /// A goal's deadline has come (appendix C.3.4).
    GoalDue { goal: u32 },
    /// A slot's patience ran out (appendix D.3.3).
    Patience { id: String },
    /// To a shared entity the choice picked: take this amount for the
    /// binding `key` of the unit `from` (appendix F.2.8).
    Take { from: String, key: String, taken: Option<(String, Val)> },
    /// The binding `key` is granted by `by`, with `by`'s facts.
    Granted { key: String, by: String, taken: Option<(String, Val)>, facts: Facts },
    /// `by` refused the binding `key`: one of its rules would break.
    Refused { key: String, by: String, rule: String },
    /// No candidate is left for the binding `key`; it waits (appendix F.2.9).
    Waiting { key: String },
    /// The binding `key` of `from` gives back what it took (appendix F.2.10).
    Release { from: String, key: String, taken: Option<(String, Val)> },
    /// An admitted event from a caller (`customer Ada`), by its verb as
    /// written (`cancels`), for the reactions to it (appendix G.2.4).
    /// `by` is `role name`; `data` is what the event carries, and `facts`
    /// the caller's carried facts, for its reactions (appendix Q.2.3).
    /// `part` is the kind of the part it goes to, empty for the root (appendix R.2.2).
    Event { verb: String, by: String, part: String, data: Vec<(String, Val)>, facts: Facts },
    /// An admitted change from a caller: set the root's attribute (appendix G.2.5).
    Change { attr: String, value: Val, by: String },
    /// New values of the facts a reader carries (appendix G.2.7).
    Refresh { facts: Facts },
    /// A source's timer: a value's `good for` ran out, or a failed fetch
    /// may ask again (appendix I.2.5–I.2.6).
    Due { id: String },
    /// The program was deployed in a new version: for each old goal, by
    /// index, its index in the new one, if it is still there (appendix M.2.5).
    /// `facts`: the shared entities it reads, as carried over too.
    Upgrade { goals: Vec<Option<u32>>, facts: Facts },
}

impl Msg {
    pub fn to_value(&self) -> Value {
        match self {
            Msg::Create => Value::tuple([Value::from(0i64)]),
            Msg::Answer { key, outcome, data } => {
                Value::tuple([Value::from(1i64), text(key), text(outcome), pairs_value(data)])
            }
            Msg::GoalDue { goal } => Value::tuple([Value::from(2i64), Value::from(i64::from(*goal))]),
            Msg::Patience { id } => Value::tuple([Value::from(3i64), text(id)]),
            Msg::Take { from, key, taken } => Value::tuple([Value::from(4i64), text(from), text(key), taken_value(taken)]),
            Msg::Granted { key, by, taken, facts } => {
                Value::tuple([Value::from(5i64), text(key), text(by), taken_value(taken), facts_value(facts)])
            }
            Msg::Refused { key, by, rule } => Value::tuple([Value::from(6i64), text(key), text(by), text(rule)]),
            Msg::Waiting { key } => Value::tuple([Value::from(7i64), text(key)]),
            Msg::Release { from, key, taken } => Value::tuple([Value::from(8i64), text(from), text(key), taken_value(taken)]),
            Msg::Event { verb, by, part, data, facts } => {
                Value::tuple([Value::from(9i64), text(verb), text(by), text(part), pairs_value(data), facts_value(facts)])
            }
            Msg::Change { attr, value, by } => Value::tuple([Value::from(10i64), pairs_value(&[(attr.clone(), value.clone())]), text(by)]),
            Msg::Refresh { facts } => Value::tuple([Value::from(11i64), facts_value(facts)]),
            Msg::Due { id } => Value::tuple([Value::from(12i64), text(id)]),
            Msg::Upgrade { goals, facts } => {
                Value::tuple([Value::from(13i64), Value::list(goals.iter().map(|g| Value::from(g.map_or(-1, i64::from)))), facts_value(facts)])
            }
        }
    }

    pub fn from_value(v: &Value) -> Result<Msg, String> {
        match items(v)?.first().map(int) {
            Some(Ok(0)) => Ok(Msg::Create),
            Some(Ok(1)) => {
                let [_, key, outcome, data] = fields::<4>(v, "answer message")?;
                Ok(Msg::Answer { key: string(key)?, outcome: string(outcome)?, data: pairs_from(data)? })
            }
            Some(Ok(2)) => {
                let [_, goal] = fields::<2>(v, "goal message")?;
                Ok(Msg::GoalDue { goal: u32::try_from(int(goal)?).map_err(|e| e.to_string())? })
            }
            Some(Ok(3)) => {
                let [_, id] = fields::<2>(v, "patience message")?;
                Ok(Msg::Patience { id: string(id)? })
            }
            Some(Ok(4)) => {
                let [_, from, key, taken] = fields::<4>(v, "take message")?;
                Ok(Msg::Take { from: string(from)?, key: string(key)?, taken: taken_from(taken)? })
            }
            Some(Ok(5)) => {
                let [_, key, by, taken, facts] = fields::<5>(v, "granted message")?;
                Ok(Msg::Granted { key: string(key)?, by: string(by)?, taken: taken_from(taken)?, facts: facts_from(facts)? })
            }
            Some(Ok(6)) => {
                let [_, key, by, rule] = fields::<4>(v, "refused message")?;
                Ok(Msg::Refused { key: string(key)?, by: string(by)?, rule: string(rule)? })
            }
            Some(Ok(7)) => {
                let [_, key] = fields::<2>(v, "waiting message")?;
                Ok(Msg::Waiting { key: string(key)? })
            }
            Some(Ok(8)) => {
                let [_, from, key, taken] = fields::<4>(v, "release message")?;
                Ok(Msg::Release { from: string(from)?, key: string(key)?, taken: taken_from(taken)? })
            }
            Some(Ok(9)) => {
                let [_, verb, by, part, data, facts] = fields::<6>(v, "event message")?;
                Ok(Msg::Event { verb: string(verb)?, by: string(by)?, part: string(part)?, data: pairs_from(data)?, facts: facts_from(facts)? })
            }
            Some(Ok(10)) => {
                let [_, change, by] = fields::<3>(v, "change message")?;
                let Ok([(attr, value)]) = <[(String, Val); 1]>::try_from(pairs_from(change)?) else {
                    return Err(format!("a change message sets one attribute: {v}"));
                };
                Ok(Msg::Change { attr, value, by: string(by)? })
            }
            Some(Ok(11)) => {
                let [_, facts] = fields::<2>(v, "refresh message")?;
                Ok(Msg::Refresh { facts: facts_from(facts)? })
            }
            Some(Ok(12)) => {
                let [_, id] = fields::<2>(v, "due message")?;
                Ok(Msg::Due { id: string(id)? })
            }
            Some(Ok(13)) => {
                let [_, goals, facts] = fields::<3>(v, "upgrade message")?;
                let goals = items(goals)?.iter().map(|g| int(g).map(|i| u32::try_from(i).ok())).collect::<Result<_, _>>()?;
                Ok(Msg::Upgrade { goals, facts: facts_from(facts)? })
            }
            _ => Err(format!("not a message: {v}")),
        }
    }
}

/// A request to the outside world, carried by a kernel effect.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ask {
    pub adapter: String,
    pub verb: String,
    pub kind: String,
    /// The unit the answer goes back to.
    pub label: String,
    /// The idempotency key: the unit, then each part down to this one with its attempt.
    pub key: String,
    pub sends: Vec<(String, Val)>,
}

impl Ask {
    pub fn to_value(&self) -> Value {
        Value::tuple([
            text(&self.adapter),
            text(&self.verb),
            text(&self.kind),
            text(&self.label),
            text(&self.key),
            pairs_value(&self.sends),
        ])
    }

    pub fn from_value(v: &Value) -> Result<Ask, String> {
        let [adapter, verb, kind, label, key, sends] = fields::<6>(v, "request")?;
        Ok(Ask {
            adapter: string(adapter)?,
            verb: string(verb)?,
            kind: string(kind)?,
            label: string(label)?,
            key: string(key)?,
            sends: pairs_from(sends)?,
        })
    }
}

/// A binding asking for a choice (appendix F.2.7): a kernel effect whose hole
/// the host fills with the pick. `node` is the part's key, `bind` its
/// binding's id; `binding` indexes the program's bindings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decide {
    pub label: String,
    pub node: String,
    pub bind: String,
    pub binding: u32,
    /// For `per`: which element, counting from 0.
    pub elem: Option<u32>,
    pub taken: Option<(String, Val)>,
    pub skip: Vec<String>,
}

impl Decide {
    /// The binding's key across units: `O1/Delivery#1/Shipment#1@Stock.0`.
    pub fn key(&self) -> String {
        format!("{}@{}", self.node, self.bind)
    }

    pub fn to_value(&self) -> Value {
        Value::tuple([
            text(&self.label),
            text(&self.node),
            text(&self.bind),
            Value::from(i64::from(self.binding)),
            Value::list(self.elem.iter().map(|&e| Value::from(i64::from(e)))),
            taken_value(&self.taken),
            Value::list(self.skip.iter().map(|l| text(l))),
        ])
    }

    pub fn from_value(v: &Value) -> Result<Decide, String> {
        let [label, node, bind, binding, elem, taken, skip] = fields::<7>(v, "choice request")?;
        let small = |x: &Value| int(x).and_then(|i| u32::try_from(i).map_err(|e| e.to_string()));
        Ok(Decide {
            label: string(label)?,
            node: string(node)?,
            bind: string(bind)?,
            binding: small(binding)?,
            elem: items(elem)?.first().map(small).transpose()?,
            taken: taken_from(taken)?,
            skip: items(skip)?.iter().map(string).collect::<Result<_, _>>()?,
        })
    }
}

/// An adapter's answer, as the host injects it at the request's effect hole.
pub fn answer_value(outcome: &str, data: &[(String, Val)]) -> Value {
    Value::tuple([text(outcome), pairs_value(data)])
}

pub fn answer_from_value(v: &Value) -> Result<(String, Vec<(String, Val)>), String> {
    let [outcome, data] = fields::<2>(v, "answer")?;
    Ok((string(outcome)?, pairs_from(data)?))
}

/// A message as it waits at a mailbox: with the clock when it was delivered,
/// which the step reads as its `now` (appendix F.2.14).
pub fn stamped(now: i64, msg: &Value) -> Value {
    Value::tuple([Value::from(now), msg.clone()])
}

pub fn unstamp(v: &Value) -> Result<(i64, Msg), String> {
    let [now, msg] = fields::<2>(v, "delivered message")?;
    Ok((int(now)?, Msg::from_value(msg)?))
}

/// An internal message on its way to the unit labeled `label`.
pub fn send_value(label: &str, msg: &Msg) -> Value {
    Value::tuple([text(label), msg.to_value()])
}

pub fn send_from_value(v: &Value) -> Result<(String, Value), String> {
    let [label, msg] = fields::<2>(v, "send")?;
    Ok((string(label)?, msg.clone()))
}

fn pairs_value(pairs: &[(String, Val)]) -> Value {
    Value::list(pairs.iter().map(|(k, v)| Value::tuple([text(k), v.to_value()])))
}

fn pairs_from(v: &Value) -> Result<Vec<(String, Val)>, String> {
    items(v)?
        .iter()
        .map(|p| {
            let [k, x] = fields::<2>(p, "pair")?;
            Ok((string(k)?, Val::from_value(x)?))
        })
        .collect()
}

fn indexes(v: &[u32]) -> Value {
    Value::list(v.iter().map(|&i| Value::from(i64::from(i))))
}

fn indexes_from(v: &Value) -> Result<Vec<u32>, String> {
    items(v)?.iter().map(|m| int(m).and_then(|i| u32::try_from(i).map_err(|e| e.to_string()))).collect()
}

fn text(s: &str) -> Value {
    Value::from(s)
}

fn string(v: &Value) -> Result<String, String> {
    v.as_str().map(str::to_string).ok_or_else(|| format!("not text: {v}"))
}

fn int(v: &Value) -> Result<i64, String> {
    v.as_int().ok_or_else(|| format!("not a number: {v}"))
}

fn items(v: &Value) -> Result<&[Value], String> {
    v.items().ok_or_else(|| format!("not a list: {v}"))
}

fn fields<'a, const N: usize>(v: &'a Value, what: &str) -> Result<[&'a Value; N], String> {
    let xs = v.items().filter(|xs| xs.len() == N).ok_or_else(|| format!("not a {what}: {v}"))?;
    Ok(std::array::from_fn(|i| &xs[i]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit() -> Unit {
        let mut attrs = BTreeMap::new();
        attrs.insert("country".to_string(), Val::Text("France".into()));
        attrs.insert("total".to_string(), Val::Money(12000));
        let part = Node {
            kind: "Payment".into(),
            attrs: BTreeMap::new(),
            created: 30,
            status: Status::Requested,
            outcome: None,
            attempt: 1,
            sends: 1,
            waiting: None,
            held: BTreeMap::new(),
            slots: BTreeMap::new(),
            binds: BTreeMap::from([
                ("Carrier".to_string(), Bind::Granted { to: "K1".into(), taken: None }),
                ("Stock.0".to_string(), Bind::Waiting { skip: vec!["S1".into()] }),
                ("Stock.1".to_string(), Bind::Granted { to: "S2".into(), taken: Some(("quantity".into(), Val::Count(2))) }),
            ]),
            parts: vec![],
            fetched: BTreeMap::from([(
                "credit".to_string(),
                Fetch { n: 2, sends: 1, at: Some(10), until: None, failed: false, fallback: true, via: "bureau".into(), tried: vec!["backup".into()] },
            )]),
            tried: BTreeMap::from([("Delivery".to_string(), vec!["Shipment".to_string()])]),
            choosing: BTreeMap::from([("O1?Delivery#2".to_string(), vec!["Refund".to_string()])]),
        };
        let root = Node {
            kind: "Order".into(),
            attrs,
            created: 0,
            status: Status::Idle,
            outcome: None,
            attempt: 1,
            sends: 1,
            waiting: Some("hold".into()),
            held: BTreeMap::from([("Shipment".to_string(), "paid".to_string())]),
            slots: BTreeMap::from([("Delivery".to_string(), "Shipment".to_string())]),
            binds: BTreeMap::new(),
            parts: vec![part],
            fetched: BTreeMap::new(),
            tried: BTreeMap::new(),
            choosing: BTreeMap::new(),
        };
        Unit {
            label: "O1".into(),
            root,
            notes: vec!["placed".into()],
            met: vec![0],
            matched: vec![0],
            alerts: vec![0],
            timers: vec![("O1/Delivery:Shipment".into(), 172_800)],
            facts: Facts::from([(
                "DE".to_string(),
                Fact {
                    kind: "Country".into(),
                    created: 0,
                    attrs: BTreeMap::from([("countries".to_string(), Val::Set(vec!["A".into(), "B".into()]))]),
                    taken: BTreeMap::from([("number of Items".to_string(), Val::Count(2))]),
                },
            )]),
            taken: BTreeMap::from([("quantity".to_string(), Val::Count(3))]),
            version: 2,
        }
    }

    #[test]
    fn units_messages_and_requests_round_trip() {
        let u = unit();
        assert_eq!(Unit::from_value(&u.to_value()).unwrap(), u);
        let m = Msg::Answer { key: "O1/Payment#1".into(), outcome: "captured".into(), data: vec![("value".into(), Val::Money(100))] };
        assert_eq!(Msg::from_value(&m.to_value()).unwrap(), m);
        assert_eq!(Msg::from_value(&Msg::Create.to_value()).unwrap(), Msg::Create);
        assert_eq!(Msg::from_value(&Msg::GoalDue { goal: 2 }.to_value()).unwrap(), Msg::GoalDue { goal: 2 });
        let patience = Msg::Patience { id: "O1/Delivery:Shipment".into() };
        assert_eq!(Msg::from_value(&patience.to_value()).unwrap(), patience);
        let a = Ask {
            adapter: "payments".into(),
            verb: "capture".into(),
            kind: "Payment".into(),
            label: "O1".into(),
            key: "O1/Payment#1".into(),
            sends: vec![("total".into(), Val::Money(12000))],
        };
        assert_eq!(Ask::from_value(&a.to_value()).unwrap(), a);
        let taken = Some(("quantity".to_string(), Val::Count(2)));
        for m in [
            Msg::Take { from: "O1".into(), key: "O1/Shipment#1@Stock.0".into(), taken: taken.clone() },
            Msg::Granted { key: "k".into(), by: "S1".into(), taken: None, facts: u.facts.clone() },
            Msg::Refused { key: "k".into(), by: "S1".into(), rule: "stock".into() },
            Msg::Waiting { key: "k".into() },
            Msg::Release { from: "O1".into(), key: "k".into(), taken: taken.clone() },
            Msg::Event { verb: "cancels".into(), by: "customer Ada".into(), part: "Payment".into(), data: vec![("reason".into(), Val::Text("late".into()))], facts: u.facts.clone() },
            Msg::Change { attr: "units".into(), value: Val::Count(2), by: "staff Tom".into() },
            Msg::Refresh { facts: u.facts.clone() },
            Msg::Due { id: "O1~credit#1:stale".into() },
        ] {
            assert_eq!(Msg::from_value(&m.to_value()).unwrap(), m);
        }
        let d = Decide { label: "O1".into(), node: "O1/Shipment#1".into(), bind: "Stock.0".into(), binding: 1, elem: Some(0), taken, skip: vec!["S1".into()] };
        assert_eq!(Decide::from_value(&d.to_value()).unwrap(), d);
        assert_eq!(d.key(), "O1/Shipment#1@Stock.0");
        let t = 16 * 86_400 + 3_600; // 2026-01-17 01:00, a Saturday
        assert_eq!(period_key("day", t), "2026-01-17");
        assert_eq!(period_key("week", t), "week of 2026-01-12");
        assert_eq!(period_key("month", t), "2026-01");
        assert_eq!(period_key("year", t), "2026");
        assert_eq!(period_key("week", 0), "week of 2025-12-29");
        let (label, msg) = send_from_value(&send_value("O1", &m)).unwrap();
        assert_eq!((label.as_str(), Msg::from_value(&msg).unwrap()), ("O1", m));
        assert!(Unit::from_value(&Value::from(3i64)).is_err());
    }

    #[test]
    fn values_show_as_people_read_them() {
        assert_eq!(Val::Money(12000).show("EUR"), "120 EUR");
        assert_eq!(Val::Money(1250).show("EUR"), "12.50 EUR");
        assert_eq!(Val::Text("alcohol".into()).show("EUR"), "\"alcohol\"");
        assert_eq!(Val::Bool(false).show("EUR"), "no");
        assert_eq!(Val::Time(86_400 * 31 + 3_660).show("EUR"), "2026-02-01 01:01");
        assert_eq!(Val::Duration(172_800).show("EUR"), "2 days");
        assert_eq!(Val::Set(vec!["DE".into(), "FR".into()]).show("EUR"), "{DE, FR}");
        assert_eq!(civil(EPOCH_DAYS), (2026, 1, 1));
    }

    #[test]
    fn a_unit_fits_well_under_the_payload_limit() {
        let mut u = unit();
        // The slice-5 fields carry what a unit shares; a plain order stays small.
        u.facts.clear();
        u.root.parts[0].binds.clear();
        assert!(u.to_value().encoded_len() < 1024);
        assert!(unit().to_value().encoded_len() < 2048);
    }
}
