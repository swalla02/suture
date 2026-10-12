//! The journal: an append-only record of boundary events and reductions.
//!
//! Records are buffered by `append` and made durable by `commit`. Because the
//! interior is deterministic, the graph is a function of the committed
//! records; recovery replays them and verifies every deterministic reduction
//! against its recorded digest.

use serde::{Deserialize, Serialize};

use crate::id::{AgentId, Digest, HoleId};
use crate::net::{NetData, Var};

/// Journal format version written in every header.
pub const FORMAT: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t")]
pub enum Record {
    Header { program: String, version: u32, format: u32 },
    Inject { seq: u64, key: Option<String>, mint: bool, net: NetData, binds: Vec<(Var, RecBind)> },
    Reduce { left: AgentId, right: AgentId, rule: String, version: u32, digest: Digest, patch: Option<NetData> },
    Fault { left: AgentId, right: AgentId, rule: Option<String>, error: String },
    Commit,
}

/// An inject binding with labels already resolved to hole identities.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecBind {
    Hole(HoleId),
    New { label: Option<String> },
}

#[derive(Debug, thiserror::Error)]
pub enum JournalError {
    #[error("journal I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("journal format: {0}")]
    Format(String),
}

pub trait Journal: Send {
    /// Buffers a record; it becomes durable at the next `commit`.
    fn append(&mut self, r: Record);
    /// Makes every buffered record durable, atomically.
    fn commit(&mut self) -> Result<(), JournalError>;
    /// The committed records, in order (commit markers included).
    fn durable(&self) -> Result<Vec<Record>, JournalError>;
    /// Simulates a crash: buffered, uncommitted records are lost.
    fn crash(&mut self);
    /// Number of committed records.
    fn len_durable(&self) -> usize;
}

/// An in-memory journal, used by default and for crash simulation.
#[derive(Default)]
pub struct MemJournal {
    durable: Vec<Record>,
    pending: Vec<Record>,
}

impl MemJournal {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Journal for MemJournal {
    fn append(&mut self, r: Record) {
        self.pending.push(r);
    }

    fn commit(&mut self) -> Result<(), JournalError> {
        if !self.pending.is_empty() {
            self.durable.append(&mut self.pending);
            self.durable.push(Record::Commit);
        }
        Ok(())
    }

    fn durable(&self) -> Result<Vec<Record>, JournalError> {
        Ok(self.durable.clone())
    }

    fn crash(&mut self) {
        self.pending.clear();
    }

    fn len_durable(&self) -> usize {
        self.durable.len()
    }
}
