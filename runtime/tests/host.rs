use suture::Machine;
use suture_lang::ir::Program;
use suture_runtime::host::Host;
use suture_runtime::kernel;
use suture_runtime::interp::in_state;
use suture_runtime::state::Val;

fn shop() -> Program {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../examples/shop");
    suture_lang::compile_dir(std::path::Path::new(dir)).unwrap_or_else(|e| panic!("{e:?}"))
}

fn french(h: &mut Host) {
    let given = [("country".to_string(), Val::Text("France".into())), ("total".to_string(), Val::Money(12000))];
    h.create("Order", "O1", &given).unwrap();
}

fn data(pairs: &[(&str, Val)]) -> Vec<(String, Val)> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
}

#[test]
fn a_french_order_runs_end_to_end_on_the_kernel() {
    let mut h = Host::new(shop());
    french(&mut h);
    h.answer("payments", "captured", &[], None).unwrap();
    let over = data(&[("value", Val::Money(15000)), ("contents", Val::Text("alcohol".into()))]);
    h.answer("forms", "filled", &over, None).unwrap();
    let right = data(&[("value", Val::Money(10000)), ("contents", Val::Text("alcohol".into()))]);
    h.answer("forms", "filled", &right, None).unwrap();
    h.answer("forms", "filled", &data(&[("licence", Val::Text("L-77".into()))]), None).unwrap();
    h.answer("carriers", "delivered", &[], None).unwrap();
    let u = h.unit("O1").unwrap();
    assert!(in_state(&h.prog, &u.root, &[], "settled"));
    let trace = h.trace.join("\n");
    assert!(trace.contains("refused by rule declared"), "{trace}");
    assert!(trace.contains("goal met"), "{trace}");
    assert!(h.machine.faults().is_empty(), "{:?}", h.machine.faults());
    assert!(h.pending().is_empty());
}

#[test]
fn an_oversized_unit_is_reported_not_panicked() {
    let mut h = Host::new(shop());
    french(&mut h);
    let big = data(&[("value", Val::Money(100)), ("contents", Val::Text("x".repeat(3_600)))]);
    let err = h.answer("forms", "filled", &big, None).unwrap_err();
    assert!(err.contains("bytes, limit 4096"), "{err}");
}

#[test]
fn answers_with_no_pending_request_are_errors_not_panics() {
    let mut h = Host::new(shop());
    let err = h.answer("payments", "captured", &[], None).unwrap_err();
    assert!(err.contains("payments has no pending request"), "{err}");
    let err = h.create("Payment", "P1", &[]).unwrap_err();
    assert!(err.contains("is a part of Order"), "{err}");
}

#[test]
fn a_faulted_step_is_journaled_and_replays() {
    let mut h = Host::new(shop());
    french(&mut h);
    let big = data(&[("value", Val::Money(100)), ("contents", Val::Text("x".repeat(3_600)))]);
    h.answer("forms", "filled", &big, None).unwrap_err();
    let (digest, faults) = (h.machine.digest(), h.machine.faults().len());
    assert_eq!(faults, 1);
    let (kernel_program, _) = kernel::program(h.prog.clone());
    let replayed = Machine::recover(kernel_program, h.machine.into_journal()).unwrap();
    assert_eq!((replayed.digest(), replayed.faults().len()), (digest, faults));
}

#[test]
fn an_answer_too_large_to_deliver_keeps_its_request() {
    let mut h = Host::new(shop());
    french(&mut h);
    let huge = data(&[("value", Val::Money(100)), ("contents", Val::Text("x".repeat(5_000)))]);
    let err = h.answer("forms", "filled", &huge, None).unwrap_err();
    assert!(err.contains("limit 4096"), "{err}");
    assert!(!h.trace.iter().any(|l| l.contains("xxxx")), "the trace must not say it was delivered");
    assert!(h.pending().iter().any(|a| a.adapter == "forms"));
    let fine = data(&[("value", Val::Money(100)), ("contents", Val::Text("books".into()))]);
    h.answer("forms", "filled", &fine, None).unwrap();
    assert!(h.trace.iter().any(|l| l.contains("Declaration filled")), "{:?}", h.trace);
}

fn japanese(h: &mut Host, label: &str) {
    let given = [("country".to_string(), Val::Text("Japan".into())), ("total".to_string(), Val::Money(8000))];
    h.create("Order", label, &given).unwrap();
}

#[test]
fn deadlines_resend_then_time_out_and_late_answers_are_ignored() {
    let mut h = Host::new(shop());
    japanese(&mut h, "J1");
    h.pass(30).unwrap();
    assert!(h.trace.iter().any(|l| l.contains("payments: no answer within 30 seconds")), "{:?}", h.trace);
    assert_eq!(h.pending().len(), 1, "sent again");
    h.pass(60).unwrap();
    assert!(h.trace.iter().any(|l| l.contains("Payment timed out")), "{:?}", h.trace);
    assert!(h.pending().is_empty());
    let before = h.unit("J1").unwrap().root;
    h.answer("payments", "captured", &[], None).unwrap();
    assert!(h.trace.iter().any(|l| l.contains("ignored a late answer for J1/Payment#1")), "{:?}", h.trace);
    assert_eq!(h.unit("J1").unwrap().root, before);
    assert!(h.unit("J1").unwrap().alerts.is_empty());
    h.pass(3 * 86_400).unwrap();
    assert_eq!(h.unit("J1").unwrap().alerts, vec![0]);
    assert!(h.machine.faults().is_empty(), "{:?}", h.machine.faults());
}

#[test]
fn a_timer_whose_reason_is_gone_fires_without_a_step() {
    let mut h = Host::new(shop());
    japanese(&mut h, "J1");
    h.answer("payments", "captured", &[], None).unwrap();
    h.answer("carriers", "delivered", &[], None).unwrap();
    let injects = h.machine.stats().injects;
    h.pass(30 * 86_400).unwrap();
    assert_eq!(h.machine.stats().injects, injects, "the answered deadlines and the met goal are dropped");
}

#[test]
fn moving_the_clock_costs_only_the_timers_due() {
    fn cost(idle: usize) -> u64 {
        let mut h = Host::new(shop());
        japanese(&mut h, "J1");
        h.pass(20).unwrap();
        for i in 0..idle {
            japanese(&mut h, &format!("I{i}"));
        }
        let s = h.machine.stats();
        let before = s.reductions + s.injects;
        h.pass(15).unwrap();
        let s = h.machine.stats();
        s.reductions + s.injects - before
    }
    let one = cost(1);
    assert!(one > 0, "J1's deadline fired");
    assert_eq!(cost(1_000), one);
}

#[test]
fn an_adapter_cannot_report_timed_out() {
    let mut h = Host::new(shop());
    japanese(&mut h, "J1");
    let err = h.answer("payments", "timed out", &[], None).unwrap_err();
    assert!(err.contains("only the runtime reports `timed out`"), "{err}");
    assert_eq!(h.pending().len(), 1);
}
