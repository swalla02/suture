//! Slice 63: a `prefer` can use a define, join `whose` clauses with `and`
//! or `or`, and rank within a margin of the best (spec appendix BN).

use suture_runtime::host::Host;
use suture_runtime::state::{Bind, Val};

const HEAD: &str = "module pick version 1, currency EUR, candidates 64
role engine
kind Move { ply: Count, name: Text, hangs: Count, captures: Count, exposing: Bool, value: Money, score: Count }
kind Turn { ply: Count }
kind Pick { turn: Turn }
Turn needs Pick
Pick uses a Move where the Move's ply is the Turn's ply
define: a Move is safe when its hangs is at most its captures and it is not exposing
admit enter a new Move by engine
admit start a new Turn by engine
";

/// (name, hangs, captures, exposing, value in cents, score)
type M = (&'static str, i64, i64, bool, i64, i64);

/// The Move that `prefer` picks among `moves`.
fn pick(prefer: &str, moves: &[M]) -> String {
    let src = format!("{HEAD}{prefer}\n");
    let prog = suture_lang::compile(&[("pick.st".to_string(), src)]).unwrap_or_else(|e| panic!("{:?}", e.iter().map(|e| &e.msg).collect::<Vec<_>>()));
    let mut h = Host::new(prog);
    for (i, (name, hangs, captures, exposing, value, score)) in moves.iter().enumerate() {
        let attrs = vec![
            ("ply".to_string(), Val::Count(0)),
            ("name".to_string(), Val::Text(name.to_string())),
            ("hangs".to_string(), Val::Count(*hangs)),
            ("captures".to_string(), Val::Count(*captures)),
            ("exposing".to_string(), Val::Bool(*exposing)),
            ("value".to_string(), Val::Money(*value)),
            ("score".to_string(), Val::Count(*score)),
        ];
        h.create("Move", &format!("M{i}"), &attrs).unwrap();
    }
    h.create("Turn", "T0", &[("ply".to_string(), Val::Count(0))]).unwrap();
    let unit = h.unit("T0").unwrap();
    let to = unit.root.parts.iter().flat_map(|p| p.binds.values()).find_map(|b| match b {
        Bind::Granted { to, .. } => Some(to.clone()),
        _ => None,
    });
    let i: usize = to.expect("a Move was granted")[1..].parse().unwrap();
    moves[i].0.to_string()
}

const MOVES: &[M] = &[("hang", 5, 0, false, 900, 900), ("expose", 0, 0, true, 800, 800), ("quiet", 0, 0, false, 0, 100), ("trade", 3, 3, false, -10, 300)];

#[test]
fn a_prefer_reads_a_define_of_the_candidate() {
    assert_eq!(pick("prefer Move: those that are safe, then highest score", MOVES), "trade");
    assert_eq!(pick("prefer Move: those that are not safe, then lowest score", MOVES), "expose");
}

#[test]
fn whose_clauses_join_with_or_and_and() {
    assert_eq!(pick("prefer Move: those whose hangs is 0 or whose captures is at least 1, then highest score", MOVES), "expose");
    assert_eq!(pick("prefer Move: those whose hangs is 0 and whose score is less than 500, then highest score", MOVES), "quiet");
}

#[test]
fn and_and_or_are_not_mixed_in_one_clause() {
    let src = format!("{HEAD}prefer Move: those whose hangs is 0 or whose captures is 1 and whose score is 1, then highest score\n");
    let e = suture_lang::compile(&[("pick.st".to_string(), src)]).unwrap_err();
    assert!(e[0].msg.contains("mixed"), "{}", e[0].msg);
}

#[test]
fn within_a_margin_the_next_criterion_decides() {
    let moves: &[M] = &[("best", 0, 0, false, 300, 100), ("near", 0, 0, false, 260, 900), ("far", 0, 0, false, 100, 1000)];
    assert_eq!(pick("prefer Move: highest value, then highest score", moves), "best");
    assert_eq!(pick("prefer Move: highest value within 0.50 EUR, then highest score", moves), "near");
    assert_eq!(pick("prefer Move: highest value within 3 EUR, then highest score", moves), "far");
    assert_eq!(pick("prefer Move: lowest score within 100, then highest value", moves), "best");
}

#[test]
fn a_margin_has_the_type_of_what_it_ranks() {
    let src = format!("{HEAD}prefer Move: highest value within 3, then highest score\n");
    let e = suture_lang::compile(&[("pick.st".to_string(), src)]).unwrap_err();
    assert!(e[0].msg.contains("margin"), "{}", e[0].msg);
}

#[test]
fn money_between_minus_one_and_zero_keeps_its_sign() {
    assert_eq!(Val::Money(-57).show("EUR"), "-0.57 EUR");
    assert_eq!(Val::Money(-157).show("EUR"), "-1.57 EUR");
    assert_eq!(Val::Money(57).show("EUR"), "0.57 EUR");
}

#[test]
fn a_bool_counts_one_or_zero_in_a_formula() {
    let head = HEAD.replace("score: Count }", "score: Count, plan: Count = score + exposing * 1000 }");
    let src = format!("{head}prefer Move: highest plan\n");
    let prog = suture_lang::compile(&[("pick.st".to_string(), src)]).unwrap_or_else(|e| panic!("{:?}", e.iter().map(|e| &e.msg).collect::<Vec<_>>()));
    let mut h = Host::new(prog);
    for (i, (exposing, score)) in [(false, 900), (true, 10), (false, 500)].iter().enumerate() {
        let attrs = vec![
            ("ply".to_string(), Val::Count(0)),
            ("name".to_string(), Val::Text(format!("m{i}"))),
            ("hangs".to_string(), Val::Count(0)),
            ("captures".to_string(), Val::Count(0)),
            ("exposing".to_string(), Val::Bool(*exposing)),
            ("value".to_string(), Val::Money(0)),
            ("score".to_string(), Val::Count(*score)),
        ];
        h.create("Move", &format!("M{i}"), &attrs).unwrap();
    }
    h.create("Turn", "T0", &[("ply".to_string(), Val::Count(0))]).unwrap();
    let granted = h.unit("T0").unwrap().root.parts.iter().flat_map(|p| p.binds.values()).find_map(|b| match b {
        Bind::Granted { to, .. } => Some(to.clone()),
        _ => None,
    });
    assert_eq!(granted.as_deref(), Some("M1"));
}

#[test]
fn a_kind_may_list_its_attributes_one_per_line() {
    let src = "module t version 1, currency EUR\nkind Move {\n  -- what it does\n  hangs: Count,\n  captures: Count\n}\nkind Turn { ply: Count }\n";
    let prog = suture_lang::compile(&[("t.st".to_string(), src.to_string())]).unwrap_or_else(|e| panic!("{:?}", e.iter().map(|e| &e.msg).collect::<Vec<_>>()));
    assert_eq!(prog.kind("Move").unwrap().attrs.len(), 2);
}
