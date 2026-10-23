//! The media store (appendix V.2.2): images, sounds, videos and documents,
//! each kept once by the SHA-256 of its content. A unit holds only the
//! reference, so the store is never replayed: a decision model reads from
//! it, and its answer is journaled.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use sha2::{Digest, Sha256};

use crate::state::Val;

/// The most a piece of media may hold: 8 MiB.
pub const MAX_MEDIA_BYTES: usize = 8 * 1024 * 1024;

/// Media content with its type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Blob {
    pub mime: String,
    pub data: Arc<Vec<u8>>,
}

/// Where media is kept: in memory, or in a directory shared by the pods.
#[derive(Default)]
pub struct Store {
    mem: BTreeMap<String, Blob>,
    dir: Option<PathBuf>,
}

/// The most the store keeps in all, so uploads cannot fill the disk the
/// log is on (appendix BB.2.2).
pub const MAX_STORE_BYTES: u64 = 1024 * 1024 * 1024;

/// The SHA-256 of `data`, in lower-case hex.
pub fn sha256(data: &[u8]) -> String {
    Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

/// Whether `mime` is a media type such as `image/png`: a type and a
/// subtype, in lower-case letters, digits and `.+-`.
pub fn valid_mime(mime: &str) -> bool {
    let ok = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || ".+-".contains(c));
    mime.split_once('/').is_some_and(|(t, s)| ok(t) && ok(s)) && mime.len() <= 100
}

/// The media type of a file name's extension, for scenarios.
pub fn mime_of(name: &str) -> Option<&'static str> {
    let ext = name.rsplit_once('.')?.1.to_ascii_lowercase();
    Some(match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "pdf" => "application/pdf",
        "txt" => "text/plain",
        _ => return None,
    })
}

impl Store {
    /// A store in the directory `dir`, made if missing.
    pub fn in_dir(dir: PathBuf) -> Store {
        Store { mem: BTreeMap::new(), dir: Some(dir) }
    }

    /// Keeps `data` as `mime`, and gives the value that refers to it.
    pub fn put(&mut self, mime: &str, data: Vec<u8>) -> Result<Val, String> {
        if !valid_mime(mime) {
            return Err(format!("`{mime}` is not a media type such as image/png"));
        }
        if data.len() > MAX_MEDIA_BYTES {
            return Err(format!("the media is {} bytes; at most {MAX_MEDIA_BYTES} are kept", data.len()));
        }
        let hash = sha256(&data);
        let bytes = data.len() as u64;
        // The content is the identity: kept once, with the type it was first kept as.
        if let Some(kept) = self.get(&hash) {
            return Ok(Val::Media { sha256: hash, mime: kept.mime, bytes });
        }
        if self.used() + bytes > MAX_STORE_BYTES {
            return Err(format!("the media store is full: it keeps at most {MAX_STORE_BYTES} bytes"));
        }
        match &self.dir {
            Some(dir) => {
                let path = dir.join(&hash);
                // Written whole under a name of its own, then renamed, so another
                // pod never reads half of it, nor two writers share a file.
                std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
                static WRITES: AtomicU64 = AtomicU64::new(0);
                let tmp = dir.join(format!(".{hash}.{}.{}.tmp", std::process::id(), WRITES.fetch_add(1, Ordering::Relaxed)));
                let file = format!("{mime}\n");
                let write = || -> std::io::Result<()> {
                    use std::io::Write;
                    let mut f = std::fs::File::create(&tmp)?;
                    f.write_all(file.as_bytes())?;
                    f.write_all(&data)?;
                    f.sync_all()?;
                    std::fs::rename(&tmp, &path)
                };
                if let Err(e) = write() {
                    let _ = std::fs::remove_file(&tmp);
                    // Another writer may have kept the same content meanwhile.
                    let Some(kept) = self.get(&hash) else { return Err(format!("{}: {e}", path.display())) };
                    return Ok(Val::Media { sha256: hash, mime: kept.mime, bytes });
                }
                // Whoever renamed last, the type is the one now kept.
                let mime = self.get(&hash).map_or_else(|| mime.to_string(), |b| b.mime);
                Ok(Val::Media { sha256: hash, mime, bytes })
            }
            None => {
                self.mem.insert(hash.clone(), Blob { mime: mime.to_string(), data: Arc::new(data) });
                Ok(Val::Media { sha256: hash, mime: mime.to_string(), bytes })
            }
        }
    }

    /// The bytes kept so far, counted afresh, since other pods share the
    /// directory.
    fn used(&self) -> u64 {
        let on_disk = self.dir.as_ref().and_then(|d| std::fs::read_dir(d).ok()).into_iter().flatten().flatten().filter_map(|e| e.metadata().ok()).map(|m| m.len()).sum::<u64>();
        on_disk + self.mem.values().map(|b| b.data.len() as u64).sum::<u64>()
    }

    /// The media with this hash, if it is kept and its content still has it.
    pub fn get(&self, hash: &str) -> Option<Blob> {
        if let Some(b) = self.mem.get(hash) {
            return Some(b.clone());
        }
        if hash.len() != 64 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let raw = std::fs::read(self.dir.as_ref()?.join(hash)).ok()?;
        let at = raw.iter().position(|b| *b == b'\n')?;
        let mime = String::from_utf8(raw[..at].to_vec()).ok()?;
        let data = raw[at + 1..].to_vec();
        (sha256(&data) == hash).then(|| Blob { mime, data: Arc::new(data) })
    }

    /// Whether the value refers to media this store keeps, as it says.
    pub fn holds(&self, v: &Val) -> bool {
        match v {
            Val::Media { sha256, mime, bytes } => self.get(sha256).is_some_and(|b| b.mime == *mime && b.data.len() as u64 == *bytes),
            _ => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_of_known_values() {
        assert_eq!(sha256(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(sha256(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn a_store_keeps_media_once_and_checks_it_on_reading() {
        let dir = std::env::temp_dir().join(format!("suture-media-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut s = Store::in_dir(dir.clone());
        let v = s.put("image/png", b"png".to_vec()).unwrap();
        let Val::Media { sha256: h, .. } = &v else { panic!() };
        assert!(s.holds(&v));
        assert_eq!(s.get(h).unwrap().data.as_slice(), b"png");
        std::fs::write(dir.join(h), b"image/png\nnot png").unwrap();
        assert!(s.get(h).is_none(), "content that no longer has its hash is not given out");
        assert!(s.put("Image/PNG", vec![]).is_err());
        // Corrupt content is replaced; the same content keeps its first type.
        let again = s.put("image/png", b"png".to_vec()).unwrap();
        assert!(s.holds(&again));
        assert_eq!(s.put("image/jpeg", b"png".to_vec()).unwrap(), again);
        let mut mem = Store::default();
        let first = mem.put("image/png", b"x".to_vec()).unwrap();
        assert_eq!(mem.put("text/plain", b"x".to_vec()).unwrap(), first);
        assert!(std::fs::read_dir(&dir).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().ends_with(".tmp")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
