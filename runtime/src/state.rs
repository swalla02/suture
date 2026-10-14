//! The runtime's data: values, the nodes of a unit, messages and outside
//! requests, and how each is carried in kernel payloads.

use std::collections::BTreeMap;

use suture::Value;
use suture_lang::ir::{Lit, Ty};

/// An attribute's value. Money is in cents.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Val {
    Text(String),
    Bool(bool),
    Money(i64),
    Count(i64),
}

impl Val {
    pub fn from_lit(l: &Lit) -> Val {
        match l {
            Lit::Text(s) => Val::Text(s.clone()),
            Lit::Bool(b) => Val::Bool(*b),
            Lit::Money(c) => Val::Money(*c),
            Lit::Count(n) => Val::Count(*n),
        }
    }

    pub fn ty(&self) -> Ty {
        match self {
            Val::Text(_) => Ty::Text,
            Val::Bool(_) => Ty::Bool,
            Val::Money(_) => Ty::Money,
            Val::Count(_) => Ty::Count,
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
        }
    }

    fn to_value(&self) -> Value {
        match self {
            Val::Text(s) => Value::tuple([Value::from(0i64), text(s)]),
            Val::Bool(b) => Value::tuple([Value::from(1i64), Value::from(*b)]),
            Val::Money(c) => Value::tuple([Value::from(2i64), Value::from(*c)]),
            Val::Count(n) => Value::tuple([Value::from(3i64), Value::from(*n)]),
        }
    }

    fn from_value(v: &Value) -> Result<Val, String> {
        let [tag, x] = fields::<2>(v, "value")?;
        Ok(match int(tag)? {
            0 => Val::Text(string(x)?),
            1 => Val::Bool(x.as_bool().ok_or_else(|| format!("not a yes or no: {x}"))?),
            2 => Val::Money(int(x)?),
            3 => Val::Count(int(x)?),
            t => return Err(format!("unknown value tag {t}")),
        })
    }
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

/// One entity of a unit: its root, or a part (spec appendix B.3.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    pub kind: String,
    pub attrs: BTreeMap<String, Val>,
    pub status: Status,
    /// What the outside world reported, once it has.
    pub outcome: Option<String>,
    /// Which attempt of its need this part is, from 1.
    pub attempt: u32,
    /// How many times its request has gone out: 0 until it is requested, at
    /// most `ATTEMPTS` (appendix C.3.3).
    pub sends: u32,
    /// The rule its request is waiting on, if any. A part with no effect uses
    /// it for the rule that keeps it from being done.
    pub waiting: Option<String>,
    /// The needs whose next part waits on a rule: the part's kind, and the rule.
    pub held: BTreeMap<String, String>,
    /// Its parts, every attempt kept, in the order they were created.
    pub parts: Vec<Node>,
}

impl Node {
    fn to_value(&self) -> Value {
        Value::tuple([
            text(&self.kind),
            Value::list(self.attrs.iter().map(|(k, v)| Value::tuple([text(k), v.to_value()]))),
            Value::from(self.status.code()),
            text(self.outcome.as_deref().unwrap_or("")),
            Value::from(i64::from(self.attempt)),
            Value::from(i64::from(self.sends)),
            text(self.waiting.as_deref().unwrap_or("")),
            Value::list(self.held.iter().map(|(part, rule)| Value::tuple([text(part), text(rule)]))),
            Value::list(self.parts.iter().map(Node::to_value)),
        ])
    }

    fn from_value(v: &Value) -> Result<Node, String> {
        let [kind, attrs, status, outcome, attempt, sends, waiting, held, parts] = fields::<9>(v, "node")?;
        let mut map = BTreeMap::new();
        for a in items(attrs)? {
            let [k, x] = fields::<2>(a, "attribute")?;
            map.insert(string(k)?, Val::from_value(x)?);
        }
        let mut held_by = BTreeMap::new();
        for h in items(held)? {
            let [part, rule] = fields::<2>(h, "held need")?;
            held_by.insert(string(part)?, string(rule)?);
        }
        let some = |s: String| if s.is_empty() { None } else { Some(s) };
        Ok(Node {
            kind: string(kind)?,
            attrs: map,
            status: Status::from_code(int(status)?)?,
            outcome: some(string(outcome)?),
            attempt: u32::try_from(int(attempt)?).map_err(|e| e.to_string())?,
            sends: u32::try_from(int(sends)?).map_err(|e| e.to_string())?,
            waiting: some(string(waiting)?),
            held: held_by,
            parts: items(parts)?.iter().map(Node::from_value).collect::<Result<_, _>>()?,
        })
    }
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
        ])
    }

    pub fn from_value(v: &Value) -> Result<Unit, String> {
        let [label, root, notes, met, matched, alerts] = fields::<6>(v, "unit")?;
        Ok(Unit {
            label: string(label)?,
            root: Node::from_value(root)?,
            notes: items(notes)?.iter().map(string).collect::<Result<_, _>>()?,
            met: indexes_from(met)?,
            matched: indexes_from(matched)?,
            alerts: indexes_from(alerts)?,
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
}

impl Msg {
    pub fn to_value(&self) -> Value {
        match self {
            Msg::Create => Value::tuple([Value::from(0i64)]),
            Msg::Answer { key, outcome, data } => {
                Value::tuple([Value::from(1i64), text(key), text(outcome), pairs_value(data)])
            }
            Msg::GoalDue { goal } => Value::tuple([Value::from(2i64), Value::from(i64::from(*goal))]),
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

/// An adapter's answer, as the host injects it at the request's effect hole.
pub fn answer_value(outcome: &str, data: &[(String, Val)]) -> Value {
    Value::tuple([text(outcome), pairs_value(data)])
}

pub fn answer_from_value(v: &Value) -> Result<(String, Vec<(String, Val)>), String> {
    let [outcome, data] = fields::<2>(v, "answer")?;
    Ok((string(outcome)?, pairs_from(data)?))
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
            status: Status::Requested,
            outcome: None,
            attempt: 1,
            sends: 1,
            waiting: None,
            held: BTreeMap::new(),
            parts: vec![],
        };
        let root = Node {
            kind: "Order".into(),
            attrs,
            status: Status::Idle,
            outcome: None,
            attempt: 1,
            sends: 1,
            waiting: Some("hold".into()),
            held: BTreeMap::from([("Shipment".to_string(), "paid".to_string())]),
            parts: vec![part],
        };
        Unit { label: "O1".into(), root, notes: vec!["placed".into()], met: vec![0], matched: vec![0], alerts: vec![0] }
    }

    #[test]
    fn units_messages_and_requests_round_trip() {
        let u = unit();
        assert_eq!(Unit::from_value(&u.to_value()).unwrap(), u);
        let m = Msg::Answer { key: "O1/Payment#1".into(), outcome: "captured".into(), data: vec![("value".into(), Val::Money(100))] };
        assert_eq!(Msg::from_value(&m.to_value()).unwrap(), m);
        assert_eq!(Msg::from_value(&Msg::Create.to_value()).unwrap(), Msg::Create);
        assert_eq!(Msg::from_value(&Msg::GoalDue { goal: 2 }.to_value()).unwrap(), Msg::GoalDue { goal: 2 });
        let a = Ask {
            adapter: "payments".into(),
            verb: "capture".into(),
            kind: "Payment".into(),
            label: "O1".into(),
            key: "O1/Payment#1".into(),
            sends: vec![("total".into(), Val::Money(12000))],
        };
        assert_eq!(Ask::from_value(&a.to_value()).unwrap(), a);
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
    }

    #[test]
    fn a_unit_fits_well_under_the_payload_limit() {
        assert!(unit().to_value().encoded_len() < 1024);
    }
}
