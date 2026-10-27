//! The variants of each case (the experiment spec, stage 2 step 1): the
//! same facts, `reworded` as prose and with `noise` added, both written by
//! the prompt LLM, and in another `order`, made here. The carry-on stays
//! the first item, since the answer never charges the first item, so a
//! variant's true answer is its case's. A variant is kept only when
//! `check` finds every fact in it.

use crate::data::Case;
use siphasher::sip::SipHasher24;
use std::collections::BTreeMap;
use std::hash::Hasher;
use std::path::PathBuf;

pub const KINDS: [&str; 3] = ["reworded", "noise", "order"];

/// What a case's text says, read from RuleArena's one template.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Parts {
    pub name: String,
    pub class: String,
    pub from: String,
    pub to: String,
    /// Each item as the case writes it, `A backpack: 22 x 13 x 6 inches, 10 lbs`.
    pub items: Vec<String>,
    /// Each item's three dimensions and weight.
    pub numbers: Vec<[i64; 4]>,
    pub ticket: i64,
}

fn between<'a>(s: &'a str, a: &str, b: &str) -> Option<&'a str> {
    let s = &s[s.find(a)? + a.len()..];
    Some(&s[..s.find(b)?])
}

pub fn parts(text: &str) -> Option<Parts> {
    let (head, rest) = text.split_once(" with the following items:\n")?;
    let (name, head) = head.split_once(" is a")?;
    let class = between(head, " ", " Class passenger flying from ")?.to_string();
    let (from, to) = head.split_once(" Class passenger flying from ")?.1.split_once(" to ")?;
    let mut items = vec![];
    let mut numbers = vec![];
    for line in rest.lines().take_while(|l| !l.is_empty()) {
        let item = line.split_once(". ")?.1.strip_suffix(';')?;
        let n: Vec<i64> = item.split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty()).filter_map(|s| s.parse().ok()).collect();
        numbers.push(<[i64; 4]>::try_from(n).ok()?);
        items.push(item.to_string());
    }
    let ticket = between(text, &format!("{name}'s flight ticket is $"), ".")?.parse().ok()?;
    Some(Parts { name: name.into(), class, from: from.into(), to: to.into(), items, numbers, ticket })
}

/// The `order` variant: the ticket first, then the class and the route,
/// then the items, the checked bags shuffled by a hash of the case id.
pub fn order(c: &Case) -> Option<String> {
    let p = parts(&c.text)?;
    let mut checked: Vec<&String> = p.items.iter().skip(1).collect();
    let mut h = SipHasher24::new_with_keys(0x2026_1027, 0x0_7de2);
    h.write(c.id.as_bytes());
    let mut seed = h.finish();
    for i in (1..checked.len()).rev() {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        checked.swap(i, (seed >> 33) as usize % (i + 1));
    }
    let mut out = format!("{}'s flight ticket is ${}. {} flies {} Class, from {} to {}, and brings these items:\n", p.name, p.ticket, p.name, p.class, p.from, p.to);
    for (i, item) in std::iter::once(&p.items[0]).chain(checked).enumerate() {
        out.push_str(&format!("{}. {item};\n", i + 1));
    }
    Some(out)
}

pub const SYSTEM: &str = "You rewrite short texts exactly as asked.";

pub fn reworded_prompt(text: &str, class: &str) -> String {
    format!(
        "Rewrite this description of an airline passenger as one or two paragraphs of natural prose, with no list. Keep every fact exactly: the passenger's name; the class, written as \"{class} Class\"; the two cities; and every item, in the same order, with its kind, its three dimensions in inches and its weight in lbs, all written as digits; and the ticket price, written as $ and digits. Do not number the items, but make clear which item comes first, second and so on. Add nothing and leave nothing out. Answer with the rewritten text alone.\n\n{text}"
    )
}

pub fn noise_prompt(text: &str) -> String {
    format!(
        "Add 3 to 5 sentences of everyday detail to this description of an airline passenger, placed between its sentences or between its items: for instance why they travel, what is packed in an item, the weather, a flight number or a departure time. The details must have nothing to do with any fee: no money or prices, no other bag or item, no pet, no service or upgrade, no change of class or route, and no size or weight. Keep every line of the original text exactly as it is. Answer with the new text alone.\n\n{text}"
    )
}

/// `text` with each Unicode space and hyphen a plain one: the model writes
/// `San Francisco` with a narrow no-break space, and `Port-au-Prince` with
/// no-break hyphens, as often as not.
pub fn plain(text: &str) -> String {
    text.trim()
        .chars()
        .map(|c| match c {
            '\n' => c,
            '\u{2010}' | '\u{2011}' => '-',
            c if c.is_whitespace() => ' ',
            c => c,
        })
        .collect()
}

/// Every fact of the case in `text`, or what is missing: each item's
/// numbers in order (others may come between), the ticket as the one
/// dollar amount, one weight in lbs per item, the class, the cities and
/// the name.
pub fn check(p: &Parts, text: &str) -> Result<(), String> {
    let found: Vec<i64> = text.split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty()).filter_map(|s| s.parse().ok()).collect();
    let mut at = found.iter();
    for (i, n) in p.numbers.iter().enumerate() {
        for x in n {
            if !at.any(|f| f == x) {
                return Err(format!("item {}: {x} is missing or out of order", i + 1));
            }
        }
    }
    let dollars: Vec<&str> = text.split('$').skip(1).collect();
    let amount = |s: &str| s.chars().take_while(char::is_ascii_digit).collect::<String>();
    if dollars.len() != 1 || amount(dollars[0]) != p.ticket.to_string() {
        return Err(format!("{} dollar amounts, the ticket is ${}", dollars.len(), p.ticket));
    }
    let lower = text.to_lowercase();
    let words: Vec<&str> = lower.split(|c: char| !c.is_ascii_alphabetic()).filter(|w| !w.is_empty()).collect();
    if let Some(w) = words.iter().find(|w| matches!(**w, "dollar" | "dollars" | "usd" | "pound" | "pounds" | "kg" | "kgs" | "cent" | "cents")) {
        return Err(format!("it says `{w}`"));
    }
    for unit in [&["lbs"][..], &["inch", "inches"]] {
        let n = words.iter().filter(|w| unit.contains(*w)).count();
        if n != p.items.len() {
            return Err(format!("{n} times `{}` for {} items", unit[0], p.items.len()));
        }
    }
    for (what, s) in [("class", format!("{} Class", p.class)), ("city", p.from.clone()), ("city", p.to.clone()), ("name", p.name.clone())] {
        if !text.contains(&s) {
            return Err(format!("no {what} `{s}`"));
        }
    }
    Ok(())
}

/// `eval/rules/airline/variants`.
pub fn dir() -> PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../variants")
}

/// The kept variants of one kind, by case id.
pub fn load(kind: &str) -> Result<BTreeMap<String, String>, String> {
    let p = dir().join(format!("{kind}.jsonl"));
    let Ok(text) = std::fs::read_to_string(&p) else { return Ok(BTreeMap::new()) };
    let mut out = BTreeMap::new();
    for l in text.lines() {
        let v: serde_json::Value = serde_json::from_str(l).map_err(|e| format!("{}: {e}", p.display()))?;
        out.insert(v["id"].as_str().ok_or("no id")?.to_string(), v["text"].as_str().ok_or("no text")?.to_string());
    }
    Ok(out)
}

pub fn save(kind: &str, variants: &BTreeMap<String, String>) -> Result<(), String> {
    std::fs::create_dir_all(dir()).map_err(|e| e.to_string())?;
    let out: String = variants.iter().map(|(id, text)| format!("{}\n", serde_json::json!({"id": id, "text": text}))).collect();
    std::fs::write(dir().join(format!("{kind}.jsonl")), out).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cases() -> Vec<Case> {
        crate::data::cases(&crate::data_dir()).unwrap()
    }

    #[test]
    fn every_case_reads_back_as_its_facts() {
        for c in cases() {
            let p = parts(&c.text).unwrap_or_else(|| panic!("{}", c.id));
            assert_eq!(p.class, c.facts.class, "{}", c.id);
            assert_eq!(p.ticket, c.facts.ticket, "{}", c.id);
            let bags: Vec<[i64; 4]> = c.facts.bags.iter().map(|b| [b.length, b.width, b.height, b.weight]).collect();
            assert_eq!(p.numbers[1..], bags[..], "{}", c.id);
            assert_eq!(check(&p, &c.text), Ok(()), "{}", c.id);
        }
    }

    /// The order variant keeps the carry-on first and every checked bag,
    /// so its answer is the case's.
    #[test]
    fn the_order_variant_keeps_the_carry_on_first_and_every_bag() {
        let mut moved = 0;
        for c in cases() {
            let v = order(&c).unwrap();
            let p = parts(&c.text).unwrap();
            assert!(v.contains(&format!("\n1. {};\n", p.items[0])), "{}", c.id);
            let mut want = p.items.clone();
            let mut got: Vec<String> = v.lines().skip(1).map(|l| l.split_once(". ").unwrap().1.trim_end_matches(';').to_string()).collect();
            moved += usize::from(got != want);
            want.sort();
            got.sort();
            assert_eq!(got, want, "{}", c.id);
            assert!(v.starts_with(&format!("{}'s flight ticket is ${}.", p.name, p.ticket)));
        }
        assert!(moved > 250, "only {moved} cases had their bags moved");
    }

    #[test]
    fn the_check_finds_what_is_missing() {
        let c = &cases()[0];
        let p = parts(&c.text).unwrap();
        let t = &c.text;
        assert!(check(&p, &t.replacen("10 lbs", "11 lbs", 1)).is_err());
        assert!(check(&p, &format!("{t} Her taxi cost $20.")).is_err());
        assert!(check(&p, &format!("{t} She also has a coat of 3 lbs.")).is_err());
        assert!(check(&p, &t.replace("Main Cabin Class", "Main Cabin")).is_err());
        assert!(check(&p, &t.replace("Main Cabin", "Main\u{202f}Cabin")).is_err());
        assert_eq!(check(&p, &plain(&t.replace("Main Cabin", "Main\u{202f}Cabin"))), Ok(()));
        assert_eq!(plain("Port\u{2011}au\u{2011}Prince"), "Port-au-Prince");
        assert!(check(&p, &format!("{t} Her flight, AA 1432, leaves at 7:45 from the center, past a background of hills.")).is_ok());
    }
}
