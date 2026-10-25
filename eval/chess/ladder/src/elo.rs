//! Ratings from game results: the most likely Elo of every player, with the
//! Stockfish anchors held at the rating they were capped to.

use std::collections::BTreeMap;

pub struct Game {
    pub white: String,
    pub black: String,
    /// White's score: 1, 0.5 or 0.
    pub score: f64,
}

pub struct Rating {
    pub elo: f64,
    /// Half the 95% interval.
    pub ci: f64,
    pub games: usize,
    pub score: f64,
}

/// `stockfish 1500` is held at 1500; everyone else is fitted. Each player
/// also gets one virtual draw against each opponent it met, so that one who
/// won or lost every game still gets a finite rating.
pub fn fit(games: &[Game]) -> BTreeMap<String, Rating> {
    let anchor = |p: &str| p.strip_prefix("stockfish ").and_then(|e| e.parse::<f64>().ok());
    let mut names: Vec<String> = games.iter().flat_map(|g| [g.white.clone(), g.black.clone()]).collect();
    names.sort();
    names.dedup();
    let mut r: BTreeMap<String, f64> = names.iter().map(|n| (n.clone(), anchor(n).unwrap_or(1500.0))).collect();
    let mut pairs: Vec<(String, String, f64)> = vec![];
    for g in games {
        pairs.push((g.white.clone(), g.black.clone(), g.score));
    }
    let mut met: Vec<(String, String)> = games.iter().map(|g| if g.white < g.black { (g.white.clone(), g.black.clone()) } else { (g.black.clone(), g.white.clone()) }).collect();
    met.sort();
    met.dedup();
    for (a, b) in &met {
        pairs.push((a.clone(), b.clone(), 0.5));
    }
    let k = 400.0 / std::f64::consts::LN_10;
    let expect = |ra: f64, rb: f64| 1.0 / (1.0 + 10f64.powf((rb - ra) / 400.0));
    for _ in 0..500 {
        let mut moved = 0.0f64;
        for n in &names {
            if anchor(n).is_some() {
                continue;
            }
            let (mut grad, mut info) = (0.0, 0.0);
            for (a, b, s) in &pairs {
                let (me, them, s) = if a == n { (a, b, *s) } else if b == n { (b, a, 1.0 - s) } else { continue };
                let e = expect(r[me], r[them]);
                grad += s - e;
                info += e * (1.0 - e);
            }
            if info > 0.0 {
                let step = (k * grad / info).clamp(-100.0, 100.0);
                *r.get_mut(n).expect("a player") += step;
                moved = moved.max(step.abs());
            }
        }
        if moved < 0.01 {
            break;
        }
    }
    names
        .iter()
        .map(|n| {
            let (mut info, mut played, mut score) = (0.0, 0, 0.0);
            for g in games {
                let (them, s) = if &g.white == n { (&g.black, g.score) } else if &g.black == n { (&g.white, 1.0 - g.score) } else { continue };
                let e = expect(r[n], r[them]);
                info += e * (1.0 - e);
                played += 1;
                score += s;
            }
            let ci = if anchor(n).is_some() || info == 0.0 { 0.0 } else { 1.96 * k / info.sqrt() };
            (n.clone(), Rating { elo: r[n], ci, games: played, score })
        })
        .collect()
}
