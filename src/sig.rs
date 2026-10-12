//! Signatures: port types and agent kinds.
//!
//! A port type is an atom with a polarity. A wire may connect two ports when
//! their types are *compatible*: the same polarized type with opposite
//! polarities, or the same self-dual type (any polarity). Agent kinds fix the
//! principal port type, the auxiliary port types, the payload schema, and two
//! flags: `effect` (the principal faces the host) and `conserved` (a linear
//! capability whose key must be preserved by every rewrite).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::value::{Value, ValueType};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TypeId(pub u16);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct KindId(pub u16);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Polarity {
    Pos,
    Neg,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PortType {
    pub ty: TypeId,
    pub pol: Polarity,
}

impl TypeId {
    /// The producing side of a polarized type.
    pub fn pos(self) -> PortType {
        PortType { ty: self, pol: Polarity::Pos }
    }
    /// The consuming side of a polarized type.
    pub fn neg(self) -> PortType {
        PortType { ty: self, pol: Polarity::Neg }
    }
    /// A port of a self-dual type (polarity is irrelevant).
    pub fn any(self) -> PortType {
        self.pos()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypeMode {
    Polarized,
    SelfDual,
}

#[derive(Clone, Debug)]
pub struct TypeDecl {
    pub name: String,
    pub mode: TypeMode,
}

#[derive(Clone, Debug)]
pub struct PortDecl {
    pub name: String,
    pub ty: PortType,
}

/// Marks a kind as a linear capability of resource class `class`, identified
/// by tuple field `key` of its payload.
#[derive(Clone, Debug)]
pub struct Conserved {
    pub class: String,
    pub key: usize,
}

#[derive(Clone, Debug)]
pub struct KindDecl {
    pub name: String,
    pub principal: PortType,
    pub aux: Vec<PortDecl>,
    pub payload: ValueType,
    pub effect: bool,
    pub conserved: Option<Conserved>,
}

#[derive(Debug, thiserror::Error)]
pub enum SigError {
    #[error("duplicate type name {0}")]
    DuplicateType(String),
    #[error("duplicate kind name {0}")]
    DuplicateKind(String),
    #[error("too many types or kinds")]
    TooMany,
    #[error("kind {0}: conserved key is not a field of its tuple payload schema")]
    ConservedKeyOutOfSchema(String),
}

/// An immutable, versioned signature.
#[derive(Clone, Debug)]
pub struct Signature {
    name: String,
    version: u32,
    types: Vec<TypeDecl>,
    kinds: Vec<KindDecl>,
    by_name: HashMap<String, KindId>,
}

impl Signature {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    pub fn kind(&self, k: KindId) -> &KindDecl {
        &self.kinds[k.0 as usize]
    }

    pub fn kind_id(&self, name: &str) -> Option<KindId> {
        self.by_name.get(name).copied()
    }

    pub fn kinds(&self) -> impl Iterator<Item = (KindId, &KindDecl)> {
        self.kinds.iter().enumerate().map(|(i, k)| (KindId(i as u16), k))
    }

    pub fn type_decl(&self, t: TypeId) -> &TypeDecl {
        &self.types[t.0 as usize]
    }

    /// Number of auxiliary ports.
    pub fn arity(&self, k: KindId) -> usize {
        self.kind(k).aux.len()
    }

    /// Type of port `port` of kind `k`; port 0 is the principal port.
    pub fn port_type(&self, k: KindId, port: usize) -> PortType {
        let d = self.kind(k);
        if port == 0 { d.principal } else { d.aux[port - 1].ty }
    }

    /// Whether a wire may connect ports of types `a` and `b`.
    pub fn compatible(&self, a: PortType, b: PortType) -> bool {
        a.ty == b.ty
            && match self.type_decl(a.ty).mode {
                TypeMode::SelfDual => true,
                TypeMode::Polarized => a.pol != b.pol,
            }
    }

    pub fn fmt_port_type(&self, p: PortType) -> String {
        let d = self.type_decl(p.ty);
        match (d.mode, p.pol) {
            (TypeMode::SelfDual, _) => d.name.clone(),
            (TypeMode::Polarized, Polarity::Pos) => format!("{}+", d.name),
            (TypeMode::Polarized, Polarity::Neg) => format!("{}-", d.name),
        }
    }

    /// The `(class, key)` of a conserved agent, or `None` for other kinds.
    pub fn conserved_key(&self, k: KindId, payload: &Value) -> Option<(String, Value)> {
        let c = self.kind(k).conserved.as_ref()?;
        let key = payload.get(c.key).cloned().unwrap_or_else(|_| payload.clone());
        Some((c.class.clone(), key))
    }
}

/// Builds a [`Signature`]; validation happens in [`SigBuilder::build`].
pub struct SigBuilder {
    name: String,
    version: u32,
    types: Vec<TypeDecl>,
    kinds: Vec<KindDecl>,
}

impl SigBuilder {
    pub fn new(name: &str, version: u32) -> SigBuilder {
        SigBuilder { name: name.to_string(), version, types: vec![], kinds: vec![] }
    }

    fn add_type(&mut self, name: &str, mode: TypeMode) -> TypeId {
        self.types.push(TypeDecl { name: name.to_string(), mode });
        TypeId((self.types.len() - 1) as u16)
    }

    pub fn polarized(&mut self, name: &str) -> TypeId {
        self.add_type(name, TypeMode::Polarized)
    }

    pub fn self_dual(&mut self, name: &str) -> TypeId {
        self.add_type(name, TypeMode::SelfDual)
    }

    pub fn kind(&mut self, name: &str, principal: PortType) -> KindBuilder<'_> {
        KindBuilder {
            sig: self,
            decl: KindDecl {
                name: name.to_string(),
                principal,
                aux: vec![],
                payload: ValueType::Unit,
                effect: false,
                conserved: None,
            },
        }
    }

    pub fn build(self) -> Result<Signature, SigError> {
        if self.types.len() > u16::MAX as usize || self.kinds.len() > u16::MAX as usize {
            return Err(SigError::TooMany);
        }
        let mut seen = HashMap::new();
        for t in &self.types {
            if seen.insert(t.name.clone(), ()).is_some() {
                return Err(SigError::DuplicateType(t.name.clone()));
            }
        }
        let mut by_name = HashMap::new();
        for (i, k) in self.kinds.iter().enumerate() {
            if by_name.insert(k.name.clone(), KindId(i as u16)).is_some() {
                return Err(SigError::DuplicateKind(k.name.clone()));
            }
            if let Some(c) = &k.conserved {
                let ok = matches!(&k.payload, ValueType::Tuple(fields) if c.key < fields.len());
                if !ok {
                    return Err(SigError::ConservedKeyOutOfSchema(k.name.clone()));
                }
            }
        }
        Ok(Signature {
            name: self.name,
            version: self.version,
            types: self.types,
            kinds: self.kinds,
            by_name,
        })
    }
}

pub struct KindBuilder<'a> {
    sig: &'a mut SigBuilder,
    decl: KindDecl,
}

impl KindBuilder<'_> {
    pub fn aux(mut self, name: &str, ty: PortType) -> Self {
        self.decl.aux.push(PortDecl { name: name.to_string(), ty });
        self
    }

    pub fn payload(mut self, t: ValueType) -> Self {
        self.decl.payload = t;
        self
    }

    /// The principal port of an effect agent faces the host (an outbox hole).
    pub fn effect(mut self) -> Self {
        self.decl.effect = true;
        self
    }

    pub fn conserved(mut self, class: &str, key_field: usize) -> Self {
        self.decl.conserved = Some(Conserved { class: class.to_string(), key: key_field });
        self
    }

    pub fn done(self) -> KindId {
        self.sig.kinds.push(self.decl);
        KindId((self.sig.kinds.len() - 1) as u16)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{tup, Value, ValueType};

    #[test]
    fn compatibility() {
        let mut b = SigBuilder::new("t", 1);
        let nat = b.polarized("Nat");
        let c = b.self_dual("C");
        let sig = b.build().unwrap();
        assert!(sig.compatible(nat.pos(), nat.neg()));
        assert!(!sig.compatible(nat.pos(), nat.pos()));
        assert!(!sig.compatible(nat.neg(), nat.neg()));
        assert!(sig.compatible(c.any(), c.any()));
        assert!(!sig.compatible(nat.pos(), c.any()));
        assert_eq!(sig.fmt_port_type(nat.neg()), "Nat-");
        assert_eq!(sig.fmt_port_type(c.any()), "C");
    }

    #[test]
    fn kinds() {
        let mut b = SigBuilder::new("t", 1);
        let nat = b.polarized("Nat");
        let s = b.kind("S", nat.pos()).aux("pred", nat.neg()).done();
        let lease = b
            .kind("Lease", nat.pos())
            .payload(ValueType::Tuple(vec![ValueType::Int]))
            .conserved("lease", 0)
            .done();
        let ask = b.kind("Ask", nat.neg()).effect().done();
        let sig = b.build().unwrap();
        assert_eq!(sig.arity(s), 1);
        assert_eq!(sig.port_type(s, 0), nat.pos());
        assert_eq!(sig.port_type(s, 1), nat.neg());
        assert_eq!(sig.kind_id("S"), Some(s));
        assert!(sig.kind(ask).effect);
        assert_eq!(
            sig.conserved_key(lease, &tup![9]),
            Some(("lease".to_string(), Value::Int(9)))
        );
        assert_eq!(sig.conserved_key(s, &Value::Unit), None);
        assert_eq!(sig.name(), "t");
        assert_eq!(sig.version(), 1);
    }

    #[test]
    fn rejects_bad_signatures() {
        let mut b = SigBuilder::new("t", 1);
        let nat = b.polarized("Nat");
        b.kind("S", nat.pos()).done();
        b.kind("S", nat.neg()).done();
        assert!(matches!(b.build(), Err(SigError::DuplicateKind(_))));

        let mut b = SigBuilder::new("t", 1);
        b.polarized("Nat");
        b.polarized("Nat");
        assert!(matches!(b.build(), Err(SigError::DuplicateType(_))));

        let mut b = SigBuilder::new("t", 1);
        let nat = b.polarized("Nat");
        b.kind("L", nat.pos()).conserved("lease", 0).done(); // Unit payload has no field 0
        assert!(matches!(b.build(), Err(SigError::ConservedKeyOutOfSchema(_))));
    }
}
