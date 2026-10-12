use suture::nets::session::{self, simulate, ClientOutcome, NetConfig, SessionConfig};

fn check_safety(seed: u64, items: &[i64], r: &session::SessionReport) {
    match &r.client {
        ClientOutcome::Ok => assert_eq!(r.delivered, items, "seed {seed}: closed cleanly but delivery incomplete"),
        ClientOutcome::Aborted(_) => {
            assert!(items.starts_with(&r.delivered), "seed {seed}: delivered {:?} is not a prefix", r.delivered)
        }
    }
}

#[test]
fn coverage_is_complete() {
    let (p, _) = session::program(&SessionConfig { max_tries: 3 });
    let c = p.rules.coverage(&p.sig);
    assert!(c.missing.is_empty(), "{:?}", c.missing);
    assert!(c.never_fires.is_empty(), "{:?}", c.never_fires);
}

#[test]
fn perfect_network_delivers_everything() {
    let items = vec![1, 2, 3];
    let r = simulate(&NetConfig { seed: 1, drop: 0.0, dup: 0.0, adversarial: 0.0, items: items.clone(), max_tries: 4 })
        .unwrap();
    assert_eq!(r.client, ClientOutcome::Ok);
    assert_eq!(r.delivered, items);
    assert_eq!(r.rejects, 0);
}

#[test]
fn total_loss_aborts_after_max_tries() {
    let r = simulate(&NetConfig { seed: 3, drop: 1.0, dup: 0.0, adversarial: 0.0, items: vec![5], max_tries: 4 }).unwrap();
    assert_eq!(r.client, ClientOutcome::Aborted("connect".into()));
    assert_eq!(r.transmitted.get("Open"), Some(&4));
    assert!(r.delivered.is_empty());
}

#[test]
fn lossy_networks_preserve_exactly_once_in_order() {
    let items = vec![10, 20, 30, 40];
    let (mut ok, mut aborted) = (0, 0);
    for seed in 0..300 {
        let cfg = NetConfig { seed, drop: 0.3, dup: 0.2, adversarial: 0.0, items: items.clone(), max_tries: 6 };
        let r = simulate(&cfg).unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        check_safety(seed, &items, &r);
        match r.client {
            ClientOutcome::Ok => ok += 1,
            ClientOutcome::Aborted(_) => aborted += 1,
        }
    }
    assert!(ok > 200, "ok {ok}, aborted {aborted}");
}

#[test]
fn adversarial_messages_are_never_accepted() {
    let items = vec![7, 8, 9];
    let mut rejects = 0;
    for seed in 0..200 {
        let cfg = NetConfig { seed, drop: 0.2, dup: 0.2, adversarial: 0.3, items: items.clone(), max_tries: 6 };
        let r = simulate(&cfg).unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        check_safety(seed, &items, &r);
        rejects += r.rejects;
    }
    assert!(rejects > 100, "only {rejects} rejections recorded");
}

#[test]
fn quiescence_is_explained() {
    let r = simulate(&NetConfig { seed: 5, drop: 0.2, dup: 0.1, adversarial: 0.0, items: vec![1, 2], max_tries: 5 }).unwrap();
    let text = &r.explanation;
    assert!(text.contains("waiting on hole 'c:1'") && text.contains("waiting on hole 's:1'"), "{text}");
    assert!(!text.contains("STUCK") && !text.contains("DEADLOCK"), "{text}");
}
