//! RuleArena's airline cases and fee tables (eval/rules/airline/data), and
//! the facts of a case: what the generator drew, which is what the case's
//! text says.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

pub const CLASSES: [&str; 6] = ["Basic Economy", "Main Cabin", "Main Plus", "Premium Economy", "Business", "First"];

/// The regions of the fee tables, in their order.
pub const REGIONS: [&str; 21] = [
    "U.S.", "Puerto Rico", "Canada", "Mexico", "Cuba", "Haiti", "Panama", "Colombia", "Ecuador", "Peru", "South America", "Israel", "Qatar", "Europe", "India", "China", "Japan",
    "South Korea", "Hong Kong", "Australia", "New Zealand",
];

/// Which way the trip crosses: from the U.S. to the region, or back.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Way {
    From,
    To,
}

impl Way {
    pub fn name(self) -> &'static str {
        match self {
            Way::From => "from",
            Way::To => "to",
        }
    }
}

/// A checked bag: its outside dimensions in inches and its weight in lbs.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Bag {
    pub length: i64,
    pub width: i64,
    pub height: i64,
    pub weight: i64,
}

impl Bag {
    pub fn size(&self) -> i64 {
        self.length + self.width + self.height
    }
}

/// What a fee depends on. The first item of every case is the carry-on,
/// and RuleArena's answer never charges it, so `bags` are the others, in
/// the order the case lists them.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Facts {
    pub class: String,
    pub region: String,
    pub way: Way,
    pub ticket: i64,
    pub bags: Vec<Bag>,
}

#[derive(Clone, Debug)]
pub struct Case {
    /// `c⟨level⟩-⟨line⟩`, the line from 0 in `comp_⟨level⟩.jsonl`.
    pub id: String,
    pub level: u8,
    pub text: String,
    pub facts: Facts,
}

#[derive(Deserialize)]
struct Line {
    prompt: String,
    info: Info,
}

#[derive(Deserialize)]
struct Info {
    base_price: i64,
    customer_class: String,
    routine: String,
    direction: u8,
    bag_list: Vec<Item>,
}

#[derive(Deserialize)]
struct Item {
    size: [i64; 3],
    weight: i64,
}

/// The 300 cases, level 0 to 2.
pub fn cases(data: &Path) -> Result<Vec<Case>, String> {
    let mut out = Vec::new();
    for level in 0..3u8 {
        let p = data.join(format!("cases/comp_{level}.jsonl"));
        let text = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        for (i, l) in text.lines().enumerate() {
            let l: Line = serde_json::from_str(l).map_err(|e| format!("{}:{}: {e}", p.display(), i + 1))?;
            let way = match l.info.direction {
                0 => Way::From,
                1 => Way::To,
                d => return Err(format!("{}:{}: direction {d}", p.display(), i + 1)),
            };
            let bags = l.info.bag_list.iter().skip(1).map(|b| Bag { length: b.size[0], width: b.size[1], height: b.size[2], weight: b.weight }).collect();
            let facts = Facts { class: l.info.customer_class, region: l.info.routine, way, ticket: l.info.base_price, bags };
            out.push(Case { id: format!("c{level}-{i:03}"), level, text: l.prompt, facts });
        }
    }
    Ok(out)
}

/// The checked-bag fee tables: the fee of the `nth` bag (1 to 4, the 4th
/// standing for every later one), by way, class and region.
#[derive(Clone, Debug, Default)]
pub struct Tables {
    pub base: BTreeMap<(u8, Way, String, String), i64>,
}

impl Tables {
    pub fn load(data: &Path) -> Result<Tables, String> {
        let mut base = BTreeMap::new();
        for nth in 1..=4u8 {
            for (file, way) in [("0.csv", Way::From), ("1.csv", Way::To)] {
                let p = data.join(format!("fee_tables/bag_{nth}/{file}"));
                let text = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
                let mut lines = text.trim_start_matches('\u{feff}').lines();
                let head: Vec<&str> = lines.next().ok_or("empty table")?.split(',').skip(1).collect();
                for l in lines {
                    let cells: Vec<&str> = l.split(',').collect();
                    for (class, fee) in head.iter().zip(&cells[1..]) {
                        let fee = fee.trim().parse().map_err(|e| format!("{}: {e}", p.display()))?;
                        base.insert((nth, way, class.to_string(), cells[0].to_string()), fee);
                    }
                }
            }
        }
        Ok(Tables { base })
    }

    /// The fee of the `nth` checked bag, from 1.
    pub fn fee(&self, nth: usize, f: &Facts) -> Option<i64> {
        self.base.get(&(nth.min(4) as u8, f.way, f.class.clone(), f.region.clone())).copied()
    }
}
