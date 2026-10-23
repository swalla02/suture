//! A machine's image: what a snapshot keeps (spec appendix BG.2.1).

mod common;

use common::*;
use suture::*;

#[test]
fn a_machine_from_its_image_goes_on_as_the_original() {
    let m0 = mini();
    let mut m = Machine::new(m0.prog.clone());
    m.inject(sum(&m0, 2, 3, "a").key("k1")).unwrap();
    assert_eq!(m.run(Some(2)), RunOutcome::Budget { reductions: 2 });
    let text = serde_json::to_string(&m.image()).unwrap();
    let mut r = Machine::from_image(m0.prog.clone(), serde_json::from_str(&text).unwrap(), Box::new(NoJournal)).unwrap();
    assert_eq!((r.digest(), r.ready_count(), r.image()), (m.digest(), m.ready_count(), m.image()));
    // It goes on the same way, and knows the keys it took.
    for x in [&mut m, &mut r] {
        x.run(None);
        assert!(x.inject(sum(&m0, 2, 3, "a").key("k1")).unwrap().duplicate);
        x.inject(start_go(&m0, "out")).unwrap();
        x.run(None);
    }
    assert_eq!((r.digest(), r.outbox(), r.stats().reductions), (m.digest(), m.outbox(), m.stats().reductions));
}

#[test]
fn an_image_that_breaks_the_graph_is_refused() {
    let m0 = mini();
    let mut m = Machine::new(m0.prog.clone());
    m.inject(sum(&m0, 2, 3, "a")).unwrap();
    let mut v = serde_json::to_value(m.image()).unwrap();
    v["holes"] = serde_json::json!([]);
    assert!(Machine::from_image(m0.prog.clone(), serde_json::from_value(v).unwrap(), Box::new(NoJournal)).is_err());
}
