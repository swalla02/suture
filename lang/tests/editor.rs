//! Slice 15: what an editor shows (spec appendix P).

use suture_lang::editor::{complete, diagnostics, hover, Diagnostic};

fn tickets() -> String {
    std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../examples/tickets/tickets.st")).unwrap()
}

/// The labels offered at the end of `line`, added to the tickets program.
fn offered(line: &str) -> Vec<String> {
    let text = tickets();
    let p = suture_lang::compile(&[("t.st".into(), text.clone())]).unwrap();
    let t = format!("{text}\n{line}");
    complete(Some(&p), &t, t.lines().count(), line.chars().count()).into_iter().map(|c| c.label).collect()
}

#[test]
fn errors_and_warnings_come_on_their_lines() {
    let files = vec![("t.st".to_string(), tickets().replace("rule early: no Booking", "rule early: no Bookin"))];
    let (ds, prog) = diagnostics(&files);
    assert!(prog.is_none());
    let early = tickets().lines().position(|l| l.starts_with("rule early")).unwrap() + 1;
    assert!(ds.iter().any(|d| d.line == early && !d.warning), "{ds:?}");
    let src = "module w version 1, currency EUR\nkind O { }\nkind A { o: O }\nkind B { o: O }\nO needs D\nD is A or B\n";
    let (ds, prog) = diagnostics(&[("w.st".into(), src.into())]);
    assert!(prog.is_some());
    assert_eq!(ds, [Diagnostic { file: "w.st".into(), line: 1, msg: "D has no `prefer`; its alternatives are tried in the order declared: A, B".into(), warning: true }]);
}

#[test]
fn completion_offers_what_the_grammar_takes_next() {
    let every = offered("rule x: every ");
    assert!(every.contains(&"Booking".into()) && every.contains(&"paid".into()), "{every:?}");
    assert!(!every.contains(&"via".into()), "{every:?}");
    assert_eq!(offered("rule x: every Booking's "), ["quantity", "total", "cancelled", "fan", "show"]);
    let is = offered("rule x: every Booking's quantity is ");
    for want in ["at most", "at least", "more than", "less than", "not"] {
        assert!(is.contains(&want.into()), "{want}: {is:?}");
    }
    assert_eq!(offered("rule x: every Booking's quantity is at "), ["least", "most"]);
    assert_eq!(offered("rule x: every Booking's quantity is at m"), ["most"]);
    assert_eq!(offered("effect charge Payment "), ["via"]);
    assert_eq!(offered("effect charge Payment via "), ["mailer", "payments"]);
    assert_eq!(offered("kind A { x: Text "), ["personal", "secret"]);
    let first = offered("");
    assert!(first.starts_with(&["kind".into(), "needs".into(), "effect".into()]) && first.contains(&"Booking".into()), "{first:?}");
    assert_eq!(offered("al"), ["allow"]);
    assert!(offered("-- every ").is_empty(), "nothing inside a comment");
    let text = tickets();
    let p = suture_lang::compile(&[("t.st".into(), text.clone())]).unwrap();
    let t = format!("{text}\nrule x: every Booking'");
    let poss = complete(Some(&p), &t, t.lines().count(), "rule x: every Booking'".len());
    assert_eq!(poss.first().map(|c| c.insert.as_str()), Some("s quantity"), "{poss:?}");
}

#[test]
fn hover_reads_back_the_statement() {
    let text = tickets();
    let p = suture_lang::compile(&[("t.st".into(), text.clone())]).unwrap();
    let at = |prefix: &str| text.lines().position(|l| l.starts_with(prefix)).unwrap() + 1;
    let rule = hover(&p, &text, at("rule seats"), 2).unwrap();
    assert!(rule.contains("in other words: each session on its own: the total quantity taken from it is at most its seats"), "{rule}");
    let kind = hover(&p, &text, at("kind Booking"), 7).unwrap();
    assert!(kind.contains("**kind Booking** (Bookings)") && kind.contains("- total: Money (computed)") && kind.contains("needs Payment, Ticket"), "{kind}");
    let effect = hover(&p, &text, at("effect charge"), 2).unwrap();
    assert!(effect.contains("charge Payment via payments within 30 seconds; it answers charged | declined"), "{effect}");
    // A kind's name reads back as the kind, wherever it is.
    let line = text.lines().nth(at("rule early") - 1).unwrap();
    let col = line.find("Ticket").unwrap() + 1;
    assert!(hover(&p, &text, at("rule early"), col).unwrap().starts_with("**kind Ticket**"));
    assert_eq!(hover(&p, &text, 1, 3), None, "a comment has no read-back");
    assert_eq!(hover(&p, &text, at("rule seats") + 2, 0), None, "nor has a blank line after a rule");
}
