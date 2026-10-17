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
    assert!(e[0].contains("rule cap: \"may not exceed\" is not a Suture phrase\n"), "{e:?}");
    assert!(e[0].contains("did you mean:     the total amount of each Order's Refunds is at most its total"), "{e:?}");
}

#[test]
fn the_longest_phrase_wins_and_case_is_ignored() {
    let e = errors(&format!("{SHOP}rule cap: the total amount of each Order's Refunds Is Less Than Or Equal To its total\n"));
    assert!(e[0].contains("\"is less than or equal to\" is not a Suture phrase\n"), "{e:?}");
}

#[test]
fn a_suggestion_that_does_not_compile_is_not_shown() {
    let e = errors(&format!("{SHOP}rule cap: the total amount of each Order's Refunds may not exceed its weight\n"));
    assert!(e[0].contains("\"may not exceed\" is not a Suture phrase; write \"is at most\""), "{e:?}");
    assert!(!e[0].contains("did you mean"), "{e:?}");
}

fn shop_with(extra: &str) -> Vec<String> {
    let shop = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../examples/shop/shop.st")).unwrap();
    let files = vec![("shop.st".to_string(), shop), ("bad.st".to_string(), extra.to_string())];
    compile(&files).err().unwrap_or_default().iter().map(|e| e.to_string()).collect()
}

#[test]
fn the_example_of_section_3_5() {
    let e = shop_with("rule cap2: refunds of an Order can't be more than its total\n");
    assert_eq!(
        e,
        ["bad.st:1: rule cap2: \"can't be more than\" is not a Suture phrase
  closest pattern:  the total ⟨attr⟩ of each ⟨Kind⟩'s ⟨Kinds⟩ is at most ⟨num⟩
  did you mean:     the total amount of each Order's Refunds is at most its total"]
    );
}

/// Golden errors (spec §7.5): each wrong sentence of `golden/⟨name⟩.st`,
/// with the shop, gives the message in `golden/⟨name⟩.errors`, word for
/// word. `UPDATE_GOLDEN=1` rewrites the expected files.
fn golden(name: &str) {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden");
    let got = shop_with(&std::fs::read_to_string(format!("{dir}/{name}.st")).unwrap()).join("\n") + "\n";
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(format!("{dir}/{name}.errors"), &got).unwrap();
    }
    assert_eq!(got, std::fs::read_to_string(format!("{dir}/{name}.errors")).unwrap());
}

#[test]
fn golden_sentence_errors() {
    golden("bad");
}

#[test]
fn golden_name_errors() {
    golden("names");
}

#[test]
fn odd_input_never_panics() {
    for s in ["rule", "rule x", "rule x:", "rule x: can't", "rule é: no Ördér can't be", "define", "when: isn't", "rule x: the total", "rule x: no no no unless", "kind K { a: Text }\nrule x: every K's a can't be more than \"ü\""] {
        let _ = shop_with(&format!("{s}\n"));
    }
    assert_eq!(tokens("Ada's order's").unwrap(), [Tok::Word("Ada".into()), Tok::Poss, Tok::Word("order".into()), Tok::Poss]);
}
