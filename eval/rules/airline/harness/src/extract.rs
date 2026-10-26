//! Extracted facts: a decision model reads the case and answers typed
//! questions, each a choice among fixed options, never free text. The trip
//! comes first (class, region, way, ticket and the number of items), then
//! each checked item's three dimensions and weight. A number is a choice
//! among the numbers the case's text holds, as span extraction is.

use crate::data::{Bag, CLASSES, Facts, REGIONS, Way};
use serde_json::{Map, Value, json};

const MAX_ITEMS: usize = 12;

/// Every whole number in the text, each once, in order of value.
pub fn numbers(text: &str) -> Vec<i64> {
    let mut out: Vec<i64> = text.split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty()).filter_map(|s| s.parse().ok()).collect();
    out.sort();
    out.dedup();
    out
}

fn number_question(instructions: String, options: &[i64]) -> Value {
    let criteria: Map<String, Value> = options.iter().map(|n| (n.to_string(), json!(n.to_string()))).collect();
    json!({"type": "choice", "instructions": instructions, "criteria": criteria})
}

/// What a region option says: the fee tables' name, and what it covers
/// where the name alone does not say.
fn region_option(r: &str) -> String {
    match r {
        "U.S." => "U.S.: both ends of the trip are in the U.S.".into(),
        "Europe" => "Europe".into(),
        "South America" => "South America, other than Colombia, Ecuador and Peru".into(),
        r => r.into(),
    }
}

pub fn trip_questions(text: &str) -> Value {
    let n = numbers(text);
    let classes: Map<String, Value> = CLASSES.iter().enumerate().map(|(i, c)| (format!("class{i}"), json!(c))).collect();
    let regions: Map<String, Value> = REGIONS.iter().enumerate().map(|(i, r)| (format!("region{i}"), json!(region_option(r)))).collect();
    let counts: Map<String, Value> = (1..=MAX_ITEMS).map(|k| (k.to_string(), json!(k.to_string()))).collect();
    json!({
        "class": {"type": "choice", "instructions": "Which cabin class does the passenger fly in?", "criteria": classes},
        "region": {"type": "choice", "instructions": "The trip goes between the U.S. and one region. Which region is the end of the trip outside the U.S. in?", "criteria": regions},
        "way": {"type": "choice", "instructions": "Does the trip start in the U.S. or end in the U.S.?", "criteria": {"from": "it starts in the U.S. (or both ends are in the U.S.)", "to": "it starts outside the U.S. and ends in the U.S."}},
        "ticket": number_question("What does the flight ticket cost, in dollars?".into(), &n),
        "count": {"type": "choice", "instructions": "How many numbered items does the passenger have?", "criteria": counts},
    })
}

/// The questions for items 2 to `count`; item 1, the carry-on, is never
/// charged.
pub fn item_questions(text: &str, count: usize) -> Value {
    let n = numbers(text);
    let mut q = Map::new();
    for i in 2..=count.min(MAX_ITEMS) {
        for (key, what) in [("length", "the first of its three dimensions, in inches"), ("width", "the second of its three dimensions, in inches"), ("height", "the third of its three dimensions, in inches"), ("weight", "its weight, in lbs")] {
            q.insert(format!("{key}{i}"), number_question(format!("For item {i}: what is {what}?"), &n));
        }
    }
    Value::Object(q)
}

fn pick<'a>(picks: &'a Value, q: &str) -> Result<&'a str, String> {
    picks[q].as_str().ok_or(format!("no answer to {q}"))
}

fn num(picks: &Value, q: &str) -> Result<i64, String> {
    pick(picks, q)?.parse().map_err(|e| format!("{q}: {e}"))
}

/// The number of items the trip's answers give.
pub fn count(trip: &Value) -> Result<usize, String> {
    Ok(num(trip, "count")? as usize)
}

/// The facts the two sets of answers give.
pub fn facts(trip: &Value, items: &Value) -> Result<Facts, String> {
    let index = |q: &str, prefix: &str| -> Result<usize, String> { pick(trip, q)?.strip_prefix(prefix).and_then(|i| i.parse().ok()).ok_or(format!("{q}: not an option")) };
    let class = CLASSES.get(index("class", "class")?).ok_or("class: not an option")?.to_string();
    let region = REGIONS.get(index("region", "region")?).ok_or("region: not an option")?.to_string();
    let way = match pick(trip, "way")? {
        "from" => Way::From,
        "to" => Way::To,
        w => return Err(format!("way: {w}")),
    };
    let mut bags = vec![];
    for i in 2..=count(trip)?.min(MAX_ITEMS) {
        bags.push(Bag { length: num(items, &format!("length{i}"))?, width: num(items, &format!("width{i}"))?, height: num(items, &format!("height{i}"))?, weight: num(items, &format!("weight{i}"))? });
    }
    Ok(Facts { class, region, way, ticket: num(trip, "ticket")?, bags })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The answers a perfect reader would give.
    fn perfect(f: &Facts, items: usize) -> (Value, Value) {
        let trip = json!({
            "class": format!("class{}", CLASSES.iter().position(|c| *c == f.class).unwrap()),
            "region": format!("region{}", REGIONS.iter().position(|r| *r == f.region).unwrap()),
            "way": f.way.name(),
            "ticket": f.ticket.to_string(),
            "count": items.to_string(),
        });
        let mut it = Map::new();
        for (k, b) in f.bags.iter().enumerate() {
            let i = k + 2;
            it.insert(format!("length{i}"), json!(b.length.to_string()));
            it.insert(format!("width{i}"), json!(b.width.to_string()));
            it.insert(format!("height{i}"), json!(b.height.to_string()));
            it.insert(format!("weight{i}"), json!(b.weight.to_string()));
        }
        (trip, Value::Object(it))
    }

    /// Every true value is one of the options, so a perfect reader gets
    /// the true facts back, on every case.
    #[test]
    fn a_perfect_reader_gets_the_true_facts() {
        for c in crate::data::cases(&crate::data_dir()).unwrap() {
            let items = c.facts.bags.len() + 1;
            let (trip, it) = perfect(&c.facts, items);
            let tq = trip_questions(&c.text);
            for q in ["class", "region", "way", "ticket", "count"] {
                assert!(tq[q]["criteria"].get(trip[q].as_str().unwrap()).is_some(), "{}: {q}", c.id);
            }
            let iq = item_questions(&c.text, items);
            for (q, a) in it.as_object().unwrap() {
                assert!(iq[q]["criteria"].get(a.as_str().unwrap()).is_some(), "{}: {q}", c.id);
            }
            assert_eq!(facts(&trip, &it).unwrap(), c.facts, "{}", c.id);
        }
    }

    #[test]
    fn numbers_are_each_once() {
        assert_eq!(numbers("1. A backpack: 22 x 13 x 6 inches, 10 lbs; 2. 13 x 22"), vec![1, 2, 6, 10, 13, 22]);
    }
}
