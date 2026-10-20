//! Slice 27: shares of an amount, negative amounts and dates (spec appendix AB).

use suture_lang::scenario::parse_scenarios;
use suture_runtime::scenario::{run, Report};

const SHOP: &str = "module shop version 1, currency EUR
role staff
kind Order { promo: Money = 0 EUR, items: Money, opens: Time = 2026-03-01 }
kind Unit  { order: Order, price: Money,
             share: Money = price * order's promo / order's items,
             amount: Money = price + share }
define: an Order is late when its created is at least its opens
admit add a new Order by staff
admit add a new Unit by staff
";

fn scenario(src: &str, body: &str) -> Report {
    let p = suture_lang::compile(&[("t.st".to_string(), src.to_string())]).unwrap_or_else(|e| panic!("{e:?}"));
    let src = format!("scenario \"s\":\n{body}");
    let sc = &parse_scenarios("t.scenario", &src).unwrap_or_else(|e| panic!("{e}"))[0];
    run(&p, sc)
}

fn passes(src: &str, body: &str) -> Report {
    let r = scenario(src, body);
    assert!(r.passed(), "{:?}\n{}", r.failures, r.host.trace.join("\n"));
    r
}

#[test]
fn a_share_of_an_amount_is_rounded_half_away_from_zero() {
    passes(
        SHOP,
        "  given  a Order O1 with promo -10 EUR and items 30 EUR
  and    a Unit U1 with order O1 and price 10 EUR
  and    a Unit U2 with order O1 and price 5 EUR
  and    a Unit U3 with order O1 and price 0.05 EUR
  expect U1's share is -3.33 EUR
  expect U1's amount is 6.67 EUR
  expect U2's share is -1.67 EUR
  expect U3's share is -0.02 EUR
",
    );
}

#[test]
fn a_share_of_nothing_is_unknown() {
    let r = scenario(
        SHOP,
        "  given  a Order O1 with promo -10 EUR and items 0 EUR
  and    a Unit U1 with order O1 and price 10 EUR
  expect U1's share is 0 EUR
",
    );
    assert!(!r.passed());
}

#[test]
fn the_clock_moves_to_a_date() {
    let r = passes(
        SHOP,
        "  given  a Order O1 with items 1 EUR
  when   it is 2026-02-01 12:30
  given  a Order O2 with items 1 EUR and opens 2026-02-01 12:30
  and    a Order O3 with items 1 EUR
  when   it is 2026-03-01
  given  a Order O4 with items 1 EUR
  expect O1 is not late
  expect O2 is late
  expect O3 is not late
  expect O4 is late
  expect O4's opens is 2026-03-01
",
    );
    assert!(r.host.trace.iter().any(|l| l == "  … it is 2026-02-01 12:30"), "{}", r.host.trace.join("\n"));
}

#[test]
fn the_clock_does_not_go_back() {
    let r = scenario(SHOP, "  when   it is 2026-03-01\n  and    it is 2026-02-01\n");
    assert_eq!(r.failures, vec!["step 2: it is already past 2026-02-01: the clock only moves forward".to_string()]);
}
