//! Experiment DNC-1, scenario level: a dependency network assembles itself
//! through local decisions and reorganizes after disturbances.

use suture::dnc::{DecisionBackend, DecisionError, DecisionRequest, DecisionResult, FirstBackend, RandomBackend};
use suture::nets::depnet::state::{Ctl, DEAD, GAVE_UP};
use suture::nets::depnet::world::{generate, GenParams, ProviderSpec, RootSpec, WorldSpec};
use suture::nets::depnet::{self, DepConfig, Depnet, Eval, RunConfig};
use suture::*;

fn prov(id: i64, ty: i64, region: i64, capacity: i64, recipes: Vec<Vec<i64>>) -> ProviderSpec {
    ProviderSpec { id, ty, region, capacity, recipes }
}

fn root(id: i64, needs: Vec<i64>, regions: Vec<i64>) -> RootSpec {
    RootSpec { id, needs, regions }
}

fn world(layers: Vec<Vec<i64>>, providers: Vec<ProviderSpec>, roots: Vec<RootSpec>) -> WorldSpec {
    WorldSpec { layers, providers, roots, regions: 2 }
}

fn assemble(spec: &WorldSpec, backend: &mut dyn DecisionBackend) -> (Depnet, Eval) {
    let mut d = Depnet::new(spec, &DepConfig::default()).unwrap();
    let st = d.run(backend, &RunConfig::default());
    assert!(!st.budget_exhausted, "did not settle");
    let e = d.evaluate();
    assert!(e.violations.is_empty(), "{:?}", e.violations);
    d.m.check_invariants().unwrap();
    (d, e)
}

#[test]
fn coverage_is_complete() {
    let (p, _) = depnet::program(&DepConfig::default());
    let c = p.rules.coverage(&p.sig);
    assert!(c.missing.is_empty(), "{:?}", c.missing);
    assert!(c.never_fires.is_empty(), "{:?}", c.never_fires);
    assert_eq!(c.forbidden.len(), 4, "channel messages that cannot arrive in that state");
}

#[test]
fn single_root_binds_a_leaf() {
    let spec = world(vec![vec![1]], vec![prov(100, 1, 0, 1, vec![vec![]])], vec![root(1, vec![1], vec![0])]);
    let (_, e) = assemble(&spec, &mut FirstBackend);
    assert_eq!((e.roots, e.satisfied, e.bindings), (1, 1, 1));
}

#[test]
fn activation_spawns_dependencies() {
    let spec = world(
        vec![vec![1], vec![2]],
        vec![prov(100, 1, 0, 1, vec![vec![2]]), prov(200, 2, 0, 1, vec![vec![]])],
        vec![root(1, vec![1], vec![0])],
    );
    let (d, e) = assemble(&spec, &mut FirstBackend);
    assert_eq!((e.satisfied, e.bindings, e.active), (1, 2, 2));
    assert_eq!(d.provider(100).unwrap().deps[0].bound, 200);
}

#[test]
fn region_policy_holds_or_the_root_gives_up() {
    let spec = world(vec![vec![1]], vec![prov(100, 1, 1, 1, vec![vec![]])], vec![root(1, vec![1], vec![0])]);
    let (d, e) = assemble(&spec, &mut FirstBackend);
    assert_eq!((e.satisfied, e.bindings), (0, 0));
    assert_eq!(d.provider(1).unwrap().phase, GAVE_UP);
}

#[test]
fn contention_respects_capacity() {
    let spec = world(
        vec![vec![1]],
        vec![prov(100, 1, 0, 1, vec![vec![]])],
        vec![root(1, vec![1], vec![0]), root(2, vec![1], vec![0])],
    );
    let (_, e) = assemble(&spec, &mut FirstBackend);
    assert_eq!((e.satisfied, e.bindings), (1, 1));
}

#[test]
fn dependency_failure_switches_recipe() {
    // Recipe 0 needs type 2, which nobody provides; recipe 1 needs type 3.
    let spec = world(
        vec![vec![1], vec![2, 3]],
        vec![prov(100, 1, 0, 1, vec![vec![2], vec![3]]), prov(300, 3, 0, 1, vec![vec![]])],
        vec![root(1, vec![1], vec![0])],
    );
    let (d, e) = assemble(&spec, &mut FirstBackend);
    assert_eq!(e.satisfied, 1);
    let p = d.provider(100).unwrap();
    assert_eq!((p.recipe, p.tried.clone()), (1, vec![0]));
}

#[test]
fn unbuildable_provider_dies_and_its_consumer_moves_on() {
    let spec = world(
        vec![vec![1], vec![2]],
        vec![prov(100, 1, 0, 1, vec![vec![2]]), prov(101, 1, 0, 1, vec![vec![]])],
        vec![root(1, vec![1], vec![0])],
    );
    let (d, e) = assemble(&spec, &mut FirstBackend); // list order picks 100 first
    assert_eq!(e.satisfied, 1);
    assert_eq!(d.provider(100).unwrap().phase, DEAD);
    assert_eq!(d.provider(1).unwrap().deps[0].bound, 101);
}

fn two_leaves(capacity: i64, roots: usize) -> WorldSpec {
    world(
        vec![vec![1]],
        vec![prov(100, 1, 0, capacity, vec![vec![]]), prov(101, 1, 0, capacity, vec![vec![]])],
        (1..=roots as i64).map(|i| root(i, vec![1], vec![0])).collect(),
    )
}

#[test]
fn killing_a_provider_triggers_local_recovery() {
    let (mut d, e) = assemble(&two_leaves(1, 1), &mut FirstBackend);
    assert_eq!(d.provider(1).unwrap().deps[0].bound, 100);
    assert_eq!(e.satisfied, 1);
    d.deliver(1, Ctl::Kill { provider: 100 }).unwrap();
    d.run(&mut FirstBackend, &RunConfig::default());
    let e = d.evaluate();
    assert!(e.violations.is_empty(), "{:?}", e.violations);
    assert_eq!(e.satisfied, 1);
    assert_eq!(d.provider(1).unwrap().deps[0].bound, 101);
}

#[test]
fn reducing_capacity_evicts_and_consumers_rebind() {
    let (mut d, e) = assemble(&two_leaves(2, 2), &mut FirstBackend);
    assert_eq!(e.satisfied, 2);
    assert_eq!(d.provider(100).unwrap().consumers.len(), 2);
    d.deliver(1, Ctl::SetCap { provider: 100, capacity: 0 }).unwrap();
    d.run(&mut FirstBackend, &RunConfig::default());
    let e = d.evaluate();
    assert!(e.violations.is_empty(), "{:?}", e.violations);
    assert_eq!(e.satisfied, 2);
    assert_eq!(d.provider(101).unwrap().consumers.len(), 2);
}

#[test]
fn a_new_requirement_is_absorbed() {
    let (mut d, e) = assemble(&two_leaves(1, 1), &mut FirstBackend);
    assert_eq!(e.roots, 1);
    d.add_root(&root(9, vec![1], vec![0])).unwrap();
    d.run(&mut FirstBackend, &RunConfig::default());
    let e = d.evaluate();
    assert!(e.violations.is_empty(), "{:?}", e.violations);
    assert_eq!((e.roots, e.satisfied), (2, 2));
}

/// Always selects an index past the end: every decision is invalid.
struct OutOfRange;
impl DecisionBackend for OutOfRange {
    fn model(&self) -> String {
        "out-of-range".into()
    }
    fn decide(&mut self, batch: &[DecisionRequest]) -> Result<Vec<DecisionResult>, DecisionError> {
        Ok(batch
            .iter()
            .map(|r| DecisionResult { selected: r.features.len() + 3, scores: None, model: self.model(), trace: String::new() })
            .collect())
    }
}

#[test]
fn no_decision_sequence_violates_hard_constraints() {
    for seed in 0..25 {
        let spec = generate(&GenParams::small(), seed);
        let backends: Vec<Box<dyn DecisionBackend>> =
            vec![Box::new(RandomBackend::new(seed)), Box::new(OutOfRange), Box::new(FirstBackend)];
        for mut b in backends {
            let mut d = Depnet::new(&spec, &DepConfig::default()).unwrap();
            let st = d.run(b.as_mut(), &RunConfig { seed, ..RunConfig::default() });
            assert!(!st.budget_exhausted, "seed {seed} {}", b.model());
            let e = d.evaluate();
            assert!(e.violations.is_empty(), "seed {seed} {}: {:?}", b.model(), e.violations);
            assert!(d.m.faults().is_empty(), "seed {seed}: {:?}", d.m.faults());
        }
    }
}

#[test]
fn runs_are_deterministic_and_replay_without_the_model() {
    let spec = generate(&GenParams::small(), 11);
    let rc = RunConfig { seed: 5, ..RunConfig::default() };
    let mut a = Depnet::new(&spec, &DepConfig::default()).unwrap();
    a.run(&mut RandomBackend::new(3), &rc);
    let mut b = Depnet::new(&spec, &DepConfig::default()).unwrap();
    b.run(&mut RandomBackend::new(3), &rc);
    assert_eq!(a.m.digest(), b.m.digest());
    a.m.commit().unwrap();
    let digest = a.m.digest();
    let (prog, _) = depnet::program(&DepConfig::default());
    let replayed = Machine::recover(prog, a.m.into_journal()).unwrap();
    assert_eq!(replayed.digest(), digest);
}

#[test]
fn the_kernel_scheduler_does_not_change_the_outcome() {
    let spec = generate(&GenParams::small(), 4);
    let mut digests = vec![];
    for s in [SchedulerKind::Fifo, SchedulerKind::Lifo, SchedulerKind::Random(9)] {
        let mut d = Depnet::new(&spec, &DepConfig::default()).unwrap();
        d.m.set_scheduler(s);
        d.run(&mut RandomBackend::new(1), &RunConfig { seed: 2, ..RunConfig::default() });
        digests.push(d.m.digest());
    }
    assert!(digests.windows(2).all(|w| w[0] == w[1]));
}

/// Pending routed messages and the control message each one carries.
fn pending_ctls(d: &Depnet) -> Vec<(EffectView, Option<Ctl>)> {
    d.m.outbox()
        .into_iter()
        .filter(|e| e.kind == d.k.send)
        .map(|e| {
            let agent = d.m.graph().agent(e.agent).unwrap();
            let ctl = match agent.ports[1] {
                Endpoint::Port(msg, 0) => d.m.graph().agent(msg).and_then(|a| Ctl::from_value(&a.payload).ok()),
                _ => None,
            };
            (e, ctl)
        })
        .collect()
}

#[test]
fn a_loss_overtaking_its_grant_notification_is_not_lost() {
    // The transport may deliver a binding's DepLost before its DepBound.
    let mut d = Depnet::new(&two_leaves(1, 1), &DepConfig::default()).unwrap();
    // Drive until the root's DepBound notification is in flight.
    for _ in 0..50 {
        d.m.run(None);
        let pending = pending_ctls(&d);
        if pending.iter().any(|(_, c)| matches!(c, Some(Ctl::DepBound { .. }))) {
            break;
        }
        for (e, _) in &pending {
            d.route(e).unwrap();
        }
        let sites = d.resolver.pending(&d.m);
        d.resolver.resolve_sites(&mut d.m, &mut FirstBackend, &sites).unwrap();
    }
    // Kill the provider before the owner hears about the binding.
    d.deliver(1, Ctl::Kill { provider: 100 }).unwrap();
    d.m.run(None);
    let mut pending = pending_ctls(&d);
    assert!(pending.iter().any(|(_, c)| matches!(c, Some(Ctl::DepLost { .. }))));
    // Deliver the loss first.
    pending.sort_by_key(|(_, c)| !matches!(c, Some(Ctl::DepLost { .. })));
    for (e, _) in &pending {
        d.route(e).unwrap();
    }
    d.run(&mut FirstBackend, &RunConfig::default());
    let e = d.evaluate();
    assert!(e.violations.is_empty(), "{:?}", e.violations);
    assert_eq!(e.satisfied, 1);
    assert_eq!(d.provider(1).unwrap().deps[0].bound, 101);
}

#[test]
fn asynchrony_reordering_and_disturbances_never_break_the_structure() {
    for seed in 0..40 {
        let params = if seed % 2 == 0 { GenParams::contended() } else { GenParams::contended_deep() };
        let spec = generate(&params, 600 + seed);
        let rc = RunConfig { seed, route_fraction: 0.3, ..RunConfig::default() };
        let mut d = Depnet::new(&spec, &DepConfig::default()).unwrap();
        let mut backend = RandomBackend::new(seed);
        // Disturb mid-assembly, while messages are still in flight.
        d.run(&mut backend, &RunConfig { max_rounds: 6, ..rc.clone() });
        for p in spec.providers.iter().filter(|p| p.id % 4 == (seed % 4) as i64) {
            d.deliver(p.ty, Ctl::Kill { provider: p.id }).unwrap();
        }
        for p in spec.providers.iter().filter(|p| p.id % 5 == 0) {
            d.deliver(p.ty, Ctl::SetCap { provider: p.id, capacity: 1 }).unwrap();
        }
        let st = d.run(&mut backend, &rc);
        assert!(!st.budget_exhausted, "seed {seed}");
        let e = d.evaluate();
        assert!(e.violations.is_empty(), "seed {seed}: {:?}", e.violations);
        assert!(d.m.faults().is_empty(), "seed {seed}: {:?}", d.m.faults());
    }
}
