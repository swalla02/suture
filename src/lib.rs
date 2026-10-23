//! Suture: a typed, deterministic interaction-net kernel.
//!
//! The running program is a port graph; the only internal computation is
//! `INTERACT`, the atomic replacement of an active pair by a validated patch.
//! See `docs/ir-spec.md` for the formal model.

pub mod check;
pub mod dnc;
mod dot;
pub mod explain;
pub mod graph;
pub mod id;
pub mod journal;
pub mod machine;
pub mod net;
pub mod nets;
pub mod policy;
pub mod rng;
pub mod rule;
pub mod sig;
pub mod value;

pub use check::NetError;
pub use explain::{Explanation, Terminal, WaitGroup};
pub use graph::{Agent, Cause, Endpoint, Graph, GraphError, Hole, HoleOrigin};
pub use id::{AgentId, Digest, DigestBuilder, HoleId};
pub use journal::{FileJournal, Journal, JournalError, MemJournal, NoJournal, RecBind, Record, RecoveryError, UpgradeError};
pub use machine::{EffectView, Fault, Image, InjectError, InjectReceipt, Limits, Machine, Program, ProgramError, Rounds, RunOutcome, SchedulerKind, Stats};
pub use net::{Bind, HoleRef, Injection, NetAgent, NetBuilder, NetData, Slot, Var};
pub use policy::{PatchView, Policy};
pub use rng::Rng;
pub use rule::{Coverage, Rule, RuleCx, RuleError, RuleMode, RuleTable};
pub use sig::{KindDecl, KindId, Polarity, PortType, SigBuilder, SigError, Signature, TypeId, TypeMode};
pub use value::{Value, ValueError, ValueType};

/// Builds a [`Value::Tuple`] from expressions convertible into [`Value`].
#[macro_export]
macro_rules! tup {
    ($($x:expr),* $(,)?) => { $crate::Value::tuple([$($crate::Value::from($x)),*]) };
}

/// Builds a [`Value::List`] from expressions convertible into [`Value`].
#[macro_export]
macro_rules! vlist {
    ($($x:expr),* $(,)?) => { $crate::Value::list([$($crate::Value::from($x)),*]) };
}
