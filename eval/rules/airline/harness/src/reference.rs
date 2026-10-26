//! R: the reference answer, a rewrite of RuleArena's `compute_answer.py`
//! (commit 59f60f6b), which computes the answer each case is scored
//! against. Kept as close to the script as Rust allows, quirks included.

use crate::data::{Bag, Facts, Tables};

const OVERSIZE_150: [&str; 8] = ["Panama", "South America", "Peru", "Colombia", "Ecuador", "Europe", "Israel", "Qatar"];
const ASIA: [&str; 5] = ["India", "China", "Japan", "South Korea", "Hong Kong"];

pub fn oversize(b: &Bag, region: &str) -> i64 {
    let s = b.size();
    if s <= 62 {
        0
    } else if s <= 65 {
        30
    } else if OVERSIZE_150.contains(&region) {
        150
    } else {
        200
    }
}

/// The overweight fee; `complimentary` when the bag rides on a free
/// allowance. As in the script, a free bag to or from Australia or New
/// Zealand may weigh 70 lbs in any class, and elsewhere only in Business
/// and First.
pub fn overweight(b: &Bag, region: &str, class: &str, complimentary: bool) -> i64 {
    let w = b.weight;
    let far = region == "Australia" || region == "New Zealand";
    if (far || class == "Business" || class == "First") && complimentary {
        return if w <= 70 {
            0
        } else if ASIA.contains(&region) {
            450
        } else {
            200
        };
    }
    if w <= 50 {
        0
    } else if w <= 53 {
        30
    } else if w <= 70 {
        if region == "Cuba" { 200 } else { 100 }
    } else if ASIA.contains(&region) {
        450
    } else {
        200
    }
}

/// The total cost: the ticket and every checked bag's fee, or `None` when
/// the tables have no fee for a bag (a 3rd bag to Cuba).
pub fn total(f: &Facts, t: &Tables) -> Option<i64> {
    // The free allowances go to the bags they save most on: sorted by
    // that saving, most first, as the script does before it matches bags
    // to the fee table's rows. Ties keep their order; the total is the same
    // either way, since tied bags save the same.
    let violation = |b: &Bag, comp: bool| oversize(b, &f.region).max(overweight(b, &f.region, &f.class, comp));
    let mut bags: Vec<&Bag> = f.bags.iter().collect();
    bags.sort_by_key(|b| -(violation(b, false) - violation(b, true)));
    let mut sum = f.ticket;
    for (i, b) in bags.iter().enumerate() {
        let base = t.fee(i + 1, f)?;
        sum += base + violation(b, base == 0);
    }
    Some(sum)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::Way;

    fn tables() -> Tables {
        Tables::load(&crate::data_dir()).unwrap()
    }

    fn bag(l: i64, w: i64, h: i64, weight: i64) -> Bag {
        Bag { length: l, width: w, height: h, weight }
    }

    #[test]
    fn the_one_shot_example_of_rulearena() {
        // auto_test.py's example: Main Plus, Orlando to Lima, $448; its
        // worked answer is $1198.
        let f = Facts {
            class: "Main Plus".into(),
            region: "Peru".into(),
            way: Way::From,
            ticket: 448,
            bags: vec![bag(51, 24, 27, 65), bag(46, 29, 24, 85), bag(35, 12, 11, 44), bag(30, 11, 10, 33)],
        };
        assert_eq!(total(&f, &tables()), Some(1198));
    }

    #[test]
    fn business_free_bags_go_where_they_save_most() {
        // U.S., Business: two free bags (70 lbs allowed), then $150.
        // A: 100 in, 60 lbs: oversize 200 either way, saves 0.
        // B: 50 in, 60 lbs: 100 overweight paid, 0 free, saves 100.
        // C: 50 in, 52 lbs: 30 paid, 0 free, saves 30.
        // Free: B and C; A is third: 150 + 200.
        let f = Facts { class: "Business".into(), region: "U.S.".into(), way: Way::From, ticket: 100, bags: vec![bag(50, 30, 20, 60), bag(20, 20, 10, 60), bag(20, 20, 10, 52)] };
        assert_eq!(total(&f, &tables()), Some(100 + 350));
    }

    #[test]
    fn australia_frees_the_allowance_in_main_cabin() {
        // Main Cabin to Sydney: 1st bag free, so it may weigh 70 lbs; the
        // 2nd is $100 and pays 100 for 60 lbs.
        let f = Facts { class: "Main Cabin".into(), region: "Australia".into(), way: Way::To, ticket: 500, bags: vec![bag(20, 20, 10, 60), bag(20, 20, 10, 60)] };
        assert_eq!(total(&f, &tables()), Some(500 + 100 + 100));
    }

    #[test]
    fn heavy_asian_bags_and_the_fourth_row() {
        // Basic Economy to Tokyo: 75, 100, 200, 200; 80 lbs is 450 each.
        let b = bag(20, 20, 10, 80);
        let f = Facts { class: "Basic Economy".into(), region: "Japan".into(), way: Way::From, ticket: 0, bags: vec![b.clone(), b.clone(), b.clone(), b.clone(), b] };
        assert_eq!(total(&f, &tables()), Some(75 + 100 + 200 + 200 + 200 + 5 * 450));
    }

    #[test]
    fn no_fee_for_a_third_bag_to_cuba() {
        let b = bag(20, 20, 10, 40);
        let f = Facts { class: "Main Cabin".into(), region: "Cuba".into(), way: Way::From, ticket: 0, bags: vec![b.clone(), b.clone(), b] };
        assert_eq!(total(&f, &tables()), None);
    }
}
