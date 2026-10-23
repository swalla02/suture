//! Slice 51: no statement exhausts the parser's stack (spec appendix BB.2.4).

#[test]
fn deep_nesting_is_an_error_not_a_crash() {
    let deep = format!("module m version 1, currency EUR\nkind A {{ x: Count }}\nrule r: every A's x is at most {}1\n", "the total of ".repeat(100_000));
    // A thread with a small stack, so a recursion that is not bounded fails here.
    let errors = std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(move || suture_lang::compile(&[("m.st".to_string(), deep)]).err().unwrap_or_default().iter().map(|e| e.to_string()).collect::<Vec<_>>())
        .unwrap()
        .join()
        .unwrap();
    assert!(errors.iter().any(|e| e.contains("calculations or conditions inside one another")), "{errors:?}");
}
