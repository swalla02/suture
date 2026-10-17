use std::sync::Arc;

use suture::Machine;
use suture_lang::ir::Program;
use suture_lang::scenario::{parse_scenarios, Scenario};
use suture_runtime::{kernel, scenario::run};

fn shop() -> Program {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../examples/shop");
    suture_lang::compile_dir(std::path::Path::new(dir)).unwrap_or_else(|e| panic!("{e:?}"))
}

fn scenarios() -> Vec<Scenario> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../examples/shop/orders.scenario");
    parse_scenarios("orders.scenario", &std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn every_scenario_replays_to_the_same_state() {
    let p = shop();
    for sc in scenarios() {
        let report = run(&p, &sc);
        assert!(report.passed(), "{}: {:?}", report.title, report.failures);
        let digest = report.host.machine.digest();
        let (kernel_program, _) = kernel::program(Arc::new(p.clone()));
        let replayed = Machine::recover(kernel_program, report.host.machine.into_journal())
            .unwrap_or_else(|e| panic!("{}: {e}", sc.title));
        assert_eq!(replayed.digest(), digest, "{}", sc.title);
    }
}

#[test]
fn a_journal_does_not_replay_under_a_different_program() {
    let p = shop();
    let report = run(&p, &scenarios()[1]);
    let mut other = p.clone();
    other.rules.retain(|r| r.name != "late");
    let (kernel_program, _) = kernel::program(Arc::new(other));
    let err = Machine::recover(kernel_program, report.host.machine.into_journal()).err().expect("must refuse");
    assert!(matches!(err, suture::RecoveryError::Version { .. }), "{err}");
}

#[test]
fn every_workspace_scenario_replays_to_the_same_state() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../examples/workspaces");
    let p = suture_lang::compile_dir(std::path::Path::new(dir)).unwrap_or_else(|e| panic!("{e:?}"));
    let src = std::fs::read_to_string(format!("{dir}/workspaces.scenario")).unwrap();
    for sc in parse_scenarios("workspaces.scenario", &src).unwrap() {
        let report = run(&p, &sc);
        assert!(report.passed(), "{}: {:?}", report.title, report.failures);
        let digest = report.host.machine.digest();
        let (kernel_program, _) = kernel::program(Arc::new(p.clone()));
        let replayed = Machine::recover(kernel_program, report.host.machine.into_journal()).unwrap_or_else(|e| panic!("{}: {e}", sc.title));
        assert_eq!(replayed.digest(), digest, "{}", sc.title);
    }
}
