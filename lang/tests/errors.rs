//! Slice 7: errors and suggestions (spec §3.5, appendix H).

use suture_lang::{compile, tokens, Tok};

fn errors(src: &str) -> Vec<String> {
    let files = vec![("t.st".to_string(), format!("module t version 1, currency EUR\n{src}"))];
    compile(&files).err().unwrap_or_default().iter().map(|e| e.to_string()).collect()
}

const SHOP: &str = "kind Order  { total: Money, paid: Bool = no }
kind Refund { order: Order, amount: Money }
Order needs Refund
";

#[test]
fn contractions_are_one_word() {
    assert_eq!(tokens("can't doesn't").unwrap(), [Tok::Word("can't".into()), Tok::Word("doesn't".into())]);
    assert_eq!(tokens("its order's total").unwrap()[1..3], [Tok::Word("order".into()), Tok::Poss]);
}

#[test]
fn every_bad_statement_is_reported() {
    let e = errors(&format!("{SHOP}rule a: every Order bogus\nrule b: no Order frobs\n"));
    assert_eq!(e.len(), 2, "{e:?}");
    assert!(e[0].contains(":5:") && e[1].contains(":6:"), "{e:?}");
}

#[test]
fn a_table_phrase_names_the_canonical_one() {
    let e = errors(&format!("{SHOP}rule cap: the total amount of each Order's Refunds may not exceed its total\n"));
    assert_eq!(e.len(), 1, "{e:?}");
    assert!(e[0].contains("rule cap: \"may not exceed\" is not a Suture phrase; write \"is at most\""), "{e:?}");
    assert!(e[0].contains("did you mean:     the total amount of each Order's Refunds is at most its total"), "{e:?}");
}

#[test]
fn the_longest_phrase_wins_and_case_is_ignored() {
    let e = errors(&format!("{SHOP}rule cap: the total amount of each Order's Refunds Is Less Than Or Equal To its total\n"));
    assert!(e[0].contains("\"is less than or equal to\" is not a Suture phrase; write \"is at most\""), "{e:?}");
}

#[test]
fn a_suggestion_that_does_not_compile_is_not_shown() {
    let e = errors(&format!("{SHOP}rule cap: the total weight of each Order's Refunds may not exceed its total\n"));
    assert!(e[0].contains("\"may not exceed\" is not a Suture phrase"), "{e:?}");
    assert!(!e[0].contains("did you mean"), "{e:?}");
}
