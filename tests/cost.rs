//! Work-count acceptance tests (language spec §7.4). They count work and
//! never time it.

mod common;

use common::*;
use suture::dnc::FirstBackend;
use suture::nets::depnet::world::{generate, GenParams};
use suture::nets::depnet::{DepConfig, Depnet, RunConfig};
use suture::*;

/// C2: taking new effects examines only the new ones, however many are pending.
#[test]
fn c2_taking_new_effects_ignores_the_backlog() {
    for pending in [0usize, 10_000, 100_000] {
        let o = orders();
        let mut m = Machine::new(o.prog.clone());
        place(&mut m, &o, 0, pending);
        assert_eq!(m.take_new_effects().len(), pending);
        let before = m.stats().effects_examined;
        place(&mut m, &o, pending as i64, 5);
        assert_eq!(m.take_new_effects().len(), 5);
        assert_eq!(m.stats().effects_examined - before, 5, "with {pending} pending");
        assert_eq!(m.outbox().len(), pending + 5);
    }
}

/// C3, payload half: DNC-1's worlds, which carry the largest payloads among
/// the existing nets, fit the default limit. The other half of C3 (a step
/// reads only its two meeting agents) holds by construction, because
/// `RuleCx` and `PatchView` expose only the meeting pair and the patch.
#[test]
fn c3_existing_nets_fit_the_payload_limit() {
    let limit = Limits::default().max_payload_bytes;
    let worlds = [
        ("small", GenParams::small()),
        ("contended", GenParams::contended()),
        ("contended_large", GenParams::contended_large()),
        ("loose", GenParams::loose()),
    ];
    for (name, params) in worlds {
        let mut d = Depnet::new(&generate(&params, 7), &DepConfig::default()).unwrap();
        d.run(&mut FirstBackend, &RunConfig::default());
        assert!(d.m.faults().iter().all(|f| !f.error.contains("bytes, limit")), "{name}: {:?}", d.m.faults());
        let largest = d.m.graph().agents_sorted().iter().map(|(_, a)| a.payload.encoded_len()).max().unwrap_or(0);
        assert!(largest <= limit, "{name}: the largest payload is {largest} bytes");
    }
}
