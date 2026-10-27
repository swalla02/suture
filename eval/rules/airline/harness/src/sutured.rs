//! S1: the answer of eval/rules/airline/suture/airline.st, run by the Suture
//! runtime. The harness enters the fee table and a case's facts, in the
//! order the module's header gives, and adds the ticket to each bag's `fee`,
//! a value the module defines. It decides nothing: every fee, and which bags
//! ride free, is the module's.

use crate::data::{Facts, REGIONS, Tables};
use suture_lang::ir::Program;
use suture_runtime::host::Host;
use suture_runtime::state::Val;
use std::path::Path;

pub fn module_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../suture")
}

pub fn compile(dir: &Path) -> Result<Program, String> {
    suture_lang::compile_dir(dir).map_err(|e| e.iter().map(|e| format!("{}:{}: {}", e.file, e.line, e.msg)).collect::<Vec<_>>().join("\n"))
}

fn region_label(name: &str) -> Option<String> {
    REGIONS.iter().position(|r| *r == name).map(|i| format!("Region{i}"))
}

fn text(s: &str) -> Val {
    Val::Text(s.to_string())
}

fn dollars(d: i64) -> Val {
    Val::Money(d * 100)
}

/// One run of the module over many cases: the tables are entered once.
pub struct Run {
    host: Host,
}

impl Run {
    pub fn new(prog: Program, t: &Tables) -> Result<Run, String> {
        let mut host = Host::new(prog);
        for (i, r) in REGIONS.iter().enumerate() {
            host.create("Region", &format!("Region{i}"), &[("name".into(), text(r))])?;
        }
        for (i, ((nth, way, class, region), fee)) in t.base.iter().enumerate() {
            let region = region_label(region).ok_or(format!("a fee table names region {region}"))?;
            let attrs = [
                ("region".into(), Val::Ref(region)),
                ("class".into(), text(class)),
                ("way".into(), text(way.name())),
                ("nth".into(), Val::Count(i64::from(*nth))),
                ("amount".into(), dollars(*fee)),
            ];
            host.create("Rate", &format!("Rate{i}"), &attrs)?;
        }
        Ok(Run { host })
    }

    /// The total the module gives the case, or why it gives none. `id` must
    /// be new to this run.
    pub fn total(&mut self, id: &str, f: &Facts) -> Result<i64, String> {
        let h = &mut self.host;
        let region = region_label(&f.region).ok_or(format!("no region {}", f.region))?;
        let trip = format!("{id}T");
        let attrs = [("region".into(), Val::Ref(region)), ("class".into(), text(&f.class)), ("way".into(), text(f.way.name())), ("ticket".into(), dollars(f.ticket))];
        h.create("Trip", &trip, &attrs)?;
        let mut bags = vec![];
        for (i, b) in f.bags.iter().enumerate() {
            let label = format!("{id}B{}", i + 1);
            let attrs = [
                ("trip".into(), Val::Ref(trip.clone())),
                ("nth".into(), Val::Count(i as i64 + 1)),
                ("length".into(), Val::Count(b.length)),
                ("width".into(), Val::Count(b.width)),
                ("height".into(), Val::Count(b.height)),
                ("weight".into(), Val::Count(b.weight)),
            ];
            h.create("Bag", &label, &attrs)?;
            bags.push(label);
        }
        // Three Allowances, whatever the case: the policy frees up to 3 bags.
        for n in 1..=3 {
            h.create("Allowance", &format!("{id}A{n}"), &[("trip".into(), Val::Ref(trip.clone())), ("nth".into(), Val::Count(n))])?;
        }
        let mut sum = f.ticket * 100;
        for b in &bags {
            match h.value(b, &[], "fee") {
                Some(Val::Money(c)) => sum += c,
                // A bag with no row of the fee table: a 3rd bag to Cuba.
                _ => return Err(format!("{b} has no fee")),
            }
        }
        if sum % 100 != 0 {
            return Err(format!("{sum} cents"));
        }
        Ok(sum / 100)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{data, reference};

    fn run() -> Run {
        let t = Tables::load(&crate::data_dir()).unwrap();
        Run::new(compile(&module_dir()).unwrap(), &t).unwrap()
    }

    #[test]
    fn the_one_shot_example_of_rulearena() {
        let f = Facts {
            class: "Main Plus".into(),
            region: "Peru".into(),
            way: data::Way::From,
            ticket: 448,
            bags: [(51, 24, 27, 65), (46, 29, 24, 85), (35, 12, 11, 44), (30, 11, 10, 33)].iter().map(|&(l, w, h, g)| data::Bag { length: l, width: w, height: h, weight: g }).collect(),
        };
        assert_eq!(run().total("x", &f), Ok(1198));
    }

    #[test]
    fn the_scenarios_pass() {
        let prog = compile(&module_dir()).unwrap();
        let text = std::fs::read_to_string(module_dir().join("airline.scenario")).unwrap();
        let scenarios = suture_lang::scenario::parse_scenarios("airline.scenario", &text).unwrap();
        assert_eq!(scenarios.len(), 5);
        for sc in &scenarios {
            let r = suture_runtime::scenario::run(&prog, sc);
            assert!(r.passed(), "{}: {:?}", sc.title, r.failures);
        }
    }

    /// H2's first half, on every case: with true facts the module gives
    /// the reference's answer.
    #[test]
    fn suture_matches_the_reference_on_every_case() {
        let t = Tables::load(&crate::data_dir()).unwrap();
        let mut r = run();
        let mut wrong = vec![];
        for c in data::cases(&crate::data_dir()).unwrap() {
            let want = reference::total(&c.facts, &t).unwrap();
            let got = r.total(&c.id, &c.facts);
            if got != Ok(want) {
                wrong.push(format!("{}: {got:?}, reference {want}", c.id));
            }
        }
        assert!(wrong.is_empty(), "{} of 300 differ:\n{}", wrong.len(), wrong.join("\n"));
    }

    /// The cases leave most of the policy untried (every checked bag in
    /// them is oversize), so random trips cross-check the rest: every
    /// region and class, and bags of any size and weight, including those
    /// the policy text does not accept and the script charges anyway.
    #[test]
    fn suture_matches_the_reference_on_random_trips() {
        let t = Tables::load(&crate::data_dir()).unwrap();
        let mut r = run();
        let mut seed: u64 = 2026;
        let mut next = |n: u64| {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 33) % n
        };
        let mut wrong = vec![];
        for i in 0..400 {
            let region = REGIONS[next(21) as usize];
            let class = data::CLASSES[next(6) as usize];
            let n = if region == "Cuba" { 1 + next(2) } else { 1 + next(10) };
            let bags = (0..n)
                .map(|_| {
                    let weight = 30 + next(101) as i64;
                    let size = 45 + next(101) as i64;
                    data::Bag { length: size / 2, width: size / 3, height: size - size / 2 - size / 3, weight }
                })
                .collect();
            let way = if next(2) == 0 { data::Way::From } else { data::Way::To };
            let f = Facts { class: class.into(), region: region.into(), way, ticket: next(1000) as i64, bags };
            let want = reference::total(&f, &t).unwrap();
            let got = r.total(&format!("r{i}"), &f);
            if got != Ok(want) {
                wrong.push(format!("{f:?}: {got:?}, reference {want}"));
            }
        }
        assert!(wrong.is_empty(), "{} of 400 differ:\n{}", wrong.len(), wrong.join("\n"));
    }
}
