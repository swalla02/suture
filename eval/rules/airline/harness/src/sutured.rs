//! S1: the answer of eval/rules/airline/suture/airline.st, run by the Suture
//! runtime. The harness enters the tables and a case's facts, in the order
//! the module's header gives, and adds up the amounts of the rows granted.
//! It decides nothing: every fee, and which bags ride free, is a choice the
//! module makes.

use crate::data::{Facts, REGIONS, Tables};
use suture_lang::ir::Program;
use suture_lang::scenario::Caller;
use suture_runtime::host::Host;
use suture_runtime::state::{Bind, Node, Val};
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
    clerk: Caller,
}

impl Run {
    pub fn new(prog: Program, t: &Tables, charges: &str) -> Result<Run, String> {
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
        let mut n = 0;
        for l in charges.lines().skip(1).filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
            let c: Vec<&str> = l.split(',').collect();
            let [kind, regions, lowsize, highsize, lowweight, highweight, amount] = c[..] else { return Err(format!("charges.csv: {l}")) };
            let regions: Vec<&str> = if regions == "all" { REGIONS.to_vec() } else { regions.split('|').collect() };
            let num = |s: &str| s.parse::<i64>().map_err(|e| format!("charges.csv: {s}: {e}"));
            // One row per region of the group: a granted row is carried into
            // the bag, with what it refers to, and 21 regions would not fit.
            for r in regions {
                let attrs = [
                    ("region".into(), Val::Ref(region_label(r).ok_or(format!("charges.csv names region {r}"))?)),
                    ("lowsize".into(), Val::Count(num(lowsize)?)),
                    ("highsize".into(), Val::Count(num(highsize)?)),
                    ("lowweight".into(), Val::Count(num(lowweight)?)),
                    ("highweight".into(), Val::Count(num(highweight)?)),
                    ("amount".into(), dollars(num(amount)?)),
                ];
                host.create(kind, &format!("{kind}{n}"), &attrs)?;
                n += 1;
            }
        }
        Ok(Run { host, clerk: Caller { role: "clerk".into(), name: "desk".into(), verb: "weighs".into() } })
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
        for b in &bags {
            h.input_event(Some(&self.clerk), b, None, &[])?;
        }
        let mut sum = f.ticket * 100;
        for b in &bags {
            let u = self.host.unit(b).ok_or("no bag")?;
            let base = u.root.parts.iter().find_map(|p| self.granted(p, "Rate"));
            sum += base.ok_or(format!("{b} has no checked-bag fee row"))?;
            let got: Vec<i64> = u.root.parts.iter().filter_map(|p| self.granted(p, "Charge").or_else(|| self.granted(p, "Grace"))).collect();
            match got[..] {
                [fee] => sum += fee,
                [] => return Err(format!("{b} has no overweight or oversize row")),
                _ => return Err(format!("{b} has {} rows", got.len())),
            }
        }
        if sum % 100 != 0 {
            return Err(format!("{sum} cents"));
        }
        Ok(sum / 100)
    }

    /// The amount, in cents, of the row a part's binding was granted.
    fn granted(&self, part: &Node, bind: &str) -> Option<i64> {
        let Some(Bind::Granted { to, .. }) = part.binds.get(bind) else { return None };
        match self.host.unit(to)?.root.attrs.get("amount") {
            Some(Val::Money(c)) => Some(*c),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{data, reference};

    fn run() -> Run {
        let t = Tables::load(&crate::data_dir()).unwrap();
        let charges = std::fs::read_to_string(module_dir().join("charges.csv")).unwrap();
        Run::new(compile(&module_dir()).unwrap(), &t, &charges).unwrap()
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
    /// region and class, bags of any size and weight the policy accepts.
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
            let refuses_heavy = ["Europe", "Israel", "Qatar", "Australia", "New Zealand"].contains(&region);
            let n = if region == "Cuba" { 1 + next(2) } else { 1 + next(10) };
            let bags = (0..n)
                .map(|_| {
                    let weight = 30 + next(if refuses_heavy { 41 } else { 71 }) as i64;
                    let size = 45 + next(71) as i64;
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
