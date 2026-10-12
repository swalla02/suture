//! Lineage identities and digests.
//!
//! An agent's identity is a 128-bit hash of its causal origin: the pair of
//! agents whose interaction created it, the rule name, and its index in the
//! patch (or, for injected agents, the inject sequence number and index).
//! Identities therefore never depend on the order in which independent
//! reductions were scheduled. Hashing is SipHash-2-4 with fixed keys, a stable
//! specification, so identities persisted in journals stay valid across builds.

use std::fmt;
use std::hash::{BuildHasherDefault, Hasher};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use siphasher::sip128::{Hasher128, SipHasher24};

const K0: u64 = 0x5375_7475_7265_2d30; // "Suture-0"
const K1: u64 = 0x6c69_6e65_6167_6531; // "lineage1"

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AgentId(pub u128);

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct HoleId(pub u128);

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Digest(pub u128);

/// Incremental, domain-separated, length-prefixed SipHash-2-4/128.
pub struct DigestBuilder(SipHasher24);

impl DigestBuilder {
    pub fn new(domain: &str) -> Self {
        let mut d = DigestBuilder(SipHasher24::new_with_keys(K0, K1));
        d.str(domain);
        d
    }
    pub fn u8(&mut self, x: u8) {
        self.0.write(&[x]);
    }
    pub fn u32(&mut self, x: u32) {
        self.0.write(&x.to_le_bytes());
    }
    pub fn u64(&mut self, x: u64) {
        self.0.write(&x.to_le_bytes());
    }
    pub fn u128(&mut self, x: u128) {
        self.0.write(&x.to_le_bytes());
    }
    pub fn bytes(&mut self, b: &[u8]) {
        self.u64(b.len() as u64);
        self.0.write(b);
    }
    pub fn str(&mut self, s: &str) {
        self.bytes(s.as_bytes());
    }
    pub fn finish(self) -> Digest {
        Digest(self.0.finish128().as_u128())
    }
}

impl AgentId {
    pub fn from_inject(seq: u64, index: u32) -> AgentId {
        let mut d = DigestBuilder::new("agent/inject");
        d.u64(seq);
        d.u32(index);
        AgentId(d.finish().0)
    }

    pub fn from_reduction(left: AgentId, right: AgentId, rule: &str, index: u32) -> AgentId {
        let mut d = DigestBuilder::new("agent/reduce");
        d.u128(left.0);
        d.u128(right.0);
        d.str(rule);
        d.u32(index);
        AgentId(d.finish().0)
    }
}

impl HoleId {
    pub fn from_inject(seq: u64, index: u32) -> HoleId {
        let mut d = DigestBuilder::new("hole/inject");
        d.u64(seq);
        d.u32(index);
        HoleId(d.finish().0)
    }

    /// The outbox hole facing an effect agent's principal port.
    pub fn for_effect(agent: AgentId) -> HoleId {
        let mut d = DigestBuilder::new("hole/effect");
        d.u128(agent.0);
        HoleId(d.finish().0)
    }
}

macro_rules! hex_id {
    ($t:ident, $prefix:literal) => {
        impl fmt::Display for $t {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, concat!($prefix, "{:08x}"), (self.0 >> 96) as u32)
            }
        }
        impl fmt::Debug for $t {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(self, f)
            }
        }
        impl Serialize for $t {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(&format!("{:032x}", self.0))
            }
        }
        impl<'de> Deserialize<'de> for $t {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let s = String::deserialize(d)?;
                if s.len() != 32 {
                    return Err(serde::de::Error::custom("expected 32 hex digits"));
                }
                u128::from_str_radix(&s, 16).map($t).map_err(serde::de::Error::custom)
            }
        }
    };
}

hex_id!(AgentId, "");
hex_id!(HoleId, "h:");

impl fmt::Display for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:032x}", self.0)
    }
}
impl fmt::Debug for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
impl Serialize for Digest {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}
impl<'de> Deserialize<'de> for Digest {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        u128::from_str_radix(&s, 16).map(Digest).map_err(serde::de::Error::custom)
    }
}

/// Hasher for maps keyed by lineage IDs, which are already uniformly random.
#[derive(Default)]
pub struct IdHasher(u64);

impl Hasher for IdHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = self.0.rotate_left(8) ^ b as u64;
        }
    }
    fn write_u128(&mut self, x: u128) {
        self.0 ^= x as u64;
    }
}

pub type IdBuildHasher = BuildHasherDefault<IdHasher>;

#[cfg(test)]
mod tests {
    use super::*;

    /// Pinned output of `AgentId::from_inject(0, 0)`. Journals persist lineage
    /// IDs, so a change here breaks replay of every existing journal.
    const KAT_INJECT_0_0: &str = "bd642fc890515857cc34d1b32d1d5a51";

    #[test]
    fn deterministic_and_distinct() {
        let a = AgentId::from_inject(0, 0);
        let b = AgentId::from_inject(0, 1);
        assert_ne!(a, b);
        assert_eq!(a, AgentId::from_inject(0, 0));
        let c = AgentId::from_reduction(a, b, "r", 0);
        assert_ne!(c, AgentId::from_reduction(b, a, "r", 0));
        assert_ne!(c, AgentId::from_reduction(a, b, "r2", 0));
        assert_ne!(c, AgentId::from_reduction(a, b, "r", 1));
        assert_ne!(HoleId::from_inject(0, 0).0, AgentId::from_inject(0, 0).0, "domain separation");
        assert_ne!(HoleId::for_effect(a), HoleId::from_inject(0, 0));
    }

    #[test]
    fn known_answer_pins_the_hash() {
        assert_eq!(format!("{:032x}", AgentId::from_inject(0, 0).0), KAT_INJECT_0_0);
    }

    #[test]
    fn serde_hex_roundtrip() {
        let a = AgentId::from_inject(3, 4);
        let s = serde_json::to_string(&a).unwrap();
        assert_eq!(s.len(), 34);
        assert_eq!(serde_json::from_str::<AgentId>(&s).unwrap(), a);
        let h = HoleId::for_effect(a);
        assert_eq!(serde_json::from_str::<HoleId>(&serde_json::to_string(&h).unwrap()).unwrap(), h);
        assert!(serde_json::from_str::<AgentId>("\"xyz\"").is_err());
    }

    #[test]
    fn digest_builder_is_length_prefixed() {
        let mut a = DigestBuilder::new("t");
        a.str("ab");
        a.str("c");
        let mut b = DigestBuilder::new("t");
        b.str("a");
        b.str("bc");
        assert_ne!(a.finish(), b.finish());
    }
}
