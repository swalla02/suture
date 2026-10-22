//! `suture help`: the agent book, docs/agents/suture.md, compiled into the
//! binary so the command and the book never differ (appendix AO).
//! runtime/tests/guide.rs checks every example in it.

const BOOK: &str = include_str!("../../docs/agents/suture.md");

/// The book's sections: each `## ⟨topic⟩: ⟨what it covers⟩` to the next.
fn sections() -> Vec<(&'static str, &'static str, &'static str)> {
    let mut out = vec![];
    let starts: Vec<usize> = BOOK.match_indices("\n## ").map(|(i, _)| i + 1).collect();
    for (k, &at) in starts.iter().enumerate() {
        let text = &BOOK[at..starts.get(k + 1).copied().unwrap_or(BOOK.len())];
        let head = text.lines().next().unwrap_or("").trim_start_matches("## ");
        let (topic, what) = head.split_once(": ").unwrap_or((head, ""));
        out.push((topic, what, text));
    }
    out
}

/// One section, the whole book for `all`, or the list of sections.
pub fn help(topic: Option<&str>) -> Result<String, String> {
    let list = || {
        let mut out = format!("suture help all prints the whole book ({} KB, about {}k tokens); suture help ⟨topic⟩ one section:\n\n", BOOK.len() / 1000, BOOK.len() / 4000);
        for (t, what, _) in sections() {
            out += &format!("  {t:<13} {what}\n");
        }
        out
    };
    match topic {
        None => Ok(list()),
        Some("all") => Ok(BOOK.to_string()),
        Some(t) => sections().into_iter().find(|s| s.0 == t).map(|s| s.2.to_string()).ok_or_else(|| format!("no section `{t}`\n\n{}", list())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_book_fits_in_a_context_and_every_section_is_reachable() {
        // The book is meant to be read whole: keep it under about 5k tokens.
        assert!(BOOK.len() < 20_000, "the book has grown to {} bytes", BOOK.len());
        let topics: Vec<&str> = sections().iter().map(|s| s.0).collect();
        assert_eq!(topics, ["loop", "example", "declarations", "sentences", "semantics", "recipes", "limits", "scenarios", "models", "serve"]);
        assert!(help(Some("recipes")).unwrap().starts_with("## recipes:"));
        assert!(help(Some("nope")).unwrap_err().starts_with("no section `nope`"));
    }
}
