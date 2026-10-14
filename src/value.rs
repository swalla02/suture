//! Immutable payload values carried by agents, with schemas and a canonical
//! byte encoding used for digests.

use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

/// An immutable payload value. Values have no identity and are never mutated;
/// a rewrite that "changes" a payload creates a new agent with a new value.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Value {
    Unit,
    Bool(bool),
    Int(i64),
    Str(Arc<str>),
    Tuple(Arc<[Value]>),
    List(Arc<[Value]>),
}

/// A payload whose shape does not match what a rule or host expected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("payload {value}: {reason}")]
pub struct ValueError {
    pub value: String,
    pub reason: String,
}

impl Value {
    pub fn tuple(items: impl IntoIterator<Item = Value>) -> Value {
        Value::Tuple(items.into_iter().collect())
    }

    pub fn list(items: impl IntoIterator<Item = Value>) -> Value {
        Value::List(items.into_iter().collect())
    }

    pub fn as_int(&self) -> Option<i64> {
        match self {
            Value::Int(i) => Some(*i),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Elements of a tuple or list.
    pub fn items(&self) -> Option<&[Value]> {
        match self {
            Value::Tuple(xs) | Value::List(xs) => Some(xs),
            _ => None,
        }
    }

    fn error(&self, reason: impl Into<String>) -> ValueError {
        ValueError { value: self.to_string(), reason: reason.into() }
    }

    /// Field `i` of a tuple payload.
    pub fn get(&self, i: usize) -> Result<&Value, ValueError> {
        match self {
            Value::Tuple(xs) => xs.get(i).ok_or_else(|| self.error(format!("no field {i}"))),
            _ => Err(self.error("not a tuple")),
        }
    }

    pub fn int(&self, i: usize) -> Result<i64, ValueError> {
        self.get(i)?.as_int().ok_or_else(|| self.error(format!("field {i} is not an int")))
    }

    pub fn str(&self, i: usize) -> Result<&str, ValueError> {
        self.get(i)?.as_str().ok_or_else(|| self.error(format!("field {i} is not a string")))
    }

    pub fn list_at(&self, i: usize) -> Result<&[Value], ValueError> {
        match self.get(i)? {
            Value::List(xs) => Ok(xs),
            _ => Err(self.error(format!("field {i} is not a list"))),
        }
    }

    /// The length of [`Value::encode`]'s output, computed without encoding.
    pub fn encoded_len(&self) -> usize {
        match self {
            Value::Unit => 1,
            Value::Bool(_) => 2,
            Value::Int(_) => 9,
            Value::Str(s) => 9 + s.len(),
            Value::Tuple(xs) | Value::List(xs) => 9 + xs.iter().map(Value::encoded_len).sum::<usize>(),
        }
    }

    /// Appends a canonical, tagged, length-prefixed encoding of the value.
    /// Distinct values always have distinct encodings.
    pub fn encode(&self, out: &mut Vec<u8>) {
        match self {
            Value::Unit => out.push(0),
            Value::Bool(b) => {
                out.push(1);
                out.push(*b as u8);
            }
            Value::Int(i) => {
                out.push(2);
                out.extend_from_slice(&i.to_le_bytes());
            }
            Value::Str(s) => {
                out.push(3);
                out.extend_from_slice(&(s.len() as u64).to_le_bytes());
                out.extend_from_slice(s.as_bytes());
            }
            Value::Tuple(xs) | Value::List(xs) => {
                out.push(if matches!(self, Value::Tuple(_)) { 4 } else { 5 });
                out.extend_from_slice(&(xs.len() as u64).to_le_bytes());
                for x in xs.iter() {
                    x.encode(out);
                }
            }
        }
    }
}

impl From<()> for Value {
    fn from(_: ()) -> Value {
        Value::Unit
    }
}
impl From<bool> for Value {
    fn from(b: bool) -> Value {
        Value::Bool(b)
    }
}
impl From<i64> for Value {
    fn from(i: i64) -> Value {
        Value::Int(i)
    }
}
impl From<i32> for Value {
    fn from(i: i32) -> Value {
        Value::Int(i as i64)
    }
}
impl From<u32> for Value {
    fn from(i: u32) -> Value {
        Value::Int(i as i64)
    }
}
impl From<usize> for Value {
    fn from(i: usize) -> Value {
        Value::Int(i as i64)
    }
}
impl From<&str> for Value {
    fn from(s: &str) -> Value {
        Value::Str(s.into())
    }
}
impl From<String> for Value {
    fn from(s: String) -> Value {
        Value::Str(s.into())
    }
}
impl From<&String> for Value {
    fn from(s: &String) -> Value {
        Value::Str(s.as_str().into())
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fn seq(f: &mut fmt::Formatter<'_>, xs: &[Value]) -> fmt::Result {
            for (i, x) in xs.iter().enumerate() {
                if i > 0 {
                    f.write_str(", ")?;
                }
                write!(f, "{x}")?;
            }
            Ok(())
        }
        match self {
            Value::Unit => f.write_str("()"),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Int(i) => write!(f, "{i}"),
            Value::Str(s) => write!(f, "{s:?}"),
            Value::Tuple(xs) => {
                f.write_str("(")?;
                seq(f, xs)?;
                f.write_str(")")
            }
            Value::List(xs) => {
                f.write_str("[")?;
                seq(f, xs)?;
                f.write_str("]")
            }
        }
    }
}

/// The schema of a payload, declared per agent kind and checked on every
/// agent the machine creates.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ValueType {
    Any,
    Unit,
    Bool,
    Int,
    Str,
    Tuple(Vec<ValueType>),
    List(Box<ValueType>),
}

impl ValueType {
    pub fn admits(&self, v: &Value) -> bool {
        match (self, v) {
            (ValueType::Any, _) => true,
            (ValueType::Unit, Value::Unit) => true,
            (ValueType::Bool, Value::Bool(_)) => true,
            (ValueType::Int, Value::Int(_)) => true,
            (ValueType::Str, Value::Str(_)) => true,
            (ValueType::Tuple(ts), Value::Tuple(xs)) => {
                ts.len() == xs.len() && ts.iter().zip(xs.iter()).all(|(t, x)| t.admits(x))
            }
            (ValueType::List(t), Value::List(xs)) => xs.iter().all(|x| t.admits(x)),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{tup, vlist};

    #[test]
    fn schema_admits() {
        let t = ValueType::Tuple(vec![
            ValueType::Int,
            ValueType::Str,
            ValueType::List(Box::new(ValueType::Str)),
        ]);
        assert!(t.admits(&tup![1, "acme", vlist!["eu"]]));
        assert!(!t.admits(&tup![1, 2, vlist![]]));
        assert!(!t.admits(&tup![1, "acme"]));
        assert!(ValueType::Any.admits(&Value::Unit));
        assert!(ValueType::List(Box::new(ValueType::Int)).admits(&vlist![1, 2]));
        assert!(!ValueType::List(Box::new(ValueType::Int)).admits(&vlist![1, "x"]));
    }

    #[test]
    fn accessors() {
        let v = tup![7, "eu", vlist![1]];
        assert_eq!(v.int(0).unwrap(), 7);
        assert_eq!(v.str(1).unwrap(), "eu");
        assert_eq!(v.list_at(2).unwrap().len(), 1);
        assert!(v.int(1).is_err());
        assert!(v.int(9).is_err());
        assert!(Value::Int(3).int(0).is_err());
    }

    #[test]
    fn encoding_is_injective_on_shapes() {
        let enc = |v: &Value| {
            let mut b = vec![];
            v.encode(&mut b);
            b
        };
        let vals = [
            Value::Int(1),
            Value::from("1"),
            tup![1],
            vlist![1],
            Value::Bool(true),
            Value::Unit,
            tup![],
            vlist![],
        ];
        for i in 0..vals.len() {
            for j in 0..vals.len() {
                if i != j {
                    assert_ne!(enc(&vals[i]), enc(&vals[j]), "{} vs {}", vals[i], vals[j]);
                }
            }
        }
    }

    #[test]
    fn display_is_compact() {
        assert_eq!(tup![1, "eu", vlist![2, 3]].to_string(), r#"(1, "eu", [2, 3])"#);
        assert_eq!(Value::Unit.to_string(), "()");
    }
}

#[cfg(test)]
mod size_tests {
    use super::*;
    use crate::{tup, vlist};

    #[test]
    fn encoded_len_matches_encode() {
        let values = [
            Value::Unit,
            Value::Bool(true),
            Value::Int(-5),
            Value::Str("héllo".into()),
            tup![1, "ab", vlist![true, ()]],
            Value::list([]),
        ];
        for v in values {
            let mut buf = vec![];
            v.encode(&mut buf);
            assert_eq!(v.encoded_len(), buf.len(), "{v}");
        }
    }
}
