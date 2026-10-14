//! The journal: an append-only record of boundary events and reductions.
//!
//! Records are buffered by `append` and made durable by `commit`. Because the
//! interior is deterministic, the graph is a function of the committed
//! records; recovery replays them and verifies every deterministic reduction
//! against its recorded digest.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::id::{AgentId, Digest, HoleId};
use crate::net::{NetData, Var};

/// Journal format version written in every header. Format 2 adds `Version`.
pub const FORMAT: u32 = 2;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t")]
pub enum Record {
    Header { program: String, version: u32, format: u32 },
    /// The program that writes the records that follow: its name, version
    /// and fingerprint (`Program::fingerprint`).
    Version { program: String, version: u32, hash: Digest },
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

/// A JSON-lines journal file. `commit` appends the buffered records plus a
/// commit marker and fsyncs; `open` truncates anything after the last commit
/// marker (a torn or uncommitted tail).
pub struct FileJournal {
    path: PathBuf,
    file: File,
    buffer: Vec<String>,
    committed: usize,
    /// Byte length of the committed prefix; a failed commit is rolled back to it.
    committed_len: u64,
    /// Test hook: the next commit writes only this many bytes, then fails.
    #[cfg(test)]
    fail_after: Option<usize>,
}

/// Parses complete lines up to the last commit marker. Returns the records
/// and the byte length of the committed prefix.
fn committed_prefix(bytes: &[u8]) -> (Vec<Record>, usize) {
    let (mut records, mut pending) = (Vec::new(), Vec::new());
    let (mut offset, mut keep) = (0, 0);
    for line in bytes.split_inclusive(|&b| b == b'\n') {
        if !line.ends_with(b"\n") {
            break;
        }
        let Ok(rec) = serde_json::from_slice::<Record>(&line[..line.len() - 1]) else { break };
        offset += line.len();
        let is_commit = rec == Record::Commit;
        pending.push(rec);
        if is_commit {
            records.append(&mut pending);
            keep = offset;
        }
    }
    (records, keep)
}

impl FileJournal {
    pub fn open(path: &Path) -> Result<FileJournal, JournalError> {
        let mut file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path)?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        let (records, keep) = committed_prefix(&bytes);
        if keep != bytes.len() {
            file.set_len(keep as u64)?;
            file.sync_all()?;
        }
        file.seek(SeekFrom::End(0))?;
        Ok(FileJournal {
            path: path.to_path_buf(),
            file,
            buffer: vec![],
            committed: records.len(),
            committed_len: keep as u64,
            #[cfg(test)]
            fail_after: None,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl FileJournal {
    fn write_durably(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        #[cfg(test)]
        if let Some(n) = self.fail_after.take() {
            self.file.write_all(&bytes[..n.min(bytes.len())])?;
            return Err(std::io::Error::other("injected write failure"));
        }
        self.file.write_all(bytes)?;
        self.file.flush()?;
        self.file.sync_data()
    }
}

impl Journal for FileJournal {
    fn append(&mut self, r: Record) {
        self.buffer.push(serde_json::to_string(&r).expect("journal records always serialize"));
    }

    fn commit(&mut self) -> Result<(), JournalError> {
        if self.buffer.is_empty() {
            return Ok(());
        }
        let mut out = String::new();
        for line in &self.buffer {
            out.push_str(line);
            out.push('\n');
        }
        out.push_str(&serde_json::to_string(&Record::Commit).expect("commit marker serializes"));
        out.push('\n');
        if let Err(e) = self.write_durably(out.as_bytes()) {
            // Remove any torn bytes so later commits stay readable; the
            // records stay buffered and the next commit retries them.
            self.file.set_len(self.committed_len)?;
            self.file.seek(SeekFrom::End(0))?;
            return Err(e.into());
        }
        self.committed_len += out.len() as u64;
        self.committed += self.buffer.len() + 1;
        self.buffer.clear();
        Ok(())
    }

    fn durable(&self) -> Result<Vec<Record>, JournalError> {
        let mut bytes = Vec::new();
        File::open(&self.path)?.read_to_end(&mut bytes)?;
        Ok(committed_prefix(&bytes).0)
    }

    fn crash(&mut self) {
        self.buffer.clear();
    }

    fn len_durable(&self) -> usize {
        self.committed
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RecoveryError {
    #[error(transparent)]
    Journal(#[from] JournalError),
    #[error("journal header mismatch: {0}")]
    Header(String),
    #[error("journal was written by {journal}; the running program is {program}")]
    Version { journal: String, program: String },
    #[error("journal references unknown rule {0}")]
    UnknownRule(String),
    #[error("rule {rule} is version {program} but the journal recorded version {journal}")]
    RuleVersion { rule: String, journal: u32, program: u32 },
    #[error("replay diverged at record {step}: rule {rule} no longer produces the recorded patch")]
    Divergence { rule: String, step: usize },
    #[error("record {step} reduces a pair that is not active")]
    MissingPair { step: usize },
    #[error("replaying an inject failed: {0}")]
    Inject(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::AgentId;

    fn rec(error: &str) -> Record {
        Record::Fault { left: AgentId(1), right: AgentId(2), rule: None, error: error.into() }
    }

    fn errors(j: &FileJournal) -> Vec<String> {
        j.durable()
            .unwrap()
            .into_iter()
            .filter_map(|r| match r {
                Record::Fault { error, .. } => Some(error),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn failed_commit_is_rolled_back_and_retryable() {
        let path = std::env::temp_dir().join(format!("suture-journal-{}-rollback.jsonl", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let mut j = FileJournal::open(&path).unwrap();
        j.append(rec("first"));
        j.commit().unwrap();
        let good = std::fs::metadata(&path).unwrap().len();
        j.append(rec("second"));
        j.fail_after = Some(5);
        assert!(j.commit().is_err());
        assert_eq!(std::fs::metadata(&path).unwrap().len(), good, "torn bytes must be rolled back");
        j.append(rec("third"));
        j.commit().unwrap(); // retries "second" as well
        assert_eq!(errors(&FileJournal::open(&path).unwrap()), ["first", "second", "third"]);
        let _ = std::fs::remove_file(&path);
    }
}
